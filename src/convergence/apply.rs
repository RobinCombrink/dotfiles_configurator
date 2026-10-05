use {
    crate::{
        configuration::{Identity, Migration, Notice, Resource, ResourceKind},
        configuration_source::WriteSource,
        confirmation::{Confirm, Confirmation},
        convergence::{
            Blocked, Change, ChangeSet, Lane, SourceReadings, build_stage,
            converge::{Download, Fetched, converge},
            plan,
        },
        currency::{self, SelfReplacement},
        desired_state::{DesiredState, ResolvedResource},
        machine::{Placement, WriteInvocation, WriteMachine},
        reporting::{Closing, Entry, EntryOutcome, RunReport},
    },
    anyhow::anyhow,
    futures::future,
    std::{
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
    },
};

#[derive(Debug)]
pub struct Failure {
    pub resource: ResolvedResource,
    pub error: anyhow::Error,
}

#[derive(Debug)]
pub struct Held {
    pub resource: ResolvedResource,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct ApplyOutcome {
    pub converged: Vec<ResolvedResource>,
    pub failed: Vec<Failure>,
    pub held: Vec<Held>,
    pub blocked: Vec<Blocked>,
    pub unverified: Vec<Change>,
    pub notices: Vec<Notice>,
    pub migrated: Vec<Migration>,
    pub passes: usize,
}

impl ApplyOutcome {
    pub fn is_converged(&self) -> bool {
        self.failed.is_empty()
            && self.held.is_empty()
            && self.blocked.is_empty()
            && self.unverified.is_empty()
    }
}

impl ApplyOutcome {
    fn closing(&self) -> Closing {
        let entry_of =
            |resource: &ResolvedResource| Entry::new(Lane::of(resource), resource.declared());
        Closing {
            converged: self.converged.len(),
            failed: self.failed.len(),
            held: self.held.len(),
            blocked: self
                .blocked
                .iter()
                .map(|blocked| (entry_of(&blocked.resource), blocked.impediment.to_string()))
                .collect(),
            did_not_take: self
                .unverified
                .iter()
                .map(|change| {
                    (
                        entry_of(&change.resource),
                        format!("converged, but still {}", change.reason),
                    )
                })
                .collect(),
            migrated: self.migrated.iter().map(ToString::to_string).collect(),
            notices: self.notices.iter().map(ToString::to_string).collect(),
            passes: self.passes,
        }
    }
}

#[derive(Debug)]
pub enum Enactment {
    Enacted(ApplyOutcome),
    Declined,
    ReplacedItself,
}

const ENACT_THE_CHANGE_SET: &str = "Enact this change set?";

// ADR 0004, ADR 0013
pub async fn apply(
    desired_state: &DesiredState,
    machine: &(impl WriteMachine + WriteSource),
    report: &RunReport,
    operator: &impl Confirm,
    self_replacement: &SelfReplacement,
) -> anyhow::Result<Enactment> {
    {
        let _doing = report.doing("removing the binaries earlier runs replaced");
        machine.sweep_superseded_images();
    }

    let (first_change_set, first_readings) = plan(desired_state, machine, report).await?;

    if first_change_set.would_enact_something() {
        report.announce(&first_change_set.to_string());

        if operator.confirmation(ENACT_THE_CHANGE_SET) == Confirmation::Declined {
            report.announce("Declined. Nothing on this machine was changed.");
            return Ok(Enactment::Declined);
        }
    }

    let mut converged: Vec<ResolvedResource> = Vec::new();
    let mut failed: Vec<Failure> = Vec::new();
    let mut held: Vec<Held> = Vec::new();
    let mut handled: BTreeSet<Handled> = BTreeSet::new();
    let mut passes = 0;

    for migration in &desired_state.migrations {
        let _doing = report.doing(format!("rewriting {migration}"));
        machine.rewrite(migration)?;
    }

    let mut confirmed = Some((first_change_set, first_readings));
    let change_set = loop {
        let (change_set, readings) = match confirmed.take() {
            Some(already_planned) => already_planned,
            None => plan(desired_state, machine, report).await?,
        };
        passes += 1;

        let pass = attempt(
            &change_set,
            &readings,
            machine,
            report,
            &handled,
            self_replacement,
        )
        .await;
        report.note(&format!(
            "pass {passes} converged {} resource(s)",
            pass.converged.len()
        ));

        if pass.replaced_itself {
            converged.extend(pass.converged);
            failed.extend(pass.failed);
            held.extend(pass.held);
            let handed_over = ApplyOutcome {
                converged,
                failed,
                held,
                blocked: Vec::new(),
                unverified: Vec::new(),
                notices: Vec::new(),
                migrated: desired_state.migrations.clone(),
                passes,
            };
            report.conclude(handed_over.closing());
            report.announce(&format!(
                "Installed the latest release over {}. The rest of this run belongs to it.",
                currency::this_build()
            ));
            return Ok(Enactment::ReplacedItself);
        }

        let productive = !pass.converged.is_empty();
        handled.extend(pass.handled);
        converged.extend(pass.converged);
        failed.extend(pass.failed);
        held.extend(pass.held);

        if !productive {
            break change_set;
        }
    };

    let unverified = change_set
        .changes
        .iter()
        .filter(|change| {
            converged.contains(&change.resource) && change.resource.declared().can_be_read_back()
        })
        .cloned()
        .collect();

    let mut notices = change_set.notices;
    notices.extend(notice_of_an_environment_change(&converged));

    let outcome = ApplyOutcome {
        converged,
        failed,
        held,
        blocked: change_set.blocked,
        unverified,
        notices,
        migrated: desired_state.migrations.clone(),
        passes,
    };
    report.conclude(outcome.closing());
    Ok(Enactment::Enacted(outcome))
}

// ADR 0017
fn notice_of_an_environment_change(converged: &[ResolvedResource]) -> Option<Notice> {
    let changed = converged
        .iter()
        .any(|resource| resource.kind() == ResourceKind::EnvironmentVariable);

    changed.then_some(Notice::EnvironmentChanged)
}

/// The work a resource performs, which is what makes two declarations of it the same work. A
/// command's work is its arguments and the shell they are run through; the presence check that
/// guards it is not part of it, having already spoken by the time a change exists.
///
/// ```
/// # use dotfiles_configurator::{
/// #     configuration::{Command, Resource, Shell},
/// #     convergence::apply::Invocation,
/// # };
/// let declared = |argument: &str, shell: Shell| {
///     Resource::Command(Command {
///         shell,
///         args: vec![argument.to_owned()],
///         presence_check: None,
///     })
/// };
/// assert_eq!(
///     Invocation::from(&declared("refresh-completions", Shell::Bash)),
///     Invocation::from(&declared("refresh-completions", Shell::Bash))
/// );
/// assert_ne!(
///     Invocation::from(&declared("refresh-completions", Shell::Bash)),
///     Invocation::from(&declared("sync-secrets", Shell::Bash))
/// );
/// assert_ne!(
///     Invocation::from(&declared("refresh-completions", Shell::Bash)),
///     Invocation::from(&declared("refresh-completions", Shell::PowerShell))
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Invocation(String);

impl From<&Resource> for Invocation {
    fn from(resource: &Resource) -> Self {
        match resource {
            Resource::Command(command) => {
                Invocation(format!("{:?} {}", command.shell, command.rendered()))
            }
            resource => Invocation(resource.to_string()),
        }
    }
}

/// What a pass has already accounted for, so that a later pass leaves it alone. A resource that
/// claims an identity is keyed by it; one that claims none — a command, and only a command — is
/// keyed by the work it performs, so that two configurations declaring the same work converge it
/// once.
///
/// ```
/// # use dotfiles_configurator::{
/// #     configuration::{Command, Identity, Resource, Shell},
/// #     convergence::apply::{Handled, Invocation},
/// # };
/// let refresh = Resource::Command(Command {
///     shell: Shell::Bash,
///     args: vec!["refresh-completions".to_owned()],
///     presence_check: None,
/// });
/// assert_eq!(
///     Handled::Claimed(Identity::MachineManifest),
///     Handled::Claimed(Identity::MachineManifest)
/// );
/// assert_ne!(
///     Handled::Claimed(Identity::MachineManifest),
///     Handled::Unclaimable(Invocation::from(&refresh))
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Handled {
    Claimed(Identity),
    Unclaimable(Invocation),
}

impl Handled {
    fn of(change: &Change, home_directory: &Path) -> Self {
        match change.resource.identity(home_directory) {
            Some(identity) => Handled::Claimed(identity),
            None => Handled::Unclaimable(Invocation::from(change.resource.declared())),
        }
    }
}

#[derive(Debug)]
enum Attempted {
    Converged,
    Held(PathBuf),
    Failed(anyhow::Error),
}

#[derive(Debug, Default)]
struct Pass {
    converged: Vec<ResolvedResource>,
    failed: Vec<Failure>,
    held: Vec<Held>,
    handled: BTreeSet<Handled>,
    replaced_itself: bool,
}

#[derive(Debug)]
struct Settled<'change> {
    position: usize,
    change: &'change Change,
    attempted: Attempted,
}

impl Pass {
    fn of(
        mut settled: Vec<Settled<'_>>,
        handled: BTreeSet<Handled>,
        replaced_itself: bool,
    ) -> Self {
        settled.sort_by_key(|settled| settled.position);

        let mut pass = Pass {
            handled,
            replaced_itself,
            ..Pass::default()
        };
        for Settled {
            change, attempted, ..
        } in settled
        {
            let resource = change.resource.clone();
            match attempted {
                Attempted::Converged => pass.converged.push(resource),
                Attempted::Held(path) => pass.held.push(Held { resource, path }),
                Attempted::Failed(error) => pass.failed.push(Failure { resource, error }),
            }
        }
        pass
    }
}

#[derive(Debug)]
struct Work<'change> {
    position: usize,
    change: &'change Change,
    fetched: Option<Fetched>,
}

impl<'change> Work<'change> {
    fn change(&self) -> &'change Change {
        self.change
    }

    async fn run(
        self,
        readings: &SourceReadings,
        machine: &impl WriteMachine,
        report: &RunReport,
    ) -> Settled<'change> {
        let Work {
            position,
            change,
            fetched,
        } = self;
        let attempted = match fetched {
            None => converge_one(change, readings, machine, report).await,
            Some(fetched) => install_fetched(change, fetched, machine, report).await,
        };

        Settled {
            position,
            change,
            attempted,
        }
    }
}

async fn attempt(
    change_set: &ChangeSet,
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
    handled: &BTreeSet<Handled>,
    self_replacement: &SelfReplacement,
) -> Pass {
    let mut claimed: BTreeSet<Handled> = BTreeSet::new();
    let pending: Vec<(usize, &Change)> = change_set
        .changes
        .iter()
        .enumerate()
        .filter(|(_, change)| {
            let key = Handled::of(change, machine.home_directory());
            !handled.contains(&key) && claimed.insert(key)
        })
        .collect();
    let in_lane = |lane: Lane| {
        pending
            .iter()
            .copied()
            .filter(move |(_, change)| Lane::of(&change.resource) == lane)
    };

    let mut settled: Vec<Settled> = Vec::new();
    for (position, change) in in_lane(Lane::Replacement) {
        let attempted = match refusal_to_replace_again(change, self_replacement) {
            Some(refusal) => refuse(change, refusal, report),
            None => converge_one(change, readings, machine, report).await,
        };
        let replaced_itself = match attempted {
            Attempted::Converged => true,
            Attempted::Held(_) | Attempted::Failed(_) => false,
        };
        settled.push(Settled {
            position,
            change,
            attempted,
        });
        if replaced_itself {
            return Pass::of(settled, claimed, true);
        }
    }

    for (position, change) in in_lane(Lane::Repositories) {
        settled.push(Settled {
            position,
            change,
            attempted: converge_one(change, readings, machine, report).await,
        });
    }

    let (mut fetched, failed_downloads) =
        fetch_ahead_of_the_lanes(&pending, readings, machine, report).await;
    settled.extend(failed_downloads);
    let mut work_in = |lane: Lane| -> Vec<Work> {
        in_lane(lane)
            .filter_map(
                |(position, change)| match Download::of(change.resource.declared()) {
                    None => Some(Work {
                        position,
                        change,
                        fetched: None,
                    }),
                    Some(_) => fetched.remove(&position).map(|fetched| Work {
                        position,
                        change,
                        fetched: Some(fetched),
                    }),
                },
            )
            .collect()
    };
    let (cargo, install, uv, instant) = (
        work_in(Lane::Cargo),
        work_in(Lane::Install),
        work_in(Lane::Uv),
        work_in(Lane::Instant),
    );

    let (cargo, install, uv, instant) = tokio::join!(
        cargo_lane(cargo, readings, machine, report),
        install_lane(install, readings, machine, report),
        in_order(uv, readings, machine, report),
        in_order(instant, readings, machine, report),
    );
    settled.extend(cargo.into_iter().chain(install).chain(uv).chain(instant));

    for (position, change) in in_lane(Lane::Commands) {
        settled.push(Settled {
            position,
            change,
            attempted: converge_one(change, readings, machine, report).await,
        });
    }

    Pass::of(settled, claimed, false)
}

async fn fetch_ahead_of_the_lanes<'change>(
    pending: &[(usize, &'change Change)],
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
) -> (BTreeMap<usize, Fetched>, Vec<Settled<'change>>) {
    let fetching = pending
        .iter()
        .filter(|(_, change)| Lane::of(&change.resource) != Lane::Replacement)
        .filter_map(|&(position, change)| {
            let download = Download::of(change.resource.declared())?;
            Some(async move {
                let entry = entry_of(change);
                let fetched = report
                    .converging(&entry, download.fetched(machine, readings))
                    .await;
                (position, change, entry, fetched)
            })
        });

    let mut ready = BTreeMap::new();
    let mut failed = Vec::new();
    for (position, change, entry, fetched) in future::join_all(fetching).await {
        match fetched {
            Ok(fetched) => {
                report.awaiting_its_lane(&entry);
                ready.insert(position, fetched);
            }
            Err(error) => failed.push(Settled {
                position,
                change,
                attempted: finish(change, &entry, Err(error), report),
            }),
        }
    }
    (ready, failed)
}

async fn in_order<'change>(
    work: Vec<Work<'change>>,
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
) -> Vec<Settled<'change>> {
    let mut settled = Vec::new();
    for item in work {
        settled.push(item.run(readings, machine, report).await);
    }
    settled
}

async fn cargo_lane<'change>(
    work: Vec<Work<'change>>,
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
) -> Vec<Settled<'change>> {
    build_stage::build(
        &build_stage::workspace_builds(work.iter().map(Work::change), readings),
        machine,
        report,
    )
    .await;

    in_order(work, readings, machine, report).await
}

async fn install_lane<'change>(
    work: Vec<Work<'change>>,
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
) -> Vec<Settled<'change>> {
    let (installers, winget): (Vec<Work>, Vec<Work>) =
        work.into_iter().partition(|item| item.fetched.is_some());

    if !winget.is_empty() {
        update_winget_sources(machine, report).await;
    }
    let mut settled = future::join_all(
        winget
            .into_iter()
            .map(|item| item.run(readings, machine, report)),
    )
    .await;
    settled.extend(in_order(installers, readings, machine, report).await);
    settled
}

async fn update_winget_sources(machine: &impl WriteMachine, report: &RunReport) {
    let entry = Entry::winget_sources();
    let updated = report
        .converging(&entry, machine.write(&WriteInvocation::UpdateWingetSources))
        .await;
    let Err(error) = updated else {
        report.entry_finished(&entry, EntryOutcome::Converged);
        return;
    };

    let reason = format!(
        "winget's sources could not be updated, so each install reads the sources winget already \
         holds: {error:#}"
    );
    report.note(&reason);
    report.entry_finished(&entry, EntryOutcome::Failed { reason });
}

fn refusal_to_replace_again(
    change: &Change,
    self_replacement: &SelfReplacement,
) -> Option<anyhow::Error> {
    let SelfReplacement::Spent { replaced } = self_replacement else {
        return None;
    };
    if !change.resource.replaces_the_running_build() {
        return None;
    }

    Some(anyhow!(
        "{} took this run over from {replaced} and still reads as behind its latest release ({}). \
         Replacing it a second time in one run could restart it without end.",
        currency::this_build(),
        change.reason
    ))
}

fn entry_of(change: &Change) -> Entry {
    Entry::new(Lane::of(&change.resource), change.resource.declared())
}

fn refuse(change: &Change, refusal: anyhow::Error, report: &RunReport) -> Attempted {
    report.note(&format!("FAILED {}: {refusal:#}", change.resource));
    report.entry_finished(
        &entry_of(change),
        EntryOutcome::Failed {
            reason: format!("{refusal:#}"),
        },
    );
    Attempted::Failed(refusal)
}

async fn converge_one(
    change: &Change,
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
) -> Attempted {
    let entry = entry_of(change);
    let outcome = report
        .converging(&entry, Box::pin(converge(change, machine, readings)))
        .await;

    finish(change, &entry, outcome, report)
}

async fn install_fetched(
    change: &Change,
    fetched: Fetched,
    machine: &impl WriteMachine,
    report: &RunReport,
) -> Attempted {
    let entry = entry_of(change);
    let outcome = report
        .resuming(&entry, Box::pin(fetched.installed(machine)))
        .await;

    finish(change, &entry, outcome, report)
}

fn finish(
    change: &Change,
    entry: &Entry,
    outcome: anyhow::Result<Placement>,
    report: &RunReport,
) -> Attempted {
    match outcome {
        Ok(Placement::Placed) => {
            report.note(&format!("converged {}", change.resource));
            report.entry_finished(entry, EntryOutcome::Converged);
            Attempted::Converged
        }
        Ok(Placement::Held(path)) => {
            let reason = format!("{} is being executed", path.display());
            report.note(&format!("HELD {}: {reason}", change.resource));
            report.entry_finished(entry, EntryOutcome::Held { reason });
            Attempted::Held(path)
        }
        Err(error) => {
            let reason = format!("{error:#}");
            report.note(&format!("FAILED {}: {reason}", change.resource));
            report.entry_finished(entry, EntryOutcome::Failed { reason });
            Attempted::Failed(error)
        }
    }
}

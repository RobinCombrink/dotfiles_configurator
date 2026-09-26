use {
    crate::{
        configuration::{Identity, Migration, Notice, Resource, ResourceKind},
        configuration_source::WriteSource,
        confirmation::{Confirm, Confirmation},
        convergence::{Blocked, Change, ChangeSet, SourceReadings, converge::converge, plan},
        currency::{self, SelfReplacement},
        desired_state::{DesiredState, ResolvedResource},
        machine::{Placement, WriteMachine},
        reporting::RunReport,
    },
    anyhow::anyhow,
    std::{
        collections::BTreeSet,
        fmt::Display,
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

impl Display for ApplyOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for resource in &self.converged {
            writeln!(formatter, "  converged {resource}")?;
        }
        for failure in &self.failed {
            writeln!(
                formatter,
                "  FAILED    {}: {:#}",
                failure.resource, failure.error
            )?;
        }
        for held in &self.held {
            writeln!(
                formatter,
                "  HELD      {} ({} is being executed)",
                held.resource,
                held.path.display()
            )?;
        }
        for blocked in &self.blocked {
            writeln!(
                formatter,
                "  BLOCKED   {} ({})",
                blocked.resource, blocked.impediment
            )?;
        }
        for change in &self.unverified {
            writeln!(
                formatter,
                "  UNDONE    {} (converged, but still {})",
                change.resource, change.reason
            )?;
        }
        for migration in &self.migrated {
            writeln!(formatter, "  migrated  {migration}")?;
        }
        for notice in &self.notices {
            writeln!(formatter, "  notice    {notice}")?;
        }
        write!(
            formatter,
            "\n{} converged, {} failed, {} held, {} still blocked, {} did not take, {} migrated, \
             over {} pass(es)",
            self.converged.len(),
            self.failed.len(),
            self.held.len(),
            self.blocked.len(),
            self.unverified.len(),
            self.migrated.len(),
            self.passes
        )
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

    Ok(Enactment::Enacted(ApplyOutcome {
        converged,
        failed,
        held,
        blocked: change_set.blocked,
        unverified,
        notices,
        migrated: desired_state.migrations.clone(),
        passes,
    }))
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

async fn attempt(
    change_set: &ChangeSet,
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
    handled: &BTreeSet<Handled>,
    self_replacement: &SelfReplacement,
) -> Pass {
    let mut pass = Pass::default();
    for change in &change_set.changes {
        let key = Handled::of(change, machine.home_directory());
        if handled.contains(&key) || pass.handled.contains(&key) {
            continue;
        }

        let attempted = attempt_one(change, readings, machine, report, self_replacement).await;
        let replaced_itself = match &attempted {
            Attempted::Converged => change.resource.replaces_the_running_build(),
            Attempted::Held(_) | Attempted::Failed(_) => false,
        };

        match attempted {
            Attempted::Converged => pass.converged.push(change.resource.clone()),
            Attempted::Held(path) => pass.held.push(Held {
                resource: change.resource.clone(),
                path,
            }),
            Attempted::Failed(error) => pass.failed.push(Failure {
                resource: change.resource.clone(),
                error,
            }),
        }
        pass.handled.insert(key);

        if replaced_itself {
            pass.replaced_itself = true;
            return pass;
        }
    }
    pass
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

async fn attempt_one(
    change: &Change,
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
    self_replacement: &SelfReplacement,
) -> Attempted {
    if let Some(refusal) = refusal_to_replace_again(change, self_replacement) {
        report.note(&format!("FAILED {}: {refusal:#}", change.resource));
        return Attempted::Failed(refusal);
    }

    let outcome = {
        let _doing = report.doing(format!("converging {}", change.resource));
        converge(&change.resource, machine, readings).await
    };

    match outcome {
        Ok(Placement::Placed) => {
            report.note(&format!("converged {}", change.resource));
            Attempted::Converged
        }
        Ok(Placement::Held(path)) => {
            report.note(&format!(
                "HELD {}: {} is being executed",
                change.resource,
                path.display()
            ));
            Attempted::Held(path)
        }
        Err(error) => {
            report.note(&format!("FAILED {}: {error:#}", change.resource));
            Attempted::Failed(error)
        }
    }
}

use {
    crate::{
        configuration::{Identity, Migration, Notice, Resource, ResourceKind},
        configuration_source::WriteSource,
        confirmation::{Confirm, Confirmation},
        convergence::{Blocked, Change, ChangeSet, SourceReadings, converge::converge, plan},
        desired_state::{DesiredState, ResolvedResource},
        machine::{Placement, WriteMachine},
        reporting::RunReport,
    },
    std::{
        collections::BTreeSet,
        fmt::Display,
        path::{Path, PathBuf},
    },
};

/// One resource whose convergence failed, together with what went wrong.
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

/// What an apply did, and what it could not do. Failures are collected rather than raised so that
/// one broken resource does not hide the state of every resource after it.
#[derive(Debug)]
pub struct ApplyOutcome {
    pub converged: Vec<ResolvedResource>,
    pub failed: Vec<Failure>,
    pub held: Vec<Held>,
    pub blocked: Vec<Blocked>,
    /// Resources that were converged without error and still read as drifted afterwards — an
    /// installer that exits zero without installing anything looks exactly like this.
    pub unverified: Vec<Change>,
    pub notices: Vec<Notice>,
    /// The documents this run rewrote a generation forward, which is neither a change nor a
    /// notice: it altered a configuration rather than the machine.
    pub migrated: Vec<Migration>,
    pub passes: usize,
}

impl ApplyOutcome {
    /// A machine is converged only when nothing failed, nothing is left unreadable, and every
    /// change that could be read back reads as done. A run that ends otherwise should not imply
    /// the machine is converged.
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
}

const ENACT_THE_CHANGE_SET: &str = "Enact this change set?";

// ADR 0004, ADR 0013
pub async fn apply(
    desired_state: &DesiredState,
    machine: &(impl WriteMachine + WriteSource),
    report: &RunReport,
    operator: &impl Confirm,
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

        let pass = attempt(&change_set, &readings, machine, report, &handled).await;
        report.note(&format!(
            "pass {passes} converged {} resource(s)",
            pass.converged.len()
        ));

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
    AlreadyHandled,
}

#[derive(Debug, Default)]
struct Pass {
    converged: Vec<ResolvedResource>,
    failed: Vec<Failure>,
    held: Vec<Held>,
    handled: BTreeSet<Handled>,
}

async fn attempt(
    change_set: &ChangeSet,
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
    handled: &BTreeSet<Handled>,
) -> Pass {
    let mut pass = Pass::default();
    for change in &change_set.changes {
        let key = Handled::of(change, machine.home_directory());

        match attempt_one(change, readings, machine, report, handled, &pass, &key).await {
            Attempted::AlreadyHandled => continue,
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
    }
    pass
}

async fn attempt_one(
    change: &Change,
    readings: &SourceReadings,
    machine: &impl WriteMachine,
    report: &RunReport,
    handled: &BTreeSet<Handled>,
    pass: &Pass,
    key: &Handled,
) -> Attempted {
    if handled.contains(key) || pass.handled.contains(key) {
        return Attempted::AlreadyHandled;
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

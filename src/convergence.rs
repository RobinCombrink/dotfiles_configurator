use {
    crate::{
        configuration::{Migration, Notice, Requirement, ResourceKind, Symlink},
        desired_state::{DesiredState, ResolvedResource},
        reporting::RunReport,
    },
    std::fmt::Display,
};

pub mod apply;
pub mod assess;
pub mod converge;
pub mod resolve;
pub mod source_reading;

pub use {
    apply::{ApplyOutcome, Enactment},
    assess::{SourceReadings, assess},
    converge::install_release,
    resolve::resolve,
    source_reading::{ReadSource, SourceReading, UnreadableReason},
};

/// What a resource kind answers when asked to compare its desired state against the machine.
/// There is deliberately no state type shared between kinds — a universal one would be the
/// lowest-common-denominator stringly type the newtype rule exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    Converged,
    Drifted(DriftReason),
    Unassessable(Impediment),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Impediment {
    // ADR 0004
    Absent(Requirement),
    ActualStateUnreadable(UnreadableReason),
}

impl Display for Impediment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Impediment::Absent(requirement) => Display::fmt(requirement, formatter),
            Impediment::ActualStateUnreadable(reason) => Display::fmt(reason, formatter),
        }
    }
}

/// Why a resource is not in its desired state, phrased for the person reading a change set.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DriftReason(String);

impl From<String> for DriftReason {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for DriftReason {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl Display for DriftReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

// ADR 0025
pub(crate) fn search_path_directory(
    entry: &crate::configuration::SearchPathEntry,
    resource: &ResolvedResource,
    machine: &impl crate::machine::ReadMachine,
) -> std::path::PathBuf {
    match &entry.directory {
        crate::configuration::SearchPathDirectory::ToolBinaries => machine.binaries_directory(),
        crate::configuration::SearchPathDirectory::Repository { repository, path } => {
            resource.clone_directory(repository).join(path)
        }
        crate::configuration::SearchPathDirectory::Home { path } => {
            machine.resolve_against_home(path)
        }
    }
}

pub(crate) fn symlink_location(
    symlink: &Symlink,
    resource: &ResolvedResource,
    machine: &impl crate::machine::ReadMachine,
) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        machine.resolve_against_home(&symlink.link_path),
        resource.files_root().join(&symlink.source_path),
    )
}

/// One resource that has drifted, together with why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub resource: ResolvedResource,
    pub reason: DriftReason,
}

/// One resource that could not be read, together with what stopped it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
    pub resource: ResolvedResource,
    pub impediment: Impediment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    pub changes: Vec<Change>,
    pub blocked: Vec<Blocked>,
    pub converged: Vec<ResolvedResource>,
    pub notices: Vec<Notice>,
    /// The documents an apply would rewrite, which a plan reports and performs none of.
    pub migrations: Vec<Migration>,
}

impl ChangeSet {
    /// A machine with no drift and nothing left unreadable.
    pub fn is_converged(&self) -> bool {
        self.changes.is_empty() && self.blocked.is_empty()
    }

    // ADR 0013
    pub fn would_enact_something(&self) -> bool {
        !self.changes.is_empty() || !self.migrations.is_empty()
    }
}

/// Compares every declared resource against the machine and orders the result. Ordering is by
/// kind first — which ADR 0004 makes a safety property — then by the order resources were
/// declared, so the same configuration against the same machine always prints the same change set
/// and two runs can be diffed.
///
/// Every source that can answer about a whole set of resources is read before any resource is
/// assessed, so one is read once per change set rather than once per resource. See ADR 0010.
pub async fn plan(
    desired_state: &DesiredState,
    machine: &impl crate::machine::ReadMachine,
    report: &RunReport,
) -> anyhow::Result<(ChangeSet, SourceReadings)> {
    let readings = {
        let _doing = report.doing("reading what the machine already has");
        SourceReadings::read_for(desired_state, machine).await
    };
    let resources = resolve(desired_state, &readings, machine.home_directory())?;

    let mut assessed: Vec<(ResourceKind, usize, ResolvedResource, Assessment)> = resources
        .iter()
        .enumerate()
        .map(|(position, resource)| {
            let _doing = report.doing(format!("reading {resource}"));
            (
                resource.kind(),
                position,
                resource.clone(),
                assess(resource, machine, &readings),
            )
        })
        .collect();

    assessed.sort_by_key(|(kind, position, _, _)| (*kind, *position));

    let mut changes = Vec::new();
    let mut blocked = Vec::new();
    let mut converged = Vec::new();
    for (_, _, resource, assessment) in assessed {
        match assessment {
            Assessment::Converged => converged.push(resource),
            Assessment::Drifted(reason) => changes.push(Change { resource, reason }),
            Assessment::Unassessable(impediment) => blocked.push(Blocked {
                resource,
                impediment,
            }),
        }
    }

    let mut notices: Vec<Notice> = desired_state
        .notices
        .iter()
        .map(|notice| Notice::Declared(notice.declared().clone()))
        .collect();
    notices.extend(desired_state.announcements.iter().cloned());
    notices.extend(
        machine
            .superseded_images()
            .into_iter()
            .map(Notice::SupersededImage),
    );

    Ok((
        ChangeSet {
            changes,
            blocked,
            converged,
            notices,
            migrations: desired_state.migrations.clone(),
        },
        readings,
    ))
}

impl Display for ChangeSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for change in &self.changes {
            writeln!(
                formatter,
                "  change  {} ({})",
                change.resource, change.reason
            )?;
        }
        for blocked in &self.blocked {
            writeln!(
                formatter,
                "  blocked {} ({})",
                blocked.resource, blocked.impediment
            )?;
        }
        for migration in &self.migrations {
            writeln!(formatter, "  migrate {migration}")?;
        }
        for notice in &self.notices {
            writeln!(formatter, "  notice  {notice}")?;
        }
        write!(
            formatter,
            "\n{} to change, {} blocked, {} already converged, {} to migrate",
            self.changes.len(),
            self.blocked.len(),
            self.converged.len(),
            self.migrations.len()
        )
    }
}

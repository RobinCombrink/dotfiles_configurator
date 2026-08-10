use {
    crate::{
        configuration::{GitHubRepository, Migration, Notice, Resource, ResourceKind, Shell},
        desired_state::{DesiredState, ResolvedResource},
        machine::Tool,
        reporting::RunReport,
    },
    std::fmt::Display,
};

pub mod apply;
pub mod assess;
pub mod converge;
pub mod resolve;

pub use {
    apply::ApplyOutcome,
    assess::{SourceReadings, assess},
    resolve::resolve,
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
    ActualStateUnreadable(DriftReason),
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

// ADR 0004
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Requirement {
    Tool(Tool),
    DotfilesRepository(GitHubRepository),
}

impl Display for Requirement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Requirement::Tool(tool) => write!(formatter, "{tool} is not on the path"),
            Requirement::DotfilesRepository(repository) => {
                write!(formatter, "{repository} has not been cloned")
            }
        }
    }
}

impl Resource {
    /// Whether converging this resource can be confirmed afterwards by reading the machine. Only
    /// a command without a presence check cannot: it claims no fact, so it has drift on every
    /// run by design, and its drift after an apply says nothing about whether the apply worked.
    pub fn can_be_read_back(&self) -> bool {
        match self {
            Resource::Command(command) => command.presence_check.is_some(),
            Resource::Repository(_)
            | Resource::Application(_)
            | Resource::Package(_)
            | Resource::EnvironmentVariable(_)
            | Resource::Symlink(_)
            | Resource::Registration(_) => true,
        }
    }

    /// What this resource needs on the machine before it can be read or converged. A property of
    /// the kind, not something an author writes down — no configuration can express a cargo
    /// package that forgets it needs cargo. What it needs of its own configuration is answered by
    /// the resolved resource instead, which is the only thing holding the origin that decides it.
    pub(crate) fn tool_requirements(&self) -> Vec<Requirement> {
        match self {
            Resource::Repository(_) => Vec::new(),
            Resource::Application(crate::configuration::Application::Installer(installer)) => {
                check_requirements(Some(&installer.presence_check))
            }
            Resource::Application(crate::configuration::Application::ReleasedBinary(_)) => {
                Vec::new()
            }
            Resource::Package(crate::configuration::Package::Winget(_)) => {
                vec![Requirement::Tool(Tool::Winget)]
            }
            Resource::Package(crate::configuration::Package::Cargo(package)) => {
                match package.source {
                    crate::configuration::CargoSource::Workspace { .. } => {
                        vec![Requirement::Tool(Tool::Cargo), Requirement::Tool(Tool::Git)]
                    }
                    crate::configuration::CargoSource::Registry
                    | crate::configuration::CargoSource::Path { .. } => {
                        vec![Requirement::Tool(Tool::Cargo)]
                    }
                }
            }
            Resource::EnvironmentVariable(_) | Resource::Symlink(_) => Vec::new(),
            Resource::Registration(_) => vec![Requirement::Tool(Tool::Claude)],
            Resource::Command(command) => {
                let mut requirements = check_requirements(command.presence_check.as_ref());
                requirements.extend(shell_requirement(command.shell));
                requirements
            }
        }
    }
}

fn check_requirements(check: Option<&crate::configuration::PresenceCheck>) -> Vec<Requirement> {
    match check {
        Some(crate::configuration::PresenceCheck::CommandOutputContains { shell, .. }) => {
            shell_requirement(*shell).into_iter().collect()
        }
        Some(_) | None => Vec::new(),
    }
}

/// Only WSL is a tool in its own right; the other shells ship with the machines that have them.
fn shell_requirement(shell: Shell) -> Option<Requirement> {
    match shell {
        Shell::Wsl => Some(Requirement::Tool(Tool::Wsl)),
        Shell::Bash | Shell::CommandPrompt | Shell::PowerShell => None,
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

/// The ordered set of changes that would close every drift, inspectable without being enacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    pub changes: Vec<Change>,
    pub blocked: Vec<Blocked>,
    pub converged: Vec<ResolvedResource>,
    pub notices: Vec<Notice>,
    /// The documents an apply would rewrite, which a plan reports and performs none of.
    pub migrations: Vec<Migration>,
    pub readings: SourceReadings,
}

impl ChangeSet {
    /// A machine with no drift and nothing left unreadable.
    pub fn is_converged(&self) -> bool {
        self.changes.is_empty() && self.blocked.is_empty()
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
) -> anyhow::Result<ChangeSet> {
    let readings = {
        let _doing = report.doing("reading what the machine already has");
        SourceReadings::read_for(desired_state, machine).await
    };
    let resources = resolve(desired_state, &readings)?;

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
        .map(|notice| notice.declared().clone())
        .collect();
    notices.extend(desired_state.announcements.iter().cloned());
    notices.extend(machine.superseded_images().iter().map(|path| {
        Notice::from(format!(
            "{} is a binary that was replaced while it was being executed; an apply removes it \
             once nothing is running it",
            path.display()
        ))
    }));

    Ok(ChangeSet {
        changes,
        blocked,
        converged,
        notices,
        migrations: desired_state.migrations.clone(),
        readings,
    })
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

use {
    crate::{
        configuration::{DesiredState, Notice, Resource, ResourceKind, Shell},
        machine::Tool,
    },
    std::fmt::Display,
};

pub mod apply;
pub mod assess;
pub mod converge;

pub use {apply::ApplyOutcome, assess::assess};

/// What a resource kind answers when asked to compare its desired state against the machine.
/// There is deliberately no state type shared between kinds — a universal one would be the
/// lowest-common-denominator stringly type the newtype rule exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    Converged,
    Drifted(DriftReason),
    /// The resource's requirements are absent, so it can neither be read nor converged yet. It is
    /// not failed; it says so and may become ready once something else has been applied.
    Unassessable(Requirement),
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

/// Something that has to be true of a machine before a resource can be read or converged. Read
/// from the machine each time rather than inferred from order, so a resource blocked now may
/// become ready once something else has been applied. See ADR 0004.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Requirement {
    Tool(Tool),
    /// Symlinks resolve against the dotfiles repository, so nothing can link out of it until it
    /// has been cloned.
    DotfilesRepository,
}

impl Display for Requirement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Requirement::Tool(tool) => write!(formatter, "{tool} is not on the path"),
            Requirement::DotfilesRepository => {
                formatter.write_str("the dotfiles repository has not been cloned")
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
            | Resource::Symlink(_)
            | Resource::Registration(_) => true,
        }
    }

    /// What this resource needs before it can be read or converged. A property of the kind, not
    /// something an author writes down — no configuration can express a cargo package that
    /// forgets it needs cargo.
    pub fn requirements(&self) -> Vec<Requirement> {
        let mut requirements = match self {
            Resource::Repository(_) => Vec::new(),
            Resource::Application(application) => {
                check_requirements(Some(&application.presence_check))
            }
            Resource::Package(crate::configuration::Package::Winget(_)) => {
                vec![Requirement::Tool(Tool::Winget)]
            }
            Resource::Package(crate::configuration::Package::Cargo(_)) => {
                vec![Requirement::Tool(Tool::Cargo)]
            }
            Resource::Symlink(_) => vec![Requirement::DotfilesRepository],
            Resource::Registration(_) => vec![Requirement::Tool(Tool::Claude)],
            Resource::Command(command) => {
                let mut requirements = check_requirements(command.presence_check.as_ref());
                requirements.extend(shell_requirement(command.shell));
                requirements
            }
        };
        requirements.sort();
        requirements.dedup();
        requirements
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

/// One resource that has drifted, together with why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub resource: Resource,
    pub reason: DriftReason,
}

/// One resource that could not be read, together with what is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
    pub resource: Resource,
    pub requirement: Requirement,
}

/// The ordered set of changes that would close every drift, inspectable without being enacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    pub changes: Vec<Change>,
    pub blocked: Vec<Blocked>,
    pub converged: Vec<Resource>,
    pub notices: Vec<Notice>,
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
pub fn plan(desired_state: &DesiredState, machine: &impl crate::machine::ReadMachine) -> ChangeSet {
    let mut assessed: Vec<(ResourceKind, usize, Resource, Assessment)> = desired_state
        .resources
        .iter()
        .enumerate()
        .map(|(position, resource)| {
            (
                resource.kind(),
                position,
                resource.clone(),
                assess(resource, machine),
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
            Assessment::Unassessable(requirement) => blocked.push(Blocked {
                resource,
                requirement,
            }),
        }
    }

    ChangeSet {
        changes,
        blocked,
        converged,
        notices: desired_state.notices.clone(),
    }
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
                blocked.resource, blocked.requirement
            )?;
        }
        for notice in &self.notices {
            writeln!(formatter, "  notice  {notice}")?;
        }
        write!(
            formatter,
            "\n{} to change, {} blocked, {} already converged",
            self.changes.len(),
            self.blocked.len(),
            self.converged.len()
        )
    }
}

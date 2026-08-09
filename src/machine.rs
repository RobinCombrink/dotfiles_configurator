use {
    crate::{
        configuration::{CrateName, PresenceCheck, Shell},
        machine::workspace_reading::{Revision, WorkspaceReading},
    },
    anyhow::Result,
    std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    },
};

pub mod invocation;
pub mod local;
pub mod workspace_reading;

pub use invocation::{ReadInvocation, WriteInvocation};

/// A program a resource kind needs in order to read or converge a resource. A tool's presence is
/// probed on the machine, never declared, so a tool installed by hand counts exactly as much as
/// one this tool installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tool {
    Winget,
    Cargo,
    Claude,
    Wsl,
}

impl Tool {
    pub fn program(&self) -> &'static str {
        match self {
            Tool::Winget => "winget",
            Tool::Cargo => "cargo",
            Tool::Claude => "claude",
            Tool::Wsl => "wsl",
        }
    }
}

impl std::fmt::Display for Tool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.program())
    }
}

/// What a process reported back. Kept whole rather than reduced to a bool, because parsing a
/// tool's output belongs to the kind that understands it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub succeeded: bool,
    pub standard_output: String,
    pub standard_error: String,
}

#[derive(Debug)]
pub enum Placement {
    Placed(CommandOutput),
    Held(PathBuf),
}

/// The capabilities that can only read a machine. Plan holds exactly these, which is what makes
/// "plan had no side effects" a property the compiler checks rather than a convention. See ADR
/// 0006.
pub trait ReadMachine {
    fn home_directory(&self) -> &Path;

    fn repositories_directory(&self) -> &Path;

    /// The directory the dotfiles repository is cloned into. Symlinks resolve their source
    /// against it, which is what makes cloning it a requirement of every symlink.
    fn dotfiles_repository_path(&self) -> &Path;

    fn path_exists(&self, path: &Path) -> bool;

    /// The target of the link at `path`, or `None` when nothing is there or it is not a link.
    fn link_target(&self, path: &Path) -> Option<PathBuf>;

    fn tool_is_present(&self, tool: Tool) -> bool;

    /// Runs one of the invocations this crate defines for reading state. The set is closed, so
    /// plan cannot be handed the ability to run an installer.
    fn read(&self, invocation: &ReadInvocation) -> Result<CommandOutput>;

    /// Runs a check the configuration's author wrote and declared as a check. Two of the three
    /// forms cannot change anything by construction; the third is the deliberate escape hatch.
    fn read_cargo_workspace(
        &self,
        repository_path: &Path,
        installed: &BTreeMap<CrateName, Revision>,
    ) -> Result<Option<WorkspaceReading>>;

    fn check_presence(&self, check: &PresenceCheck) -> Result<bool>;

    /// Resolves a path declared relative to the home directory. Absolute paths are left alone.
    fn resolve_against_home(&self, path: &Path) -> PathBuf {
        match path.is_absolute() {
            true => path.to_path_buf(),
            false => self.home_directory().join(path),
        }
    }
}

/// The capabilities that can change a machine. Apply holds these as well as the reading ones;
/// plan holds none of them.
pub trait WriteMachine: ReadMachine {
    fn create_link(&self, link_path: &Path, target_path: &Path) -> Result<()>;

    fn clone_repository(
        &self,
        repository: &crate::configuration::GitHubRepository,
    ) -> impl std::future::Future<Output = Result<()>>;

    fn install_application(
        &self,
        application: &crate::configuration::Application,
    ) -> impl std::future::Future<Output = Result<()>>;

    /// Runs one of the invocations this crate defines for changing state.
    fn write(&self, invocation: &WriteInvocation) -> Result<CommandOutput>;

    fn write_displacing(&self, invocation: &WriteInvocation) -> Result<Placement>;

    /// Runs a command the configuration declared, which is the escape hatch's whole point.
    fn run_declared_command(&self, shell: Shell, args: &[String]) -> Result<CommandOutput>;
}

use {
    crate::{
        TOOL_DIRECTORY,
        configuration::{
            CrateName, GitHubAccount, GitHubRepository, McpServerName, PresenceCheck, Shell, Tool,
            VariableName, VariableValue, path_folding,
        },
        machine::{
            environment_reading::SearchPathReading,
            release_reading::ReleaseReading,
            workspace_reading::{Revision, WorkspaceReading},
        },
    },
    anyhow::Result,
    std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    },
};

pub mod environment_reading;
pub mod invocation;
pub mod local;
pub mod release_reading;
pub mod workspace_reading;

pub use invocation::{
    DisplacingInvocation, ReadInvocation, RefusedCopy, ReplacementCommands, ReplacingInvocation,
    ResolvedCargoSource, WriteInvocation,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub succeeded: bool,
    pub standard_output: String,
    pub standard_error: String,
}

pub const SUPERSEDED_SUFFIX: &str = ".superseded";

pub fn superseded_name(destination: &Path) -> PathBuf {
    let mut name = destination.file_name().unwrap_or_default().to_os_string();
    name.push(SUPERSEDED_SUFFIX);

    destination.with_file_name(name)
}

pub const PARTIAL_DOWNLOAD_SUFFIX: &str = ".partial";

pub fn partial_download_path(destination: &Path) -> PathBuf {
    let mut name = destination.file_name().unwrap_or_default().to_os_string();
    name.push(PARTIAL_DOWNLOAD_SUFFIX);

    destination.with_file_name(name)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Placement {
    Placed,
    Held(PathBuf),
}

#[derive(Debug)]
pub enum Replacement {
    Replaced,
    RemovedButCouldNotAdd {
        name: McpServerName,
        cause: anyhow::Error,
    },
}

// ADR 0006
pub trait ReadMachine {
    fn home_directory(&self) -> &Path;

    fn superseded_images(&self) -> Vec<PathBuf>;

    fn path_exists(&self, path: &Path) -> bool;

    fn link_target(&self, path: &Path) -> Option<PathBuf>;

    fn canonical_path(&self, path: &Path) -> Option<PathBuf>;

    fn tool_is_present(&self, tool: Tool) -> bool;

    /// The text held at `path`, where `Ok(None)` means nothing is there and an error means
    /// something is but could not be read as text.
    ///
    /// ```no_run
    /// # use {dotfiles_configurator::machine::ReadMachine, std::path::Path};
    /// # fn held_by(machine: &impl ReadMachine, path: &Path) -> String {
    /// match machine.text_file_at(path) {
    ///     Ok(Some(text)) => text,
    ///     Ok(None) => "the machine holds no such file".to_owned(),
    ///     Err(error) => format!("{error:#}"),
    /// }
    /// # }
    /// ```
    fn text_file_at(&self, path: &Path) -> Result<Option<String>>;

    fn read(&self, invocation: &ReadInvocation) -> Result<CommandOutput>;

    fn read_cargo_workspace(
        &self,
        repository_path: &Path,
        installed: &BTreeMap<CrateName, Revision>,
    ) -> Result<Option<WorkspaceReading>>;

    fn check_presence(&self, check: &PresenceCheck) -> Result<bool>;

    /// The latest release a repository has published, or `None` where it has published none at
    /// all. A repository that could not be asked is an error rather than an empty answer.
    ///
    /// ```no_run
    /// # use dotfiles_configurator::{
    /// #     configuration::{GitHubAccount, GitHubRepository},
    /// #     machine::ReadMachine,
    /// # };
    /// # async fn has_published(
    /// #     machine: &impl ReadMachine,
    /// #     repository: &GitHubRepository,
    /// #     account: &GitHubAccount,
    /// # ) -> anyhow::Result<bool> {
    /// Ok(machine.latest_release(repository, account).await?.is_some())
    /// # }
    /// ```
    // ADR 0010
    fn latest_release(
        &self,
        repository: &GitHubRepository,
        account: &GitHubAccount,
    ) -> impl std::future::Future<Output = Result<Option<ReleaseReading>>>;

    // ADR 0016
    fn report_version(&self, binary_path: &Path, arguments: &[String]) -> Result<CommandOutput>;

    // ADR 0017
    fn read_search_path(&self) -> Result<SearchPathReading>;

    // ADR 0017
    fn read_environment_variable(&self, name: &VariableName) -> Result<Option<VariableValue>>;

    fn resolve_against_home(&self, path: &Path) -> PathBuf {
        path_folding::home_relative_path(self.home_directory(), path)
    }

    // ADR 0015
    fn binaries_directory(&self) -> PathBuf {
        self.home_directory().join(TOOL_DIRECTORY).join("bin")
    }

    fn build_cache_directory(&self) -> PathBuf {
        self.home_directory()
            .join(TOOL_DIRECTORY)
            .join("build-cache")
    }

    // ADR 0022
    fn displacement_directories(&self) -> Vec<PathBuf>;
}

pub trait WriteMachine: ReadMachine {
    fn create_link(&self, link_path: &Path, target_path: &Path) -> Result<()>;

    fn write_text_file(&self, path: &Path, contents: &str) -> Result<()>;

    fn clone_repository(
        &self,
        repository: &crate::configuration::GitHubRepository,
        clone_directory: &Path,
        account: &GitHubAccount,
    ) -> impl std::future::Future<Output = Result<()>>;

    fn install_application(
        &self,
        installer: &crate::configuration::Installer,
        release_asset: Option<&release_reading::ReleaseAsset>,
    ) -> impl std::future::Future<Output = Result<()>>;

    // ADR 0016
    fn install_released_binary(
        &self,
        binary: &crate::configuration::ReleasedBinary,
        asset: &release_reading::ReleaseAsset,
    ) -> impl std::future::Future<Output = Result<Placement>>;

    /// Makes the search path carry a directory. The postcondition is membership, so calling it for
    /// a directory the path already carries changes nothing — which is what keeps two resources
    /// resolving to one directory from adding it twice within a single pass.
    ///
    /// ```no_run
    /// # use dotfiles_configurator::machine::WriteMachine;
    /// # use std::path::Path;
    /// # fn put(machine: &impl WriteMachine) -> anyhow::Result<()> {
    /// machine.put_on_search_path(Path::new("C:\\tools\\bin"))
    /// # }
    /// ```
    fn put_on_search_path(&self, directory: &Path) -> Result<()>;

    // ADR 0017
    fn set_environment_variable(&self, name: &VariableName, value: &VariableValue) -> Result<()>;

    fn write(&self, invocation: &WriteInvocation) -> Result<CommandOutput>;

    fn write_displacing(&self, invocation: &DisplacingInvocation) -> Result<Placement>;

    // ADR 0033
    fn write_over_running_images(&self, invocation: &WriteInvocation) -> Result<Placement>;

    /// Runs both commands of one replacement, in the order the invocation gives them. A refusal
    /// of the second after the first has taken the name away is a `Replacement` rather than an
    /// error, because the machine has been changed and the caller has to say so.
    ///
    /// ```no_run
    /// # use dotfiles_configurator::machine::{Replacement, ReplacingInvocation, WriteMachine};
    /// # fn register(
    /// #     machine: &impl WriteMachine,
    /// #     invocation: &ReplacingInvocation,
    /// # ) -> anyhow::Result<()> {
    /// match machine.replace(invocation)? {
    ///     Replacement::Replaced => Ok(()),
    ///     Replacement::RemovedButCouldNotAdd { name, cause } => {
    ///         Err(cause.context(format!("nothing holds the name {name} now")))
    ///     }
    /// }
    /// # }
    /// ```
    fn replace(&self, invocation: &ReplacingInvocation) -> Result<Replacement>;

    fn sweep_superseded_images(&self);

    fn run_declared_command(&self, shell: Shell, args: &[String]) -> Result<CommandOutput>;
}

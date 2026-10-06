use {
    crate::{
        TOOL_DIRECTORY,
        configuration::{
            Answer, CrateName, CrateVersion, DartPackage, GitHubAccount, GitHubRepository,
            McpServerName, PresenceCheck, Shell, Tool, VariableName, VariableValue, path_folding,
        },
        machine::{
            dart_reading::{DartLocations, DartReading},
            environment_reading::SearchPathReading,
            release_reading::ReleaseReading,
            workspace_reading::{Fingerprint, Revision, WorkspaceReading},
        },
    },
    anyhow::Result,
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt::Display,
        path::{Path, PathBuf},
        process::ExitStatus,
    },
};

pub mod crate_index_reading;
pub mod dart_reading;
pub mod elevation;
pub mod environment_reading;
pub mod invocation;
pub mod local;
pub mod release_reading;
pub mod workspace_reading;

pub use {
    elevation::{
        Batched, ContentDigest, ElevatedBatch, ElevatedOutcome, ElevatedWork, Elevation,
        PrivilegeRefusal,
    },
    invocation::{
        DisplacingInvocation, ReadInvocation, RefusedCopy, ReplacementCommands,
        ReplacingInvocation, ResolvedCargoSource, WorkspaceBuild, WriteInvocation,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exited {
    Code(i32),
    WithoutCode,
}

impl Exited {
    pub fn succeeded(self) -> bool {
        match self {
            Exited::Code(code) => code == 0,
            Exited::WithoutCode => false,
        }
    }
}

impl From<ExitStatus> for Exited {
    fn from(status: ExitStatus) -> Self {
        match status.code() {
            Some(code) => Exited::Code(code),
            None => Exited::WithoutCode,
        }
    }
}

impl Display for Exited {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Exited::Code(code) if code.is_negative() => {
                write!(formatter, "exited with {:#X}", code.cast_unsigned())
            }
            Exited::Code(code) => write!(formatter, "exited with {code}"),
            Exited::WithoutCode => formatter.write_str("exited without a code"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exited: Exited,
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

#[derive(Debug)]
pub struct Downloaded<Declared> {
    declared: Declared,
    file: PathBuf,
}

impl<Declared> Downloaded<Declared> {
    pub fn fetched(declared: Declared, file: PathBuf) -> Self {
        Self { declared, file }
    }

    pub fn declared(&self) -> &Declared {
        &self.declared
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Placement {
    Placed,
    Held(HeldReason),
    Refused(PrivilegeRefusal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldReason {
    BeingExecuted(PathBuf),
    ElevationDeclined,
    ReportedInUse(String),
    UpgradedByItsPublisher,
}

impl Display for HeldReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeldReason::BeingExecuted(path) => {
                write!(formatter, "{} is being executed", path.display())
            }
            HeldReason::ElevationDeclined => formatter.write_str("elevation declined"),
            HeldReason::ReportedInUse(reported) => write!(formatter, "in use: {reported}"),
            HeldReason::UpgradedByItsPublisher => {
                formatter.write_str("its publisher upgrades it rather than winget")
            }
        }
    }
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

    fn is_elevated(&self) -> bool;

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

    fn read_cargo_workspace(&self, repository_path: &Path) -> Result<Option<WorkspaceReading>>;

    // ADR 0040
    fn read_cargo_workspace_at(
        &self,
        repository_path: &Path,
        revision: &Revision,
    ) -> Result<BTreeMap<CrateName, Fingerprint>>;

    fn cargo_binaries_directory(&self) -> PathBuf;

    fn check_presence(&self, check: &PresenceCheck) -> Result<Option<Answer>>;

    fn clone_is_shallow(&self, clone_directory: &Path) -> Result<bool>;

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

    fn newest_published_crate(
        &self,
        crate_name: &CrateName,
    ) -> impl std::future::Future<Output = Result<CrateVersion>>;

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

    fn dart_locations(&self) -> DartLocations {
        let local_application_data = self.home_directory().join("AppData").join("Local");
        DartLocations {
            install_directory: local_application_data.join("Dart").join("install"),
            pub_cache_directory: local_application_data.join("Pub").join("Cache"),
        }
    }

    fn read_dart_package(&self, package: &DartPackage) -> Result<DartReading>;

    // ADR 0022
    fn displacement_directories(&self) -> Vec<PathBuf>;
}

pub trait WriteMachine: ReadMachine {
    fn create_link(&self, link_path: &Path, target_path: &Path) -> Result<Placement>;

    fn write_text_file(&self, path: &Path, contents: &str) -> Result<()>;

    fn clone_repository(
        &self,
        clone: &crate::configuration::RepositoryClone,
        clone_directory: &Path,
        account: &GitHubAccount,
    ) -> impl std::future::Future<Output = Result<()>>;

    // ADR 0038
    fn deepen_clone(
        &self,
        repository: &GitHubRepository,
        clone_directory: &Path,
        account: &GitHubAccount,
    ) -> impl std::future::Future<Output = Result<()>>;

    fn download_installer(
        &self,
        installer: &crate::configuration::Installer,
        release_asset: Option<&release_reading::ReleaseAsset>,
    ) -> impl std::future::Future<Output = Result<Downloaded<crate::configuration::Installer>>>;

    fn install_application(
        &self,
        downloaded: Downloaded<crate::configuration::Installer>,
    ) -> impl std::future::Future<Output = Result<Placement>>;

    // ADR 0042
    fn run_elevated(
        &self,
        batch: &ElevatedBatch<()>,
    ) -> impl std::future::Future<Output = Result<Elevation>>;

    // ADR 0016
    fn download_released_binary(
        &self,
        binary: &crate::configuration::ReleasedBinary,
        asset: &release_reading::ReleaseAsset,
    ) -> impl std::future::Future<Output = Result<Downloaded<crate::configuration::ReleasedBinary>>>;

    // ADR 0016
    fn install_released_binary(
        &self,
        downloaded: Downloaded<crate::configuration::ReleasedBinary>,
    ) -> Result<Placement>;

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

    fn write(
        &self,
        invocation: &WriteInvocation,
    ) -> impl std::future::Future<Output = Result<CommandOutput>>;

    // ADR 0041
    fn attempt_write(
        &self,
        invocation: &WriteInvocation,
    ) -> impl std::future::Future<Output = Result<CommandOutput>>;

    fn write_displacing(
        &self,
        invocation: &DisplacingInvocation,
    ) -> impl std::future::Future<Output = Result<Placement>>;

    fn reap_builds_of_other_revisions(&self, building: &BTreeSet<Revision>);

    fn build_workspace_members(
        &self,
        build: &WorkspaceBuild<'_>,
    ) -> impl std::future::Future<Output = Result<()>>;

    // ADR 0033
    fn write_over_running_images(
        &self,
        invocation: &WriteInvocation,
    ) -> impl std::future::Future<Output = Result<Placement>>;

    /// Runs both commands of one replacement, in the order the invocation gives them. A refusal
    /// of the second after the first has taken the name away is a `Replacement` rather than an
    /// error, because the machine has been changed and the caller has to say so.
    ///
    /// ```no_run
    /// # use dotfiles_configurator::machine::{Replacement, ReplacingInvocation, WriteMachine};
    /// # async fn register(
    /// #     machine: &impl WriteMachine,
    /// #     invocation: &ReplacingInvocation,
    /// # ) -> anyhow::Result<()> {
    /// match machine.replace(invocation).await? {
    ///     Replacement::Replaced => Ok(()),
    ///     Replacement::RemovedButCouldNotAdd { name, cause } => {
    ///         Err(cause.context(format!("nothing holds the name {name} now")))
    ///     }
    /// }
    /// # }
    /// ```
    fn replace(
        &self,
        invocation: &ReplacingInvocation,
    ) -> impl std::future::Future<Output = Result<Replacement>>;

    fn sweep_superseded_images(&self);

    fn run_declared_command(
        &self,
        shell: Shell,
        args: &[String],
    ) -> impl std::future::Future<Output = Result<CommandOutput>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_exiting_zero_succeeded() {
        assert!(Exited::Code(0).succeeded());
    }

    #[test]
    fn a_child_exiting_with_any_other_code_did_not_succeed() {
        assert!(!Exited::Code(1).succeeded());
    }

    #[test]
    fn a_child_ending_without_a_code_did_not_succeed() {
        assert!(!Exited::WithoutCode.succeeded());
    }

    #[test]
    fn a_code_without_its_high_bit_set_is_written_in_decimal() {
        assert_eq!(Exited::Code(1603).to_string(), "exited with 1603");
    }

    #[test]
    fn a_code_with_its_high_bit_set_is_written_in_hex_as_windows_documents_it() {
        assert_eq!(
            Exited::Code(0x8A15_0014_u32.cast_signed()).to_string(),
            "exited with 0x8A150014"
        );
    }

    #[test]
    fn a_child_ending_without_a_code_is_written_without_one() {
        assert_eq!(Exited::WithoutCode.to_string(), "exited without a code");
    }
}

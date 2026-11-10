use {
    crate::{
        configuration::{
            CrateName, GitHubRepository, Tool, UnreportedVersion, UvToolName, VersionInvocation,
            WingetPackageId,
        },
        convergence::Impediment,
        machine::{
            FailedExit, FileReadFailure, ProgramStartFailure, dart_reading::DartReadFailure,
            environment_reading::RegistryReadFailure, workspace_reading::WorkspaceReadFailure,
        },
    },
    std::{fmt::Display, path::PathBuf},
};

#[derive(Debug, Clone)]
pub enum ReadFailure {
    Chain(String),
    Workspace(WorkspaceReadFailure),
    Dart(DartReadFailure),
    Registry(RegistryReadFailure),
    ProgramStart(ProgramStartFailure),
    FileRead(FileReadFailure),
}

impl From<&anyhow::Error> for ReadFailure {
    fn from(error: &anyhow::Error) -> Self {
        Self::Chain(format!("{error:#}"))
    }
}

impl From<WorkspaceReadFailure> for ReadFailure {
    fn from(failure: WorkspaceReadFailure) -> Self {
        Self::Workspace(failure)
    }
}

impl From<DartReadFailure> for ReadFailure {
    fn from(failure: DartReadFailure) -> Self {
        Self::Dart(failure)
    }
}

impl From<RegistryReadFailure> for ReadFailure {
    fn from(failure: RegistryReadFailure) -> Self {
        Self::Registry(failure)
    }
}

impl From<ProgramStartFailure> for ReadFailure {
    fn from(failure: ProgramStartFailure) -> Self {
        Self::ProgramStart(failure)
    }
}

impl From<FileReadFailure> for ReadFailure {
    fn from(failure: FileReadFailure) -> Self {
        Self::FileRead(failure)
    }
}

impl Display for ReadFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadFailure::Chain(chain) => formatter.write_str(chain),
            ReadFailure::Workspace(failure) => Display::fmt(failure, formatter),
            ReadFailure::Dart(failure) => Display::fmt(failure, formatter),
            ReadFailure::Registry(failure) => Display::fmt(failure, formatter),
            ReadFailure::ProgramStart(failure) => Display::fmt(failure, formatter),
            ReadFailure::FileRead(failure) => Display::fmt(failure, formatter),
        }
    }
}

#[derive(Debug, Clone)]
pub enum UnreadableReason {
    ReadFailed(ReadFailure),
    NotRead(ReadSource),
    ToolFailed {
        tool: Tool,
        failure: FailedExit,
    },
    WingetPackageUnasked {
        id: WingetPackageId,
        failure: FailedExit,
    },
    WingetUpgradesUnlisted {
        failure: FailedExit,
        standard_output: String,
    },
    WingetListsNoIdColumn,
    WingetCutAnIdShort,
    WingetOffersNoVersion {
        id: WingetPackageId,
    },
    WingetListsNoOneVersion {
        listed: String,
    },
    UvListsNoInstalledVersion {
        name: UvToolName,
        line: String,
    },
    UvListsAnUnnamedNewerVersion {
        name: UvToolName,
        line: String,
    },
    UvNamesNoNewerVersion {
        name: UvToolName,
    },
    NoClone {
        clone_directory: PathBuf,
    },
    NoRelease {
        repository: GitHubRepository,
    },
    VersionRequestFailed {
        invocation: VersionInvocation,
        failure: FailedExit,
    },
    VersionUnreported(UnreportedVersion),
    CargoListsAnUnreadableVersion {
        listed: String,
    },
}

impl From<&anyhow::Error> for UnreadableReason {
    fn from(error: &anyhow::Error) -> Self {
        UnreadableReason::ReadFailed(ReadFailure::from(error))
    }
}

impl UnreadableReason {
    pub fn read_failed(failure: impl Into<ReadFailure>) -> Self {
        UnreadableReason::ReadFailed(failure.into())
    }
}

impl Display for UnreadableReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnreadableReason::ReadFailed(failure) => Display::fmt(failure, formatter),
            UnreadableReason::NotRead(source) => {
                write!(formatter, "{source} was not read for this change set")
            }
            UnreadableReason::ToolFailed { tool, failure } => {
                write!(formatter, "{tool} could not be read, {failure}")
            }
            UnreadableReason::WingetPackageUnasked { id, failure } => {
                write!(formatter, "winget could not be asked about {id}, {failure}")
            }
            UnreadableReason::WingetUpgradesUnlisted {
                failure,
                standard_output,
            } => write!(
                formatter,
                "winget could not list what it would upgrade, {}: {} {}",
                failure.exited,
                standard_output.trim(),
                failure.standard_error.trim()
            ),
            UnreadableReason::WingetListsNoIdColumn => {
                formatter.write_str("winget's listing has no Id column, so it could not be read")
            }
            UnreadableReason::WingetCutAnIdShort => {
                formatter.write_str("winget cut an Id short, so its listing could not be read")
            }
            UnreadableReason::WingetOffersNoVersion { id } => write!(
                formatter,
                "winget lists an upgrade of {id} without the version it offers"
            ),
            UnreadableReason::WingetListsNoOneVersion { listed } => write!(
                formatter,
                "winget reports no one version it is installed at, only {listed:?}"
            ),
            UnreadableReason::UvListsNoInstalledVersion { name, line } => write!(
                formatter,
                "uv listed {name} without the version it is installed at: {line}"
            ),
            UnreadableReason::UvListsAnUnnamedNewerVersion { name, line } => write!(
                formatter,
                "uv listed {name} as behind a version it did not name: {line}"
            ),
            UnreadableReason::UvNamesNoNewerVersion { name } => write!(
                formatter,
                "uv listed {name} as behind without naming the version it is behind"
            ),
            UnreadableReason::NoClone { clone_directory } => write!(
                formatter,
                "{} holds no clone, so its members cannot be read",
                clone_directory.display()
            ),
            UnreadableReason::NoRelease { repository } => {
                write!(formatter, "{repository} has published no release")
            }
            UnreadableReason::VersionRequestFailed {
                invocation,
                failure,
            } => write!(formatter, "`{invocation}` failed, {failure}"),
            UnreadableReason::VersionUnreported(unreported) => Display::fmt(unreported, formatter),
            UnreadableReason::CargoListsAnUnreadableVersion { listed } => write!(
                formatter,
                "cargo lists it at {listed:?}, a version this program cannot read"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadSource {
    Tool(Tool),
    SearchPath,
    CargoWorkspace(PathBuf),
    LatestRelease(GitHubRepository),
    CratesIndex(CrateName),
}

impl ReadSource {
    pub fn was_not_read(&self) -> Impediment {
        Impediment::ActualStateUnreadable(UnreadableReason::NotRead(self.clone()))
    }
}

impl Display for ReadSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadSource::Tool(tool) => Display::fmt(tool, formatter),
            ReadSource::SearchPath => formatter.write_str("the search path"),
            ReadSource::CargoWorkspace(clone_directory) => write!(
                formatter,
                "the cargo workspace at {}",
                clone_directory.display()
            ),
            ReadSource::LatestRelease(repository) => {
                write!(formatter, "the latest release of {repository}")
            }
            ReadSource::CratesIndex(crate_name) => {
                write!(formatter, "what crates.io lists for {crate_name}")
            }
        }
    }
}

/// The three states one source can be in for a change set, so that a reader cannot reach the
/// value without having answered for the two states where there is none. The impediment is the
/// only thing the non-read states yield, which is what keeps an unreadable actual state out of
/// [`crate::convergence::Assessment::Drifted`].
///
/// ```
/// # use dotfiles_configurator::convergence::{
/// #     Impediment,
/// #     source_reading::{SourceReading, UnreadableReason},
/// # };
/// let reading: SourceReading<String> =
///     SourceReading::Unreadable(UnreadableReason::WingetListsNoIdColumn);
///
/// let Err(impediment) = reading.read() else {
///     panic!("a reading that could not be read answered a value");
/// };
/// let Impediment::ActualStateUnreadable(UnreadableReason::WingetListsNoIdColumn) = impediment
/// else {
///     panic!("{impediment:?}");
/// };
/// ```
#[derive(Debug, Clone)]
pub enum SourceReading<T> {
    Read(T),
    Unreadable(UnreadableReason),
    NotRequested(ReadSource),
}

impl<T> SourceReading<T> {
    pub fn read(&self) -> Result<&T, Impediment> {
        match self {
            SourceReading::Read(value) => Ok(value),
            SourceReading::Unreadable(reason) => {
                Err(Impediment::ActualStateUnreadable(reason.clone()))
            }
            SourceReading::NotRequested(source) => Err(source.was_not_read()),
        }
    }
}

#[cfg(test)]
mod tests;

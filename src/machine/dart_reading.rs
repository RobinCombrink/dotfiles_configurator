use {
    crate::{
        configuration::{
            DartPackageName, GitCommit, GitReferenceName, GitRemoteUrl, RepositorySubdirectory,
        },
        machine::{FailedExit, FileReadFailure, ProgramStartFailure},
    },
    std::{
        error::Error,
        fmt::{Display, Formatter},
        path::PathBuf,
        sync::Arc,
    },
};

#[derive(Debug, Clone)]
pub enum DartReadFailure {
    FileRead(FileReadFailure),
    ProgramStart(ProgramStartFailure),
    PubspecUnparsed {
        pubspec: PathBuf,
        error: Arc<serde_saphyr::Error>,
    },
    LockUnparsed {
        lock: PathBuf,
        error: Arc<serde_saphyr::Error>,
    },
    LockHoldsNoEntry {
        lock: PathBuf,
        package: DartPackageName,
    },
    LockNamesAnUnreadableCommit {
        lock: PathBuf,
        listed: String,
    },
    SeveralMirrors {
        count: usize,
        url: GitRemoteUrl,
        mirrors: PathBuf,
    },
    OriginUnlisted {
        mirror: PathBuf,
        url: GitRemoteUrl,
        failure: FailedExit,
    },
    OriginUnfetched {
        mirror: PathBuf,
        url: GitRemoteUrl,
        failure: FailedExit,
    },
    OriginAnsweredAnUnreadableCommit {
        reference: GitReferenceName,
        listed: String,
    },
    OriginHoldsNoReference {
        url: GitRemoteUrl,
        reference: GitReferenceName,
    },
    TreeUnnamed {
        mirror: PathBuf,
        commit: GitCommit,
        path: RepositorySubdirectory,
        failure: FailedExit,
    },
}

impl From<FileReadFailure> for DartReadFailure {
    fn from(failure: FileReadFailure) -> Self {
        DartReadFailure::FileRead(failure)
    }
}

impl From<ProgramStartFailure> for DartReadFailure {
    fn from(failure: ProgramStartFailure) -> Self {
        DartReadFailure::ProgramStart(failure)
    }
}

const ROUTING_REMEDY: &str =
    "Check that the shared .gitconfig routes the pub cache to an account that can read";

impl Display for DartReadFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DartReadFailure::FileRead(failure) => Display::fmt(failure, formatter),
            DartReadFailure::ProgramStart(failure) => Display::fmt(failure, formatter),
            DartReadFailure::PubspecUnparsed { pubspec, error } => write!(
                formatter,
                "{} could not be read as a pubspec: {error}",
                pubspec.display()
            ),
            DartReadFailure::LockUnparsed { lock, error } => write!(
                formatter,
                "{} could not be read as a lock file: {error}",
                lock.display()
            ),
            DartReadFailure::LockHoldsNoEntry { lock, package } => {
                write!(formatter, "{} holds no entry for {package}", lock.display())
            }
            DartReadFailure::LockNamesAnUnreadableCommit { lock, listed } => write!(
                formatter,
                "{} names {listed:?}, which is not a full commit",
                lock.display()
            ),
            DartReadFailure::SeveralMirrors {
                count,
                url,
                mirrors,
            } => write!(
                formatter,
                "{count} copies of {url} sit under {}, so which one pub reads cannot be told",
                mirrors.display()
            ),
            DartReadFailure::OriginUnlisted {
                mirror,
                url,
                failure,
            } => write!(
                formatter,
                "git ls-remote in {} {failure}. {ROUTING_REMEDY} {url}",
                mirror.display()
            ),
            DartReadFailure::OriginUnfetched {
                mirror,
                url,
                failure,
            } => write!(
                formatter,
                "git fetch in {} {failure}. {ROUTING_REMEDY} {url}",
                mirror.display()
            ),
            DartReadFailure::OriginAnsweredAnUnreadableCommit { reference, listed } => write!(
                formatter,
                "origin answered for {reference} with {listed:?}, which is not a full commit"
            ),
            DartReadFailure::OriginHoldsNoReference { url, reference } => {
                write!(
                    formatter,
                    "origin {url} holds no reference named {reference}"
                )
            }
            DartReadFailure::TreeUnnamed {
                mirror,
                commit,
                path,
                failure,
            } => write!(
                formatter,
                "git could not name the tree of {path} at {commit} in {}, {failure}",
                mirror.display()
            ),
        }
    }
}

impl Error for DartReadFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            DartReadFailure::FileRead(failure) => failure.source(),
            DartReadFailure::ProgramStart(failure) => failure.source(),
            DartReadFailure::PubspecUnparsed { error, .. }
            | DartReadFailure::LockUnparsed { error, .. } => Some(error.as_ref()),
            DartReadFailure::LockHoldsNoEntry { .. }
            | DartReadFailure::LockNamesAnUnreadableCommit { .. }
            | DartReadFailure::SeveralMirrors { .. }
            | DartReadFailure::OriginUnlisted { .. }
            | DartReadFailure::OriginUnfetched { .. }
            | DartReadFailure::OriginAnsweredAnUnreadableCommit { .. }
            | DartReadFailure::OriginHoldsNoReference { .. }
            | DartReadFailure::TreeUnnamed { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartLocations {
    pub install_directory: PathBuf,
    pub pub_cache_directory: PathBuf,
}

impl DartLocations {
    /// ```
    /// # use {
    /// #     dotfiles_configurator::{
    /// #         configuration::DartPackageName, machine::dart_reading::DartLocations,
    /// #     },
    /// #     std::path::PathBuf,
    /// # };
    /// let locations = DartLocations {
    ///     install_directory: PathBuf::from("/install"),
    ///     pub_cache_directory: PathBuf::from("/cache"),
    /// };
    ///
    /// assert_eq!(
    ///     locations.bundles_of(&DartPackageName::from("coderabbit_findings")),
    ///     PathBuf::from("/install/app-bundles/coderabbit_findings/git")
    /// );
    /// assert_eq!(locations.mirrors(), PathBuf::from("/cache/git/cache"));
    /// ```
    pub fn bundles_of(&self, name: &DartPackageName) -> PathBuf {
        self.install_directory
            .join("app-bundles")
            .join(name.as_ref())
            .join("git")
    }

    pub fn mirrors(&self) -> PathBuf {
        self.pub_cache_directory.join("git").join("cache")
    }

    pub fn binaries(&self) -> PathBuf {
        self.install_directory.join("bin")
    }

    pub fn shim_of(&self, executable: &str) -> PathBuf {
        self.binaries().join(format!("{executable}{SHIM_SUFFIX}"))
    }
}

#[cfg(target_family = "windows")]
const SHIM_SUFFIX: &str = ".bat";

#[cfg(target_family = "unix")]
const SHIM_SUFFIX: &str = "";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartReading {
    Current,
    Drifted(DartDrift),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartDrift {
    NoBundle,
    SeveralBundles(usize),
    InstalledFromElsewhere(String),
    MissingShims(Vec<String>),
    NoMirror,
    AtAnotherCommit {
        installed: GitCommit,
        desired: GitCommit,
    },
}

impl Display for DartDrift {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DartDrift::NoBundle => formatter.write_str("dart has not installed it"),
            DartDrift::SeveralBundles(count) => {
                write!(
                    formatter,
                    "dart holds {count} bundles of it where it keeps one"
                )
            }
            DartDrift::InstalledFromElsewhere(source) => {
                write!(formatter, "installed from {source}")
            }
            DartDrift::MissingShims(executables) => write!(
                formatter,
                "dart's bin directory holds no shim for {}",
                executables.join(", ")
            ),
            DartDrift::NoMirror => {
                formatter.write_str("pub holds no copy of the repository it is installed from")
            }
            DartDrift::AtAnotherCommit { installed, desired } => write!(
                formatter,
                "installed from {installed}, and the declared reference names {desired}"
            ),
        }
    }
}

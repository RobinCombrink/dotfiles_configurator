use {
    crate::configuration::{BinaryName, CrateName},
    serde::Deserialize,
    std::{
        collections::{BTreeMap, BTreeSet},
        error::Error,
        fmt::{Display, Formatter},
        path::{Path, PathBuf},
        string::FromUtf8Error,
        sync::Arc,
    },
    workspace_lock::{LockRefusal, ManifestRefusal, PathClosureRefusal},
    workspace_members::MembersRefusal,
};

#[cfg(test)]
mod tests;

macro_rules! object_identifier {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(String);

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

object_identifier!(Revision);
object_identifier!(ObjectHash);

impl Revision {
    // 2026-09-14: cargo reuses a build it finds in a shared target directory even when the
    // revision asked for differs, installing the previous revision's binary and reporting
    // success, because it fingerprints a git source by its relative path (rust-lang/cargo#13259,
    // open and S-accepted; the fix, PR #13689, was closed unmerged on 2026-05-31). Naming the
    // directory for the revision is what makes that reuse unreachable.
    pub fn build_directory_in(&self, build_cache: &Path) -> PathBuf {
        build_cache.join(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub crate_subtree: ObjectHash,
    pub workspace_bindings: BTreeMap<String, ObjectHash>,
    pub lock_closure: ObjectHash,
    pub dependency_subtrees: BTreeMap<String, ObjectHash>,
}

impl Fingerprint {
    pub fn difference_from(&self, other: &Self) -> Option<FingerprintDifference> {
        let crate_differs = self.crate_subtree != other.crate_subtree;
        let dependencies_differ = self.workspace_bindings != other.workspace_bindings
            || self.lock_closure != other.lock_closure
            || self.dependency_subtrees != other.dependency_subtrees;

        match (crate_differs, dependencies_differ) {
            (false, false) => None,
            (true, false) => Some(FingerprintDifference::Content),
            (false, true) => Some(FingerprintDifference::Dependencies),
            (true, true) => Some(FingerprintDifference::ContentAndDependencies),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerprintDifference {
    Content,
    Dependencies,
    ContentAndDependencies,
}

impl Display for FingerprintDifference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            FingerprintDifference::Content => "the workspace holds different content for it",
            FingerprintDifference::Dependencies => "its dependencies have changed",
            FingerprintDifference::ContentAndDependencies => {
                "the workspace holds different content for it, and its dependencies have changed"
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberReading {
    pub desired: Fingerprint,
    pub binaries: BTreeSet<BinaryName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReading {
    pub revision: Revision,
    pub members: BTreeMap<CrateName, MemberReading>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferableBinary {
    pub path: String,
    pub name: BinaryName,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberTree {
    pub holds_a_main_file: bool,
    pub inferable_binaries: Vec<InferableBinary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeclaredBinary {
    name: BinaryName,
    path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberManifest {
    pub name: CrateName,
    declared_binaries: Vec<DeclaredBinary>,
}

impl MemberManifest {
    pub fn binaries(&self, tree: &MemberTree) -> BTreeSet<BinaryName> {
        let main_file = self.main_file_binary(tree);
        let inferable = main_file
            .iter()
            .chain(&tree.inferable_binaries)
            .filter(|candidate| !self.already_declares(candidate))
            .map(|candidate| candidate.name.clone());

        self.declared_binaries
            .iter()
            .map(|declared| declared.name.clone())
            .chain(inferable)
            .collect()
    }

    fn main_file_binary(&self, tree: &MemberTree) -> Option<InferableBinary> {
        tree.holds_a_main_file.then(|| InferableBinary {
            path: "src/main.rs".to_owned(),
            name: BinaryName::from(self.name.as_ref()),
        })
    }

    fn already_declares(&self, candidate: &InferableBinary) -> bool {
        self.declared_binaries.iter().any(|declared| {
            declared.name == candidate.name || declared.path.as_deref() == Some(&candidate.path)
        })
    }
}

#[derive(Deserialize)]
struct MemberDocument {
    package: Option<PackageSection>,
    #[serde(default, rename = "bin")]
    binaries: Vec<BinarySection>,
}

#[derive(Deserialize)]
struct BinarySection {
    name: String,
    path: Option<String>,
}

#[derive(Deserialize)]
struct PackageSection {
    name: String,
}

pub fn read_member_manifest(manifest: &str) -> Result<MemberManifest, MemberManifestRefusal> {
    let document: MemberDocument = toml::from_str(manifest)
        .map_err(|error| MemberManifestRefusal::Unparsable(Arc::new(error)))?;

    let Some(package) = document.package else {
        return Err(MemberManifestRefusal::DeclaresNoPackage);
    };

    Ok(MemberManifest {
        name: CrateName::from(package.name),
        declared_binaries: document
            .binaries
            .into_iter()
            .map(|section| DeclaredBinary {
                name: BinaryName::from(section.name),
                path: section.path,
            })
            .collect(),
    })
}

#[derive(Debug, Clone)]
pub enum MemberManifestRefusal {
    Unparsable(Arc<toml::de::Error>),
    DeclaresNoPackage,
}

impl Display for MemberManifestRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MemberManifestRefusal::Unparsable(error) => Display::fmt(error, formatter),
            MemberManifestRefusal::DeclaresNoPackage => {
                formatter.write_str("it declares no [package]")
            }
        }
    }
}

impl Error for MemberManifestRefusal {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            MemberManifestRefusal::Unparsable(error) => Some(error.as_ref()),
            MemberManifestRefusal::DeclaresNoPackage => None,
        }
    }
}

#[derive(Debug)]
pub enum CrateListingFailure {
    Unread(git2::Error),
    NotNamedInUtf8 { parent: PathBuf },
}

impl Display for CrateListingFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CrateListingFailure::Unread(error) => Display::fmt(error, formatter),
            CrateListingFailure::NotNamedInUtf8 { parent } => write!(
                formatter,
                "a directory under \"{}\" holding a Cargo.toml is not named in UTF-8",
                parent.display()
            ),
        }
    }
}

impl Error for CrateListingFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            CrateListingFailure::Unread(error) => Some(error),
            CrateListingFailure::NotNamedInUtf8 { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum WorkspaceReadFailure {
    CloneUnopened {
        clone: PathBuf,
        error: Arc<git2::Error>,
    },
    HeadUnreadable {
        clone: PathBuf,
        error: Arc<git2::Error>,
    },
    NotABranch {
        clone: PathBuf,
    },
    NoLocalBranch {
        clone: PathBuf,
        branch: String,
        error: Arc<git2::Error>,
    },
    TracksNoRemoteBranch {
        clone: PathBuf,
        branch: String,
        error: Arc<git2::Error>,
    },
    TrackedNamesNoCommit {
        clone: PathBuf,
        branch: String,
        error: Arc<git2::Error>,
    },
    RevisionNotInClone {
        revision: Revision,
        error: Arc<git2::Error>,
    },
    TreeUnreadable {
        revision: Revision,
        error: Arc<git2::Error>,
    },
    FileAbsent {
        path: PathBuf,
        error: Arc<git2::Error>,
    },
    NotAFile {
        path: PathBuf,
        error: Arc<git2::Error>,
    },
    NotText {
        path: PathBuf,
        error: Arc<FromUtf8Error>,
    },
    LockRefused(Arc<LockRefusal>),
    MembersRefused(Arc<MembersRefusal<CrateListingFailure>>),
    MemberMissing {
        member: PathBuf,
        revision: Revision,
    },
    BindingRefused {
        directory: PathBuf,
        error: Arc<ManifestRefusal>,
    },
    MemberManifestRefused {
        member: PathBuf,
        refusal: MemberManifestRefusal,
    },
    PathDependenciesRefused {
        member: PathBuf,
        error: Arc<PathClosureRefusal>,
    },
    DependencyMissing {
        directory: PathBuf,
        member: PathBuf,
    },
    BinariesUnreadable {
        directory: PathBuf,
        error: Arc<git2::Error>,
    },
    BinariesNotADirectory {
        directory: PathBuf,
        error: Arc<git2::Error>,
    },
    ContentUnhashed(Arc<git2::Error>),
}

impl Display for WorkspaceReadFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceReadFailure::CloneUnopened { clone, error } => write!(
                formatter,
                "the repository at {} could not be opened: {error}",
                clone.display()
            ),
            WorkspaceReadFailure::HeadUnreadable { clone, error } => write!(
                formatter,
                "the HEAD of the clone at {} could not be read: {error}",
                clone.display()
            ),
            WorkspaceReadFailure::NotABranch { clone } => write!(
                formatter,
                "the revision checked out in the clone at {} is not a branch",
                clone.display()
            ),
            WorkspaceReadFailure::NoLocalBranch {
                clone,
                branch,
                error,
            } => write!(
                formatter,
                "{branch} is not a local branch of the clone at {}: {error}",
                clone.display()
            ),
            WorkspaceReadFailure::TracksNoRemoteBranch {
                clone,
                branch,
                error,
            } => write!(
                formatter,
                "the branch {branch} of the clone at {} tracks no remote branch: {error}",
                clone.display()
            ),
            WorkspaceReadFailure::TrackedNamesNoCommit {
                clone,
                branch,
                error,
            } => write!(
                formatter,
                "what {branch} tracks in the clone at {} does not name a commit: {error}",
                clone.display()
            ),
            WorkspaceReadFailure::RevisionNotInClone { revision, error } => {
                write!(formatter, "{revision} is not in this clone: {error}")
            }
            WorkspaceReadFailure::TreeUnreadable { revision, error } => {
                write!(
                    formatter,
                    "the tree of {revision} could not be read: {error}"
                )
            }
            WorkspaceReadFailure::FileAbsent { path, error } => {
                write!(formatter, "it holds no {}: {error}", path.display())
            }
            WorkspaceReadFailure::NotAFile { path, error } => {
                write!(formatter, "{} is not a file: {error}", path.display())
            }
            WorkspaceReadFailure::NotText { path, error } => {
                write!(formatter, "{} is not text: {error}", path.display())
            }
            WorkspaceReadFailure::LockRefused(error) => Display::fmt(error, formatter),
            WorkspaceReadFailure::MembersRefused(error) => Display::fmt(error, formatter),
            WorkspaceReadFailure::MemberMissing { member, revision } => write!(
                formatter,
                "its [workspace] member \"{}\" is not in the repository at {revision}",
                member.display()
            ),
            WorkspaceReadFailure::BindingRefused { directory, error } => write!(
                formatter,
                "\"{}\" could not be read against its workspace manifest: {error}",
                directory.display()
            ),
            WorkspaceReadFailure::MemberManifestRefused { member, refusal } => write!(
                formatter,
                "the Cargo.toml of \"{}\" could not be read: {refusal}",
                member.display()
            ),
            WorkspaceReadFailure::PathDependenciesRefused { member, error } => write!(
                formatter,
                "the path dependencies of \"{}\" could not be walked: {error}",
                member.display()
            ),
            WorkspaceReadFailure::DependencyMissing { directory, member } => write!(
                formatter,
                "\"{}\", which \"{}\" depends on, is not in the repository",
                directory.display(),
                member.display()
            ),
            WorkspaceReadFailure::BinariesUnreadable { directory, error } => {
                write!(
                    formatter,
                    "{} could not be read: {error}",
                    directory.display()
                )
            }
            WorkspaceReadFailure::BinariesNotADirectory { directory, error } => {
                write!(
                    formatter,
                    "{} is not a directory: {error}",
                    directory.display()
                )
            }
            WorkspaceReadFailure::ContentUnhashed(error) => {
                write!(formatter, "its content could not be hashed: {error}")
            }
        }
    }
}

impl Error for WorkspaceReadFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            WorkspaceReadFailure::CloneUnopened { error, .. }
            | WorkspaceReadFailure::HeadUnreadable { error, .. }
            | WorkspaceReadFailure::NoLocalBranch { error, .. }
            | WorkspaceReadFailure::TracksNoRemoteBranch { error, .. }
            | WorkspaceReadFailure::TrackedNamesNoCommit { error, .. }
            | WorkspaceReadFailure::RevisionNotInClone { error, .. }
            | WorkspaceReadFailure::TreeUnreadable { error, .. }
            | WorkspaceReadFailure::FileAbsent { error, .. }
            | WorkspaceReadFailure::NotAFile { error, .. }
            | WorkspaceReadFailure::BinariesUnreadable { error, .. }
            | WorkspaceReadFailure::BinariesNotADirectory { error, .. }
            | WorkspaceReadFailure::ContentUnhashed(error) => Some(error.as_ref()),
            WorkspaceReadFailure::NotText { error, .. } => Some(error.as_ref()),
            WorkspaceReadFailure::LockRefused(error) => Some(error.as_ref()),
            WorkspaceReadFailure::MembersRefused(error) => Some(error.as_ref()),
            WorkspaceReadFailure::BindingRefused { error, .. } => Some(error.as_ref()),
            WorkspaceReadFailure::MemberManifestRefused { refusal, .. } => Some(refusal),
            WorkspaceReadFailure::PathDependenciesRefused { error, .. } => Some(error.as_ref()),
            WorkspaceReadFailure::NotABranch { .. }
            | WorkspaceReadFailure::MemberMissing { .. }
            | WorkspaceReadFailure::DependencyMissing { .. } => None,
        }
    }
}

use {
    crate::configuration::{BinaryName, CrateName},
    anyhow::{Result, anyhow, bail},
    serde::Deserialize,
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt::Display,
        path::{Path, PathBuf},
    },
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
    pub fn difference_from(&self, other: &Self) -> Option<String> {
        let crate_differs = self.crate_subtree != other.crate_subtree;
        let dependencies_differ = self.workspace_bindings != other.workspace_bindings
            || self.lock_closure != other.lock_closure
            || self.dependency_subtrees != other.dependency_subtrees;

        match (crate_differs, dependencies_differ) {
            (false, false) => None,
            (true, false) => Some("the workspace holds different content for it".to_owned()),
            (false, true) => Some("its dependencies have changed".to_owned()),
            (true, true) => Some(
                "the workspace holds different content for it, and its dependencies have changed"
                    .to_owned(),
            ),
        }
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
struct WorkspaceDocument {
    workspace: Option<WorkspaceSection>,
}

#[derive(Deserialize)]
struct WorkspaceSection {
    #[serde(default)]
    members: Vec<String>,
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

pub fn member_paths(manifest: &str) -> Result<Vec<String>> {
    let document: WorkspaceDocument =
        toml::from_str(manifest).map_err(|error| anyhow!("{error}"))?;

    let Some(workspace) = document.workspace else {
        bail!("its Cargo.toml declares no [workspace]");
    };

    for path in &workspace.members {
        if path.contains('*') {
            bail!("its [workspace] members entry \"{path}\" is a glob, which is not resolved");
        }
    }

    Ok(workspace.members)
}

pub fn read_member_manifest(manifest: &str) -> Result<MemberManifest> {
    let document: MemberDocument = toml::from_str(manifest).map_err(|error| anyhow!("{error}"))?;

    let Some(package) = document.package else {
        bail!("a member's Cargo.toml declares no [package]");
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

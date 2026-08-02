use {
    crate::configuration::CrateName,
    anyhow::{Result, anyhow, bail},
    serde::Deserialize,
    std::{collections::BTreeMap, fmt::Display},
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub crate_subtree: ObjectHash,
    pub workspace_manifest: ObjectHash,
    pub lockfile: ObjectHash,
}

impl Fingerprint {
    pub fn difference_from(&self, other: &Self) -> Option<String> {
        let crate_differs = self.crate_subtree != other.crate_subtree;
        let dependencies_differ =
            self.workspace_manifest != other.workspace_manifest || self.lockfile != other.lockfile;

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
    pub installed: Option<Fingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReading {
    pub revision: Revision,
    pub members: BTreeMap<CrateName, MemberReading>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberTree {
    pub holds_a_main_file: bool,
    pub holds_a_binaries_directory: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberManifest {
    pub name: CrateName,
    pub declares_a_binary: bool,
}

impl MemberManifest {
    pub fn builds_a_binary(&self, tree: &MemberTree) -> bool {
        self.declares_a_binary || tree.holds_a_main_file || tree.holds_a_binaries_directory
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
    binaries: Vec<toml::Value>,
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
        declares_a_binary: !document.binaries.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_manifest_lists_the_paths_of_its_members() {
        let manifest = r#"
            [workspace]
            resolver = "2"
            members = ["tools/stop-gate", "tools/ci-checks"]
        "#;

        assert_eq!(
            member_paths(manifest).unwrap(),
            vec!["tools/stop-gate".to_owned(), "tools/ci-checks".to_owned()]
        );
    }

    #[test]
    fn a_member_path_holding_a_glob_is_refused_by_name() {
        let manifest = r#"
            [workspace]
            members = ["tools/*"]
        "#;

        let error = member_paths(manifest).unwrap_err().to_string();

        assert!(
            error.contains("tools/*"),
            "expected the message to name the entry, got: {error}"
        );
    }

    #[test]
    fn a_manifest_that_declares_no_workspace_is_refused() {
        let manifest = r#"
            [package]
            name = "stop-gate"
        "#;

        assert!(member_paths(manifest).is_err());
    }

    #[test]
    fn a_member_is_known_by_the_name_its_own_manifest_gives_it() {
        let manifest = r#"
            [package]
            name = "session-mining"
            version = "0.1.0"
        "#;

        assert_eq!(
            read_member_manifest(manifest).unwrap().name,
            CrateName::from("session-mining")
        );
    }

    #[test]
    fn a_member_whose_manifest_declares_a_binary_section_is_installable() {
        let manifest = r#"
            [package]
            name = "session-mining"

            [[bin]]
            name = "session_census"
            path = "src/bin/session_census.rs"
        "#;

        assert!(
            read_member_manifest(manifest)
                .unwrap()
                .builds_a_binary(&MemberTree::default())
        );
    }

    #[test]
    fn a_member_holding_a_main_file_is_installable() {
        let manifest = r#"
            [package]
            name = "stop-gate"
        "#;
        let tree = MemberTree {
            holds_a_main_file: true,
            holds_a_binaries_directory: false,
        };

        assert!(
            read_member_manifest(manifest)
                .unwrap()
                .builds_a_binary(&tree)
        );
    }

    #[test]
    fn a_member_holding_a_binaries_directory_is_installable() {
        let manifest = r#"
            [package]
            name = "session-mining"
        "#;
        let tree = MemberTree {
            holds_a_main_file: false,
            holds_a_binaries_directory: true,
        };

        assert!(
            read_member_manifest(manifest)
                .unwrap()
                .builds_a_binary(&tree)
        );
    }

    #[test]
    fn a_member_holding_only_a_library_is_not_installable() {
        let manifest = r#"
            [package]
            name = "shared"

            [lib]
            path = "src/lib.rs"
        "#;

        assert!(
            !read_member_manifest(manifest)
                .unwrap()
                .builds_a_binary(&MemberTree::default())
        );
    }

    #[test]
    fn a_fingerprint_matching_another_reports_no_difference() {
        let fingerprint = Fingerprint {
            crate_subtree: ObjectHash::from("aaa"),
            workspace_manifest: ObjectHash::from("bbb"),
            lockfile: ObjectHash::from("ccc"),
        };

        assert_eq!(fingerprint.difference_from(&fingerprint), None);
    }

    #[test]
    fn a_fingerprint_differing_only_in_its_lockfile_names_the_dependencies() {
        let installed = Fingerprint {
            crate_subtree: ObjectHash::from("aaa"),
            workspace_manifest: ObjectHash::from("bbb"),
            lockfile: ObjectHash::from("ccc"),
        };
        let desired = Fingerprint {
            lockfile: ObjectHash::from("ddd"),
            ..installed.clone()
        };

        assert_eq!(
            desired.difference_from(&installed),
            Some("its dependencies have changed".to_owned())
        );
    }
}

use {
    crate::configuration::CrateName,
    anyhow::{Result, anyhow, bail},
    serde::Deserialize,
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt::Display,
    },
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
object_identifier!(BinaryName);

impl BinaryName {
    pub fn file_name(&self) -> String {
        format!("{self}{}", std::env::consts::EXE_SUFFIX)
    }
}

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
    pub absent_binaries: BTreeSet<BinaryName>,
}

impl MemberReading {
    pub fn difference(&self) -> Option<String> {
        let Some(installed) = &self.installed else {
            return Some("cargo has not installed it".to_owned());
        };

        let differences: Vec<String> = self
            .desired
            .difference_from(installed)
            .into_iter()
            .chain(absence_of(&self.absent_binaries))
            .collect();

        match differences.is_empty() {
            true => None,
            false => Some(differences.join(", and ")),
        }
    }
}

fn absence_of(binaries: &BTreeSet<BinaryName>) -> Option<String> {
    let names: Vec<String> = binaries.iter().map(BinaryName::to_string).collect();
    let (subject, verb) = match names.len() {
        0 => return None,
        1 => (names.join(""), "is"),
        _ => (
            names[..names.len() - 1].join(", ") + " and " + &names[names.len() - 1],
            "are",
        ),
    };

    Some(format!(
        "{subject} {verb} missing from the directory cargo installs into"
    ))
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
    pub binaries_directory: Vec<InferableBinary>,
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
            .chain(&tree.binaries_directory)
            .filter(|candidate| !self.already_declares(candidate))
            .map(|candidate| candidate.name.clone());

        self.declared_binaries
            .iter()
            .map(|declared| declared.name.clone())
            .chain(inferable)
            .collect()
    }

    pub fn builds_a_binary(&self, tree: &MemberTree) -> bool {
        !self.binaries(tree).is_empty()
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

    fn binaries_of(manifest: &str, tree: &MemberTree) -> Vec<String> {
        read_member_manifest(manifest)
            .unwrap()
            .binaries(tree)
            .iter()
            .map(BinaryName::to_string)
            .collect()
    }

    fn under_binaries_directory(stems: &[&str]) -> MemberTree {
        MemberTree {
            holds_a_main_file: false,
            binaries_directory: stems
                .iter()
                .map(|stem| InferableBinary {
                    path: format!("src/bin/{stem}.rs"),
                    name: BinaryName::from(*stem),
                })
                .collect(),
        }
    }

    #[test]
    fn a_member_is_known_by_the_binaries_its_manifest_names() {
        let manifest = r#"
            [package]
            name = "session-mining"

            [[bin]]
            name = "session-census"
            path = "src/bin/session_census.rs"
        "#;

        assert_eq!(
            binaries_of(manifest, &MemberTree::default()),
            vec!["session-census".to_owned()]
        );
    }

    #[test]
    fn a_member_holding_a_main_file_builds_a_binary_named_for_the_package() {
        let manifest = r#"
            [package]
            name = "stop-gate"
        "#;
        let tree = MemberTree {
            holds_a_main_file: true,
            binaries_directory: Vec::new(),
        };

        assert_eq!(binaries_of(manifest, &tree), vec!["stop-gate".to_owned()]);
    }

    #[test]
    fn a_member_builds_a_binary_for_every_file_under_its_binaries_directory() {
        let manifest = r#"
            [package]
            name = "session-mining"
        "#;

        assert_eq!(
            binaries_of(
                manifest,
                &under_binaries_directory(&["sweep-status", "reach"])
            ),
            vec!["reach".to_owned(), "sweep-status".to_owned()]
        );
    }

    #[test]
    fn a_file_a_binary_section_already_claims_does_not_also_build_one_named_for_its_stem() {
        let manifest = r#"
            [package]
            name = "session-mining"

            [[bin]]
            name = "tool-use-statistics"
            path = "src/bin/tool_use_statistics.rs"
        "#;

        assert_eq!(
            binaries_of(
                manifest,
                &under_binaries_directory(&["tool_use_statistics"])
            ),
            vec!["tool-use-statistics".to_owned()]
        );
    }

    #[test]
    fn a_main_file_a_binary_section_already_claims_does_not_also_build_one_named_for_the_package() {
        let manifest = r#"
            [package]
            name = "claude-session"

            [[bin]]
            name = "claude-live-set"
            path = "src/main.rs"
        "#;
        let tree = MemberTree {
            holds_a_main_file: true,
            binaries_directory: Vec::new(),
        };

        assert_eq!(
            binaries_of(manifest, &tree),
            vec!["claude-live-set".to_owned()]
        );
    }

    #[test]
    fn a_binary_section_naming_no_path_does_not_double_the_file_it_infers() {
        let manifest = r#"
            [package]
            name = "session-mining"

            [[bin]]
            name = "sweep"
        "#;

        assert_eq!(
            binaries_of(manifest, &under_binaries_directory(&["sweep"])),
            vec!["sweep".to_owned()]
        );
    }

    #[test]
    fn a_member_holding_only_a_library_builds_no_binary() {
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

    fn a_fingerprint() -> Fingerprint {
        Fingerprint {
            crate_subtree: ObjectHash::from("aaa"),
            workspace_manifest: ObjectHash::from("bbb"),
            lockfile: ObjectHash::from("ccc"),
        }
    }

    fn reading_missing(absent: &[&str]) -> MemberReading {
        MemberReading {
            desired: a_fingerprint(),
            installed: Some(a_fingerprint()),
            absent_binaries: absent.iter().map(|name| BinaryName::from(*name)).collect(),
        }
    }

    #[test]
    fn a_member_whose_every_binary_is_on_disk_at_the_installed_revision_has_not_drifted() {
        assert_eq!(reading_missing(&[]).difference(), None);
    }

    #[test]
    fn a_member_missing_one_binary_drifts_and_names_it() {
        assert_eq!(
            reading_missing(&["tool-use-statistics"]).difference(),
            Some(
                "tool-use-statistics is missing from the directory cargo installs into".to_owned()
            )
        );
    }

    #[test]
    fn a_member_missing_several_binaries_names_every_one_of_them() {
        assert_eq!(
            reading_missing(&["sweep-status", "session-census", "reach"]).difference(),
            Some(
                "reach, session-census and sweep-status are missing from the directory cargo \
                 installs into"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_member_that_both_moved_on_and_lost_a_binary_reports_each_reason() {
        let reading = MemberReading {
            desired: Fingerprint {
                lockfile: ObjectHash::from("ddd"),
                ..a_fingerprint()
            },
            installed: Some(a_fingerprint()),
            absent_binaries: BTreeSet::from([BinaryName::from("sweep")]),
        };

        assert_eq!(
            reading.difference(),
            Some(
                "its dependencies have changed, and sweep is missing from the directory cargo \
                 installs into"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_member_cargo_never_installed_drifts_without_listing_binaries_it_could_not_have() {
        let reading = MemberReading {
            desired: a_fingerprint(),
            installed: None,
            absent_binaries: BTreeSet::from([BinaryName::from("sweep")]),
        };

        assert_eq!(
            reading.difference(),
            Some("cargo has not installed it".to_owned())
        );
    }
}

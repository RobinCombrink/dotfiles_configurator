use {
    cargo_lock::{Lockfile, dependency::Tree},
    std::{
        collections::BTreeSet,
        fmt::{Display, Formatter},
    },
};

#[derive(Debug)]
pub enum LockRefusal {
    Unparsable(cargo_lock::Error),
    Unresolvable(cargo_lock::Error),
    NotExactlyOneEntryFor(String),
}

impl Display for LockRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparsable(error) => {
                write!(formatter, "its Cargo.lock could not be read: {error}")
            }
            Self::Unresolvable(error) => {
                write!(formatter, "its Cargo.lock could not be resolved: {error}")
            }
            Self::NotExactlyOneEntryFor(member) => write!(
                formatter,
                "its Cargo.lock does not hold exactly one entry for the member {member}"
            ),
        }
    }
}

impl std::error::Error for LockRefusal {}

#[derive(Debug)]
pub enum ManifestRefusal {
    Unparsable(toml::de::Error),
    Unwritable(toml::ser::Error),
}

impl Display for ManifestRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparsable(error) => write!(formatter, "{error}"),
            Self::Unwritable(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ManifestRefusal {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LockClosure(String);

impl LockClosure {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct WorkspaceLock {
    tree: Tree,
}

impl WorkspaceLock {
    pub fn read(lock: &str) -> Result<Self, LockRefusal> {
        let lockfile: Lockfile = lock.parse().map_err(LockRefusal::Unparsable)?;
        let tree = lockfile
            .dependency_tree()
            .map_err(LockRefusal::Unresolvable)?;
        Ok(Self { tree })
    }

    pub fn closure_of(&self, member: &str) -> Result<LockClosure, LockRefusal> {
        let graph = self.tree.graph();
        let mut entries = self
            .tree
            .nodes()
            .iter()
            .filter(|(entry, _)| entry.name.as_str() == member && entry.source.is_none())
            .map(|(_, node)| *node);
        let (Some(root), None) = (entries.next(), entries.next()) else {
            return Err(LockRefusal::NotExactlyOneEntryFor(member.to_owned()));
        };

        let mut reached = BTreeSet::new();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if reached.insert(node) {
                pending.extend(graph.neighbors(node));
            }
        }

        let mut packages: Vec<_> = reached.into_iter().map(|node| &graph[node]).collect();
        packages.sort_by_key(|package| cargo_lock::Dependency::from(*package));

        let mut canonical = String::new();
        for package in packages {
            let source = package
                .source
                .as_ref()
                .map_or_else(|| "path".to_owned(), ToString::to_string);
            let checksum = package
                .checksum
                .as_ref()
                .map_or_else(|| "none".to_owned(), ToString::to_string);
            canonical.push_str(&format!(
                "{} {} {source} {checksum}\n",
                package.name, package.version
            ));

            let mut dependencies: Vec<_> = package.dependencies.iter().collect();
            dependencies.sort();
            for dependency in dependencies {
                canonical.push_str(&format!("  {dependency}\n"));
            }
        }
        Ok(LockClosure(canonical))
    }
}

pub fn manifest_without_membership(manifest: &str) -> Result<String, ManifestRefusal> {
    let mut document: toml::Table =
        toml::from_str(manifest).map_err(ManifestRefusal::Unparsable)?;
    if let Some(toml::Value::Table(workspace)) = document.get_mut("workspace") {
        workspace.remove("members");
        workspace.remove("exclude");
    }
    toml::to_string(&document).map_err(ManifestRefusal::Unwritable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member_entry(name: &str, dependencies: &[&str]) -> String {
        format!(
            "[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\ndependencies = [{}]\n",
            quoted(dependencies)
        )
    }

    fn registry_entry(name: &str, version: &str, dependencies: &[&str]) -> String {
        format!(
            "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n\
             source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
             checksum = \"{}\"\ndependencies = [{}]\n",
            "ab".repeat(32),
            quoted(dependencies)
        )
    }

    fn quoted(names: &[&str]) -> String {
        names
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn lock_of(entries: &[String]) -> String {
        format!("version = 4\n\n{}", entries.join("\n"))
    }

    fn closure_of(member: &str, lock: &str) -> LockClosure {
        match WorkspaceLock::read(lock) {
            Ok(lock) => lock.closure_of(member).unwrap(),
            Err(refusal) => panic!("expected the lock to be read, got: {refusal}"),
        }
    }

    fn alpha_through_left_pad_and_pad_core_on_pad_bytes(pad_bytes_version: &str) -> String {
        lock_of(&[
            member_entry("alpha", &["left-pad"]),
            member_entry("beta", &[]),
            registry_entry("left-pad", "1.3.0", &["pad-core"]),
            registry_entry("pad-core", "1.0.0", &["pad-bytes"]),
            registry_entry("pad-bytes", pad_bytes_version, &[]),
        ])
    }

    #[test]
    fn a_new_version_of_a_dependency_several_levels_beneath_a_member_changes_its_closure() {
        assert_ne!(
            closure_of(
                "alpha",
                &alpha_through_left_pad_and_pad_core_on_pad_bytes("1.0.0")
            ),
            closure_of(
                "alpha",
                &alpha_through_left_pad_and_pad_core_on_pad_bytes("1.0.1")
            )
        );
    }

    #[test]
    fn a_dependency_added_to_one_member_leaves_another_members_closure_unchanged() {
        let before = lock_of(&[member_entry("alpha", &[]), member_entry("delta", &[])]);
        let after = lock_of(&[
            member_entry("alpha", &[]),
            member_entry("delta", &["left-pad"]),
            registry_entry("left-pad", "1.3.0", &[]),
        ]);

        assert_eq!(closure_of("alpha", &before), closure_of("alpha", &after));
    }

    #[test]
    fn a_lock_holding_no_entry_for_a_member_refuses_its_closure_naming_the_member() {
        let lock = WorkspaceLock::read(&lock_of(&[member_entry("beta", &[])])).unwrap();

        let refusal = match lock.closure_of("alpha") {
            Ok(closure) => panic!("expected a refusal, got: {closure:?}"),
            Err(refusal) => refusal,
        };

        let LockRefusal::NotExactlyOneEntryFor(member) = refusal else {
            panic!("expected the refusal to be about the member, got: {refusal}");
        };
        assert_eq!(member, "alpha");
    }

    #[test]
    fn a_lock_naming_a_dependency_it_holds_no_entry_for_is_refused_as_unresolvable() {
        let lock = lock_of(&[member_entry("alpha", &["ghost 1.0.0"])]);

        let refusal = match WorkspaceLock::read(&lock) {
            Ok(_) => panic!("expected the lock to be refused"),
            Err(refusal) => refusal,
        };

        let LockRefusal::Unresolvable(_) = refusal else {
            panic!("expected the lock to be refused as unresolvable, got: {refusal}");
        };
    }

    #[test]
    fn a_workspace_manifest_differing_only_in_its_excluded_paths_reads_the_same() {
        let excluding_nothing = r#"
            [workspace]
            members = ["tools/stop-gate"]
        "#;
        let excluding_a_path = r#"
            [workspace]
            members = ["tools/stop-gate"]
            exclude = ["tools/scratch"]
        "#;

        assert_eq!(
            manifest_without_membership(excluding_nothing).unwrap(),
            manifest_without_membership(excluding_a_path).unwrap()
        );
    }

    #[test]
    fn a_workspace_manifest_reads_the_same_whatever_order_its_keys_are_written_in() {
        let one_order = r#"
            [profile.release]
            lto = true
            strip = true

            [workspace]
            resolver = "2"
            members = ["tools/stop-gate"]
        "#;
        let another_order = r#"
            [workspace]
            members = ["tools/stop-gate"]
            resolver = "2"

            [profile.release]
            strip = true
            lto = true
        "#;

        assert_eq!(
            manifest_without_membership(one_order).unwrap(),
            manifest_without_membership(another_order).unwrap()
        );
    }
}

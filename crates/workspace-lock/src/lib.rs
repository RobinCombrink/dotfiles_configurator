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
    InheritsWhatTheWorkspaceDoesNotHold(String),
    UnrecognisedEdition(String),
}

impl Display for ManifestRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparsable(error) => write!(formatter, "{error}"),
            Self::Unwritable(error) => write!(formatter, "{error}"),
            Self::InheritsWhatTheWorkspaceDoesNotHold(key) => write!(
                formatter,
                "it inherits {key}, which its workspace manifest does not hold"
            ),
            Self::UnrecognisedEdition(edition) => write!(
                formatter,
                "its root package names the edition {edition}, which implies no known resolver"
            ),
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

const BOUND_WITHOUT_OPTING_IN: [&str; 3] = ["profile", "patch", "replace"];

const DEPENDENCY_TABLES: [&str; 5] = [
    "dependencies",
    "dev-dependencies",
    "dev_dependencies",
    "build-dependencies",
    "build_dependencies",
];

pub fn inherited_by(root_manifest: &str, member_manifest: &str) -> Result<String, ManifestRefusal> {
    let root: toml::Table = toml::from_str(root_manifest).map_err(ManifestRefusal::Unparsable)?;
    let member: toml::Table =
        toml::from_str(member_manifest).map_err(ManifestRefusal::Unparsable)?;
    let workspace = root.get("workspace").and_then(toml::Value::as_table);

    let mut binding: toml::Table = BOUND_WITHOUT_OPTING_IN
        .iter()
        .filter_map(|section| Some(((*section).to_owned(), root.get(*section)?.clone())))
        .collect();

    let mut workspace_binding = toml::Table::new();
    workspace_binding.insert("resolver".to_owned(), effective_resolver(&root, workspace)?);

    let package_fields = member
        .get("package")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(opted_in_keys);
    insert_unless_empty(
        &mut workspace_binding,
        "package",
        held_entries(workspace, "package", package_fields)?,
    );
    insert_unless_empty(
        &mut workspace_binding,
        "dependencies",
        held_entries(workspace, "dependencies", inherited_dependencies(&member))?,
    );

    if member.get("lints").is_some_and(opts_into_the_workspace) {
        let lints = workspace
            .and_then(|workspace| workspace.get("lints"))
            .ok_or_else(|| {
                ManifestRefusal::InheritsWhatTheWorkspaceDoesNotHold("workspace.lints".to_owned())
            })?;
        workspace_binding.insert("lints".to_owned(), lints.clone());
    }

    insert_unless_empty(&mut binding, "workspace", workspace_binding);
    toml::to_string(&binding).map_err(ManifestRefusal::Unwritable)
}

fn effective_resolver(
    root: &toml::Table,
    workspace: Option<&toml::Table>,
) -> Result<toml::Value, ManifestRefusal> {
    let root_package = root.get("package").and_then(toml::Value::as_table);
    let declared = workspace
        .and_then(|workspace| workspace.get("resolver"))
        .or_else(|| root_package.and_then(|package| package.get("resolver")));
    if let Some(resolver) = declared {
        return Ok(resolver.clone());
    }

    let edition = match root_package.and_then(|package| package.get("edition")) {
        None => None,
        Some(edition) if opts_into_the_workspace(edition) => Some(
            workspace
                .and_then(|workspace| workspace.get("package"))
                .and_then(|package| package.get("edition"))
                .ok_or_else(|| {
                    ManifestRefusal::InheritsWhatTheWorkspaceDoesNotHold(
                        "workspace.package.edition".to_owned(),
                    )
                })?,
        ),
        Some(edition) => Some(edition),
    };
    let resolver = match edition.map(|edition| (edition, edition.as_str())) {
        None | Some((_, Some("2015" | "2018"))) => "1",
        Some((_, Some("2021"))) => "2",
        Some((_, Some("2024"))) => "3",
        Some((edition, _)) => {
            return Err(ManifestRefusal::UnrecognisedEdition(edition.to_string()));
        }
    };
    Ok(toml::Value::String(resolver.to_owned()))
}

fn opts_into_the_workspace(value: &toml::Value) -> bool {
    value
        .as_table()
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

fn opted_in_keys(table: &toml::Table) -> impl Iterator<Item = &String> {
    table
        .iter()
        .filter(|(_, value)| opts_into_the_workspace(value))
        .map(|(key, _)| key)
}

fn inherited_dependencies(member: &toml::Table) -> BTreeSet<&String> {
    let per_target = member
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|targets| targets.values().filter_map(toml::Value::as_table));

    std::iter::once(member)
        .chain(per_target)
        .flat_map(|sections| {
            DEPENDENCY_TABLES
                .iter()
                .filter_map(|table| sections.get(*table).and_then(toml::Value::as_table))
        })
        .flat_map(opted_in_keys)
        .collect()
}

fn held_entries<'a>(
    workspace: Option<&toml::Table>,
    section: &str,
    keys: impl IntoIterator<Item = &'a String>,
) -> Result<toml::Table, ManifestRefusal> {
    let held = workspace
        .and_then(|workspace| workspace.get(section))
        .and_then(toml::Value::as_table);

    keys.into_iter()
        .map(|key| {
            let value = held.and_then(|held| held.get(key)).ok_or_else(|| {
                ManifestRefusal::InheritsWhatTheWorkspaceDoesNotHold(format!(
                    "workspace.{section}.{key}"
                ))
            })?;
            Ok((key.clone(), value.clone()))
        })
        .collect()
}

fn insert_unless_empty(table: &mut toml::Table, key: &str, value: toml::Table) {
    if !value.is_empty() {
        table.insert(key.to_owned(), toml::Value::Table(value));
    }
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

    fn alpha_inheriting(dependency_sections: &str) -> String {
        format!("[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n\n{dependency_sections}")
    }

    fn workspace_depending_on(serde_version: &str, left_pad_version: &str) -> String {
        format!(
            "[workspace]\nmembers = [\"tools/alpha\"]\n\n[workspace.dependencies]\n\
             serde = \"{serde_version}\"\nleft-pad = \"{left_pad_version}\"\n"
        )
    }

    fn inherited_text(root_manifest: &str, member_manifest: &str) -> String {
        match inherited_by(root_manifest, member_manifest) {
            Ok(text) => text,
            Err(refusal) => panic!("expected what the member inherits, got: {refusal}"),
        }
    }

    #[test]
    fn an_edit_to_a_workspace_dependency_a_member_does_not_inherit_leaves_what_it_inherits_unchanged()
     {
        let alpha = alpha_inheriting("[dependencies]\nserde = { workspace = true }\n");

        assert_eq!(
            inherited_text(&workspace_depending_on("1.0.0", "1.3.0"), &alpha),
            inherited_text(&workspace_depending_on("1.0.0", "1.4.0"), &alpha)
        );
    }

    fn moved_by_editing_left_pad(dependency_sections: &str) -> bool {
        let alpha = alpha_inheriting(dependency_sections);

        inherited_text(&workspace_depending_on("1.0.0", "1.3.0"), &alpha)
            != inherited_text(&workspace_depending_on("1.0.0", "1.4.0"), &alpha)
    }

    #[test]
    fn an_edit_to_a_workspace_dependency_a_member_inherits_changes_what_it_inherits() {
        assert!(moved_by_editing_left_pad(
            "[dependencies]\nleft-pad = { workspace = true }\n"
        ));
    }

    #[test]
    fn an_edit_to_a_workspace_dependency_a_member_inherits_only_for_its_tests_changes_what_it_inherits()
     {
        assert!(moved_by_editing_left_pad(
            "[dev-dependencies]\nleft-pad = { workspace = true }\n"
        ));
    }

    #[test]
    fn an_edit_to_a_workspace_dependency_a_member_inherits_to_build_for_one_platform_changes_what_it_inherits()
     {
        assert!(moved_by_editing_left_pad(
            "[target.'cfg(windows)'.build-dependencies]\nleft-pad = { workspace = true }\n"
        ));
    }

    fn workspace_linting(unsafe_code: &str) -> String {
        format!(
            "[workspace]\nmembers = [\"tools/alpha\"]\n\n\
             [workspace.lints.rust]\nunsafe_code = \"{unsafe_code}\"\n"
        )
    }

    fn moved_by_editing_the_workspace_lints(member_sections: &str) -> bool {
        let alpha = alpha_inheriting(member_sections);

        inherited_text(&workspace_linting("warn"), &alpha)
            != inherited_text(&workspace_linting("forbid"), &alpha)
    }

    #[test]
    fn an_edit_to_the_workspace_lints_changes_what_a_member_opting_into_them_inherits() {
        assert!(moved_by_editing_the_workspace_lints(
            "[lints]\nworkspace = true\n"
        ));
    }

    #[test]
    fn an_edit_to_the_workspace_lints_leaves_what_a_member_not_opting_into_them_inherits_unchanged()
    {
        assert!(!moved_by_editing_the_workspace_lints(""));
    }

    fn workspace_describing_its_packages(version: &str, authors: &str) -> String {
        format!(
            "[workspace]\nmembers = [\"tools/alpha\"]\n\n\
             [workspace.package]\nversion = \"{version}\"\nauthors = [\"{authors}\"]\n"
        )
    }

    #[test]
    fn an_edit_to_a_package_field_a_member_inherits_changes_what_it_inherits() {
        let alpha = "[package]\nname = \"alpha\"\nversion.workspace = true\n";

        assert_ne!(
            inherited_text(&workspace_describing_its_packages("0.1.0", "Alice"), alpha),
            inherited_text(&workspace_describing_its_packages("0.2.0", "Alice"), alpha)
        );
    }

    #[test]
    fn an_edit_to_a_package_field_a_member_does_not_inherit_leaves_what_it_inherits_unchanged() {
        let alpha = "[package]\nname = \"alpha\"\nversion.workspace = true\n";

        assert_eq!(
            inherited_text(&workspace_describing_its_packages("0.1.0", "Alice"), alpha),
            inherited_text(&workspace_describing_its_packages("0.1.0", "Bob"), alpha)
        );
    }

    fn moved_for_a_member_inheriting_nothing(before: &str, after: &str) -> bool {
        let alpha = alpha_inheriting("");
        let workspace = "[workspace]\nmembers = [\"tools/alpha\"]\n";

        inherited_text(&format!("{workspace}{before}"), &alpha)
            != inherited_text(&format!("{workspace}{after}"), &alpha)
    }

    #[test]
    fn an_edit_to_a_profile_changes_what_every_member_inherits() {
        assert!(moved_for_a_member_inheriting_nothing(
            "\n[profile.release]\nlto = false\n",
            "\n[profile.release]\nlto = true\n"
        ));
    }

    #[test]
    fn an_edit_to_the_resolver_changes_what_every_member_inherits() {
        assert!(moved_for_a_member_inheriting_nothing(
            "resolver = \"2\"\n",
            "resolver = \"3\"\n"
        ));
    }

    fn workspace_with_a_root_package(package_lines: &str, workspace_lines: &str) -> String {
        format!(
            "[package]\nname = \"root\"\n{package_lines}\n\
             [workspace]\nmembers = [\"tools/alpha\"]\n{workspace_lines}"
        )
    }

    #[test]
    fn an_edit_to_the_root_package_edition_changes_what_every_member_inherits_when_the_workspace_names_no_resolver()
     {
        let alpha = alpha_inheriting("");

        assert_ne!(
            inherited_text(
                &workspace_with_a_root_package("edition = \"2018\"\n", ""),
                &alpha
            ),
            inherited_text(
                &workspace_with_a_root_package("edition = \"2021\"\n", ""),
                &alpha
            )
        );
    }

    #[test]
    fn an_edit_to_the_root_package_edition_leaves_what_every_member_inherits_unchanged_when_the_workspace_names_a_resolver()
     {
        let alpha = alpha_inheriting("");

        assert_eq!(
            inherited_text(
                &workspace_with_a_root_package("edition = \"2021\"\n", "resolver = \"2\"\n"),
                &alpha
            ),
            inherited_text(
                &workspace_with_a_root_package("edition = \"2024\"\n", "resolver = \"2\"\n"),
                &alpha
            )
        );
    }

    #[test]
    fn every_spelling_yielding_the_same_resolver_gives_a_member_the_same_inheritance() {
        let alpha = alpha_inheriting("");
        let spellings = [
            "[workspace]\nmembers = [\"tools/alpha\"]\nresolver = \"2\"\n".to_owned(),
            workspace_with_a_root_package("edition = \"2015\"\nresolver = \"2\"\n", ""),
            workspace_with_a_root_package("edition = \"2021\"\n", ""),
            workspace_with_a_root_package(
                "edition.workspace = true\n",
                "\n[workspace.package]\nedition = \"2021\"\n",
            ),
        ];

        let texts: BTreeSet<String> = spellings
            .iter()
            .map(|root| inherited_text(root, &alpha))
            .collect();

        assert_eq!(texts.len(), 1, "expected one text, got: {texts:?}");
    }

    #[test]
    fn a_root_package_naming_an_edition_that_implies_no_known_resolver_is_refused() {
        let root = workspace_with_a_root_package("edition = \"2077\"\n", "");

        let refusal = match inherited_by(&root, &alpha_inheriting("")) {
            Ok(text) => panic!("expected a refusal, got: {text}"),
            Err(refusal) => refusal,
        };

        let ManifestRefusal::UnrecognisedEdition(edition) = refusal else {
            panic!("expected the refusal to name the edition, got: {refusal}");
        };
        assert_eq!(edition, "\"2077\"");
    }

    #[test]
    fn an_edit_to_a_patch_changes_what_every_member_inherits() {
        assert!(moved_for_a_member_inheriting_nothing(
            "\n[patch.crates-io]\nleft-pad = { path = \"vendor/left-pad\" }\n",
            "\n[patch.crates-io]\nleft-pad = { path = \"vendor/left-pad-fork\" }\n"
        ));
    }

    #[test]
    fn an_edit_to_a_replacement_changes_what_every_member_inherits() {
        assert!(moved_for_a_member_inheriting_nothing(
            "\n[replace]\n\"left-pad:1.3.0\" = { path = \"vendor/left-pad\" }\n",
            "\n[replace]\n\"left-pad:1.3.0\" = { path = \"vendor/left-pad-fork\" }\n"
        ));
    }

    #[test]
    fn a_change_of_membership_leaves_what_a_member_inherits_unchanged() {
        assert!(!moved_for_a_member_inheriting_nothing(
            "",
            "exclude = [\"tools/scratch\"]\ndefault-members = [\"tools/alpha\"]\n"
        ));
    }

    #[test]
    fn what_a_member_inherits_reads_the_same_whatever_order_the_workspace_is_written_in() {
        let alpha = alpha_inheriting(
            "[dependencies]\nserde = { workspace = true }\nleft-pad = { workspace = true }\n",
        );
        let one_order = "[profile.release]\nlto = true\nstrip = true\n\n\
            [workspace]\nresolver = \"2\"\nmembers = [\"tools/alpha\"]\n\n\
            [workspace.dependencies]\nserde = \"1\"\nleft-pad = \"1\"\n";
        let another_order = "[workspace]\nmembers = [\"tools/alpha\"]\nresolver = \"2\"\n\n\
            [workspace.dependencies]\nleft-pad = \"1\"\nserde = \"1\"\n\n\
            [profile.release]\nstrip = true\nlto = true\n";

        assert_eq!(
            inherited_text(one_order, &alpha),
            inherited_text(another_order, &alpha)
        );
    }

    fn refusal_naming(root_manifest: &str, member_manifest: &str) -> String {
        match inherited_by(root_manifest, member_manifest) {
            Ok(text) => panic!("expected a refusal, got: {text}"),
            Err(ManifestRefusal::InheritsWhatTheWorkspaceDoesNotHold(key)) => key,
            Err(refusal) => panic!("expected a refusal naming what is missing, got: {refusal}"),
        }
    }

    #[test]
    fn a_member_inheriting_a_dependency_the_workspace_does_not_hold_is_refused_naming_it() {
        let alpha = alpha_inheriting("[dependencies]\nghost = { workspace = true }\n");

        assert_eq!(
            refusal_naming(&workspace_depending_on("1.0.0", "1.3.0"), &alpha),
            "workspace.dependencies.ghost"
        );
    }

    #[test]
    fn a_member_inheriting_lints_from_a_workspace_holding_none_is_refused_naming_them() {
        let alpha = alpha_inheriting("[lints]\nworkspace = true\n");

        assert_eq!(
            refusal_naming(&workspace_depending_on("1.0.0", "1.3.0"), &alpha),
            "workspace.lints"
        );
    }
}

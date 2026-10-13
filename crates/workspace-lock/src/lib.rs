mod path_closure;

pub use path_closure::{BuildClosure, RepositoryDirectory, TestClosure};

use {
    cargo_lock::{Lockfile, dependency::Tree},
    std::{
        collections::BTreeSet,
        fmt::{Display, Formatter},
    },
};

#[cfg(test)]
mod tests;

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

#[derive(Debug)]
pub enum PathClosureRefusal {
    OutsideTheRepository {
        declared_in: RepositoryDirectory,
        path: String,
    },
    Unanswered(RepositoryDirectory),
    Unparsable {
        directory: RepositoryDirectory,
        error: toml::de::Error,
    },
}

impl Display for PathClosureRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutsideTheRepository { declared_in, path } => write!(
                formatter,
                "the path \"{path}\" declared in {} is not inside the repository",
                declared_in.manifest()
            ),
            Self::Unanswered(directory) => write!(
                formatter,
                "{}, which the closure reaches, could not be read",
                directory.manifest()
            ),
            Self::Unparsable { directory, error } => {
                write!(
                    formatter,
                    "{} does not parse: {error}",
                    directory.manifest()
                )
            }
        }
    }
}

impl std::error::Error for PathClosureRefusal {}

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

pub(crate) const DEPENDENCY_TABLES: [&str; 5] = [
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

pub(crate) fn opts_into_the_workspace(value: &toml::Value) -> bool {
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

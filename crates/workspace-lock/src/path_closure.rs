use {
    crate::{DEPENDENCY_TABLES, PathClosureRefusal, opts_into_the_workspace},
    std::{
        collections::BTreeSet,
        fmt::{Display, Formatter},
    },
};

#[cfg(test)]
mod tests;

const BUILT_WITH_TABLES: [&str; 3] = ["dependencies", "build-dependencies", "build_dependencies"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepositoryDirectory(String);

impl RepositoryDirectory {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn root() -> Self {
        Self(String::new())
    }

    pub fn manifest(&self) -> String {
        if self.0.is_empty() {
            return "Cargo.toml".to_owned();
        }
        format!("{}/Cargo.toml", self.0)
    }

    fn joined(&self, declared: &str) -> Result<Self, PathClosureRefusal> {
        let outside = || PathClosureRefusal::OutsideTheRepository {
            declared_in: self.clone(),
            path: declared.to_owned(),
        };
        let forward = declared.replace('\\', "/");
        if forward.starts_with('/') || forward.contains(':') {
            return Err(outside());
        }

        let mut segments: Vec<&str> = Vec::new();
        for segment in self.0.split('/').chain(forward.split('/')) {
            match segment {
                "" | "." => {}
                ".." => {
                    if segments.pop().is_none() {
                        return Err(outside());
                    }
                }
                _ => segments.push(segment),
            }
        }
        Ok(Self(segments.join("/")))
    }
}

impl Display for RepositoryDirectory {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildClosure(BTreeSet<RepositoryDirectory>);

impl BuildClosure {
    pub fn of(
        root_manifest: &str,
        member_directory: &str,
        lookup: impl Fn(&RepositoryDirectory) -> Option<String>,
    ) -> Result<Self, PathClosureRefusal> {
        walk(root_manifest, member_directory, &BUILT_WITH_TABLES, lookup).map(Self)
    }

    pub fn directories(&self) -> &BTreeSet<RepositoryDirectory> {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestClosure(BTreeSet<RepositoryDirectory>);

impl TestClosure {
    pub fn of(
        root_manifest: &str,
        member_directory: &str,
        lookup: impl Fn(&RepositoryDirectory) -> Option<String>,
    ) -> Result<Self, PathClosureRefusal> {
        walk(root_manifest, member_directory, &DEPENDENCY_TABLES, lookup).map(Self)
    }

    pub fn directories(&self) -> &BTreeSet<RepositoryDirectory> {
        &self.0
    }
}

fn walk(
    root_manifest: &str,
    member_directory: &str,
    tables: &[&str],
    lookup: impl Fn(&RepositoryDirectory) -> Option<String>,
) -> Result<BTreeSet<RepositoryDirectory>, PathClosureRefusal> {
    let root = RepositoryDirectory::root();
    let root_table = parsed(&root, root_manifest)?;
    let workspace_dependencies = root_table
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table);

    let member = root.joined(member_directory)?;
    let mut reached = BTreeSet::from([member.clone()]);
    let mut pending = vec![member.clone()];
    while let Some(directory) = pending.pop() {
        let text =
            lookup(&directory).ok_or_else(|| PathClosureRefusal::Unanswered(directory.clone()))?;
        let manifest = parsed(&directory, &text)?;

        for (name, specification) in dependencies_of(&manifest, tables) {
            let next = match specification.get("path").and_then(toml::Value::as_str) {
                Some(path) => directory.joined(path)?,
                None if opts_into_the_workspace(specification) => {
                    let Some(path) = workspace_dependencies
                        .and_then(|held| held.get(name))
                        .and_then(|held| held.get("path"))
                        .and_then(toml::Value::as_str)
                    else {
                        continue;
                    };
                    root.joined(path)?
                }
                None => continue,
            };
            if reached.insert(next.clone()) {
                pending.push(next);
            }
        }
    }

    reached.remove(&member);
    Ok(reached)
}

fn parsed(directory: &RepositoryDirectory, text: &str) -> Result<toml::Table, PathClosureRefusal> {
    toml::from_str(text).map_err(|error| PathClosureRefusal::Unparsable {
        directory: directory.clone(),
        error,
    })
}

fn dependencies_of<'a>(
    manifest: &'a toml::Table,
    tables: &'a [&str],
) -> impl Iterator<Item = (&'a String, &'a toml::Value)> {
    let per_target = manifest
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|targets| targets.values().filter_map(toml::Value::as_table));

    std::iter::once(manifest)
        .chain(per_target)
        .flat_map(move |sections| {
            tables
                .iter()
                .filter_map(|table| sections.get(*table).and_then(toml::Value::as_table))
        })
        .flatten()
}

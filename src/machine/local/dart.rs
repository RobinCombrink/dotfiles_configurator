use {
    crate::{
        configuration::{
            DartPackage, DartPackageName, DartReference, DartSource, GitCommit, GitReferenceName,
            GitRemoteUrl, RepositorySubdirectory,
        },
        machine::{
            CommandOutput,
            dart_reading::{DartDrift, DartLocations, DartReading},
        },
    },
    anyhow::{Context, Result, anyhow, bail},
    serde::{Deserialize, de::IgnoredAny},
    std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
    },
};

const LOCK_FILE: &str = "pubspec.lock";
const GIT_SOURCE: &str = "git";
const PATH_SOURCE: &str = "path";
const PEELED_SUFFIX: &str = "^{}";

pub fn read(
    locations: &DartLocations,
    package: &DartPackage,
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput>,
) -> Result<DartReading> {
    let DartSource::Git {
        url,
        path,
        reference,
    } = &package.source;

    let bundles = directories_in(&locations.bundles_of(&package.name))?;
    let bundle = match bundles.as_slice() {
        [] => return Ok(DartReading::Drifted(DartDrift::NoBundle)),
        [bundle] => bundle,
        several => {
            return Ok(DartReading::Drifted(DartDrift::SeveralBundles(
                several.len(),
            )));
        }
    };

    let lock = BundleLock::read(&bundle.join(LOCK_FILE))?;
    let installed = match lock.installed_from(&package.name, url, path)? {
        Installed::FromTheDeclaredSource(commit) => commit,
        Installed::FromElsewhere(source) => {
            return Ok(DartReading::Drifted(DartDrift::InstalledFromElsewhere(
                source,
            )));
        }
    };

    let Some(mirror) = mirror_of(&locations.mirrors(), url, git)? else {
        return Ok(DartReading::Drifted(DartDrift::NoMirror));
    };

    let tracked = match reference {
        DartReference::Exactly(commit) => {
            return Ok(compared_by_commit(installed, commit.clone()));
        }
        DartReference::Tracking(name) => name,
    };
    let desired = commit_origin_names(&mirror, tracked, url, git)?;
    if installed == desired || lock.depends_beyond(&package.name) {
        return Ok(compared_by_commit(installed, desired));
    }

    fetch(&mirror, tracked, url, git)?;
    let installed_tree = subtree(&mirror, &installed, path, git)?;
    let desired_tree = subtree(&mirror, &desired, path, git)?;
    match installed_tree == desired_tree {
        true => Ok(DartReading::Current),
        false => Ok(DartReading::Drifted(DartDrift::AtAnotherCommit {
            installed,
            desired,
        })),
    }
}

fn compared_by_commit(installed: GitCommit, desired: GitCommit) -> DartReading {
    match installed == desired {
        true => DartReading::Current,
        false => DartReading::Drifted(DartDrift::AtAnotherCommit { installed, desired }),
    }
}

fn directories_in(directory: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("Could not read {}", directory.display()));
        }
    };

    let mut bundles = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("Could not read {}", directory.display()))?;
        if entry.file_type()?.is_dir() {
            bundles.push(entry.path());
        }
    }
    bundles.sort();
    Ok(bundles)
}

#[derive(Debug, Deserialize)]
struct BundleLock {
    packages: BTreeMap<String, LockedPackage>,
}

#[derive(Debug, Deserialize)]
struct LockedPackage {
    source: String,
    description: LockedDescription,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LockedDescription {
    Git(LockedGitDescription),
    Other(IgnoredAny),
}

#[derive(Debug, Deserialize)]
struct LockedGitDescription {
    url: String,
    path: String,
    #[serde(rename = "resolved-ref")]
    resolved_reference: String,
}

enum Installed {
    FromTheDeclaredSource(GitCommit),
    FromElsewhere(String),
}

impl BundleLock {
    fn read(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        serde_saphyr::from_str(&text).map_err(|error| {
            anyhow!(
                "{} could not be read as a lock file: {error}",
                path.display()
            )
        })
    }

    fn installed_from(
        &self,
        name: &DartPackageName,
        url: &GitRemoteUrl,
        path: &RepositorySubdirectory,
    ) -> Result<Installed> {
        let Some(entry) = self.packages.get(name.as_ref()) else {
            bail!("the bundle's {LOCK_FILE} holds no entry for {name}");
        };
        let (LockedDescription::Git(description), GIT_SOURCE) =
            (&entry.description, entry.source.as_str())
        else {
            return Ok(Installed::FromElsewhere(format!(
                "a {} source rather than git",
                entry.source
            )));
        };
        if description.url != url.as_ref() || description.path != path.as_ref() {
            return Ok(Installed::FromElsewhere(format!(
                "{} in {}",
                description.path, description.url
            )));
        }

        GitCommit::try_from(description.resolved_reference.as_str())
            .map(Installed::FromTheDeclaredSource)
            .map_err(|reason| anyhow!("the bundle's {LOCK_FILE} names {reason}"))
    }

    fn depends_beyond(&self, name: &DartPackageName) -> bool {
        self.packages
            .iter()
            .filter(|(listed, _)| listed.as_str() != name.as_ref())
            .any(|(_, package)| package.source == GIT_SOURCE || package.source == PATH_SOURCE)
    }
}

fn mirror_of(
    mirrors: &Path,
    url: &GitRemoteUrl,
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput>,
) -> Result<Option<PathBuf>> {
    let mut matching = Vec::new();
    for candidate in directories_in(mirrors)? {
        let output = git(&candidate, &["config", "--get", "remote.origin.url"])?;
        if output.exited.succeeded() && output.standard_output.trim() == url.as_ref() {
            matching.push(candidate);
        }
    }

    match matching.len() {
        0 | 1 => Ok(matching.pop()),
        several => bail!(
            "{several} copies of {url} sit under {}, so which one pub reads cannot be told",
            mirrors.display()
        ),
    }
}

fn commit_origin_names(
    mirror: &Path,
    name: &GitReferenceName,
    url: &GitRemoteUrl,
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput>,
) -> Result<GitCommit> {
    // 2026-10-06: `git ls-remote origin v1` listed only `refs/tags/v1` at the annotated tag's own
    // object; naming `v1^{}` beside it added the `refs/tags/v1^{}` line carrying the commit. git
    // 2.52.0 on Windows 11.
    let peeled = format!("{name}{PEELED_SUFFIX}");
    let output = git(
        mirror,
        &["ls-remote", "origin", name.as_ref(), peeled.as_str()],
    )?;
    if !output.exited.succeeded() {
        bail!("{}", unauthenticated("ls-remote", mirror, url, &output));
    }

    let listed: Vec<(&str, &str)> = output
        .standard_output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    let commit = listed
        .iter()
        .find(|(_, reference)| reference.ends_with(PEELED_SUFFIX))
        .or_else(|| listed.first())
        .map(|(commit, _)| *commit);

    match commit {
        Some(commit) => GitCommit::try_from(commit)
            .map_err(|reason| anyhow!("origin answered for {name} with {reason}")),
        None => bail!("origin {url} holds no reference named {name}"),
    }
}

fn fetch(
    mirror: &Path,
    name: &GitReferenceName,
    url: &GitRemoteUrl,
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput>,
) -> Result<()> {
    let output = git(mirror, &["fetch", "origin", name.as_ref()])?;
    match output.exited.succeeded() {
        true => Ok(()),
        false => bail!("{}", unauthenticated("fetch", mirror, url, &output)),
    }
}

fn unauthenticated(
    operation: &str,
    mirror: &Path,
    url: &GitRemoteUrl,
    output: &CommandOutput,
) -> String {
    format!(
        "git {operation} in {} {}: {}. Check that the shared .gitconfig routes the pub cache to \
         an account that can read {url}",
        mirror.display(),
        output.exited,
        output.standard_error.trim()
    )
}

fn subtree(
    mirror: &Path,
    commit: &GitCommit,
    path: &RepositorySubdirectory,
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput>,
) -> Result<String> {
    let object = format!("{commit}:{path}");
    let output = git(mirror, &["rev-parse", object.as_str()])?;
    match output.exited.succeeded() {
        true => Ok(output.standard_output.trim().to_owned()),
        false => bail!(
            "git could not name the tree of {path} at {commit} in {}, {}: {}",
            mirror.display(),
            output.exited,
            output.standard_error.trim()
        ),
    }
}

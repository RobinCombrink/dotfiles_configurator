use {
    crate::{
        configuration::{
            DartPackage, DartPackageName, DartReference, DartSource, GitCommit, GitReferenceName,
            GitRemoteUrl, RepositorySubdirectory,
        },
        machine::{
            CommandOutput, FailedExit, FileReadFailure, ProgramStartFailure,
            dart_reading::{DartDrift, DartLocations, DartReadFailure, DartReading},
        },
    },
    serde::{Deserialize, de::IgnoredAny},
    std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::Arc,
    },
};

const LOCK_FILE: &str = "pubspec.lock";
const GIT_SOURCE: &str = "git";
const PATH_SOURCE: &str = "path";
const PEELED_SUFFIX: &str = "^{}";

pub fn read(
    locations: &DartLocations,
    package: &DartPackage,
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput, ProgramStartFailure>,
) -> Result<DartReading, DartReadFailure> {
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

    let lock_path = bundle.join(LOCK_FILE);
    let lock = BundleLock::read(&lock_path)?;
    let installed = match lock.installed_from(&lock_path, &package.name, url, path)? {
        Installed::FromTheDeclaredSource(commit) => commit,
        Installed::FromElsewhere(source) => {
            return Ok(DartReading::Drifted(DartDrift::InstalledFromElsewhere(
                source,
            )));
        }
    };

    let missing: Vec<String> = executables_of(bundle)?
        .into_iter()
        .filter(|executable| !locations.shim_of(executable).is_file())
        .collect();
    if !missing.is_empty() {
        return Ok(DartReading::Drifted(DartDrift::MissingShims(missing)));
    }

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

fn directories_in(directory: &Path) -> Result<Vec<PathBuf>, FileReadFailure> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(FileReadFailure::of(directory, error)),
    };

    let mut bundles = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| FileReadFailure::of(directory, error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| FileReadFailure::of(&entry.path(), error))?;
        if file_type.is_dir() {
            bundles.push(entry.path());
        }
    }
    bundles.sort();
    Ok(bundles)
}

const PUBSPEC_FILE: &str = "pubspec.yaml";
const COMPILED_SUFFIX: &str = std::env::consts::EXE_SUFFIX;

#[derive(Debug, Deserialize)]
struct BundlePubspec {
    #[serde(default)]
    executables: Option<BTreeMap<String, Option<String>>>,
}

// 2026-10-07: dhttpd 4.3.0's bundle held its own pubspec.yaml declaring `executables: dhttpd:`
// with no value, and `dart install` wrote the shim `install\bin\dhttpd.bat` for that key and
// compiled `bundle\bin\dhttpd.exe`; `dart install --help` says a package declaring no executables
// section has every `bin/*.dart` entry point installed. Dart 3.11.1 on Windows 11.
fn executables_of(bundle: &Path) -> Result<Vec<String>, DartReadFailure> {
    let path = bundle.join(PUBSPEC_FILE);
    let text = fs::read_to_string(&path).map_err(|error| FileReadFailure::of(&path, error))?;
    let pubspec: BundlePubspec =
        serde_saphyr::from_str(&text).map_err(|error| DartReadFailure::PubspecUnparsed {
            pubspec: path.clone(),
            error: Arc::new(error),
        })?;

    if let Some(declared) = pubspec.executables {
        return Ok(declared.into_keys().collect());
    }

    let compiled = bundle.join("bundle").join("bin");
    let entries = fs::read_dir(&compiled).map_err(|error| FileReadFailure::of(&compiled, error))?;
    let mut executables = Vec::new();
    for entry in entries {
        let name = entry
            .map_err(|error| FileReadFailure::of(&compiled, error))?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if let Some(executable) = name.strip_suffix(COMPILED_SUFFIX) {
            executables.push(executable.to_owned());
        }
    }
    executables.sort();
    Ok(executables)
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
    fn read(path: &Path) -> Result<Self, DartReadFailure> {
        let text = fs::read_to_string(path).map_err(|error| FileReadFailure::of(path, error))?;
        serde_saphyr::from_str(&text).map_err(|error| DartReadFailure::LockUnparsed {
            lock: path.to_path_buf(),
            error: Arc::new(error),
        })
    }

    fn installed_from(
        &self,
        lock: &Path,
        name: &DartPackageName,
        url: &GitRemoteUrl,
        path: &RepositorySubdirectory,
    ) -> Result<Installed, DartReadFailure> {
        let Some(entry) = self.packages.get(name.as_ref()) else {
            return Err(DartReadFailure::LockHoldsNoEntry {
                lock: lock.to_path_buf(),
                package: name.clone(),
            });
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
            .map_err(|_| DartReadFailure::LockNamesAnUnreadableCommit {
                lock: lock.to_path_buf(),
                listed: description.resolved_reference.clone(),
            })
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
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput, ProgramStartFailure>,
) -> Result<Option<PathBuf>, DartReadFailure> {
    let mut matching = Vec::new();
    for candidate in directories_in(mirrors)? {
        let output = git(&candidate, &["config", "--get", "remote.origin.url"])?;
        if output.exited.succeeded() && output.standard_output.trim() == url.as_ref() {
            matching.push(candidate);
        }
    }

    match matching.len() {
        0 | 1 => Ok(matching.pop()),
        several => Err(DartReadFailure::SeveralMirrors {
            count: several,
            url: url.clone(),
            mirrors: mirrors.to_path_buf(),
        }),
    }
}

fn commit_origin_names(
    mirror: &Path,
    name: &GitReferenceName,
    url: &GitRemoteUrl,
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput, ProgramStartFailure>,
) -> Result<GitCommit, DartReadFailure> {
    // 2026-10-06: `git ls-remote origin v1` listed only `refs/tags/v1` at the annotated tag's own
    // object; naming `v1^{}` beside it added the `refs/tags/v1^{}` line carrying the commit. git
    // 2.52.0 on Windows 11.
    let peeled = format!("{name}{PEELED_SUFFIX}");
    let output = git(
        mirror,
        &["ls-remote", "origin", name.as_ref(), peeled.as_str()],
    )?;
    if !output.exited.succeeded() {
        return Err(DartReadFailure::OriginUnlisted {
            mirror: mirror.to_path_buf(),
            url: url.clone(),
            failure: FailedExit::from(&output),
        });
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
        Some(commit) => GitCommit::try_from(commit).map_err(|_| {
            DartReadFailure::OriginAnsweredAnUnreadableCommit {
                reference: name.clone(),
                listed: commit.to_owned(),
            }
        }),
        None => Err(DartReadFailure::OriginHoldsNoReference {
            url: url.clone(),
            reference: name.clone(),
        }),
    }
}

fn fetch(
    mirror: &Path,
    name: &GitReferenceName,
    url: &GitRemoteUrl,
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput, ProgramStartFailure>,
) -> Result<(), DartReadFailure> {
    let output = git(mirror, &["fetch", "origin", name.as_ref()])?;
    match output.exited.succeeded() {
        true => Ok(()),
        false => Err(DartReadFailure::OriginUnfetched {
            mirror: mirror.to_path_buf(),
            url: url.clone(),
            failure: FailedExit::from(&output),
        }),
    }
}

fn subtree(
    mirror: &Path,
    commit: &GitCommit,
    path: &RepositorySubdirectory,
    git: &impl Fn(&Path, &[&str]) -> Result<CommandOutput, ProgramStartFailure>,
) -> Result<String, DartReadFailure> {
    let object = format!("{commit}:{path}");
    let output = git(mirror, &["rev-parse", object.as_str()])?;
    match output.exited.succeeded() {
        true => Ok(output.standard_output.trim().to_owned()),
        false => Err(DartReadFailure::TreeUnnamed {
            mirror: mirror.to_path_buf(),
            commit: commit.clone(),
            path: path.clone(),
            failure: FailedExit::from(&output),
        }),
    }
}

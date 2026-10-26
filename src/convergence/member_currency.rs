use {
    crate::{
        configuration::{BinaryName, CrateName, path_folding},
        convergence::{Assessment, Finding},
        machine::{
            CommandOutput, ReadMachine,
            environment_reading::SearchPathReading,
            workspace_reading::{Fingerprint, Revision, WorkspaceReading},
        },
    },
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt::Display,
        path::{Path, PathBuf},
    },
};

const VERSION_ARGUMENT: &str = "--version";
const UNRECORDED_BUILD: &str = "unrecorded";
const COMMIT_LENGTH: usize = 40;

// ADR 0040
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnCopy {
    Absent,
    Current,
    Stale(Staleness),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ours,
    Another(PathBuf),
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Staleness {
    ContentDiffers(String),
    NoSuchMemberAt(Revision),
    UnreadableCommit { commit: Revision, reason: String },
    Unrecorded,
    Unreadable(String),
}

impl Display for Staleness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Staleness::ContentDiffers(difference) => formatter.write_str(difference),
            Staleness::NoSuchMemberAt(commit) => {
                write!(
                    formatter,
                    "it reports {commit}, where the workspace held no such member"
                )
            }
            Staleness::UnreadableCommit { commit, reason } => write!(
                formatter,
                "it reports {commit}, which this clone cannot answer for: {reason}"
            ),
            Staleness::Unrecorded => {
                formatter.write_str("it reports a build that recorded no commit")
            }
            Staleness::Unreadable(reason) => formatter.write_str(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stamp {
    Commit(Revision),
    Unrecorded,
    Unreadable(String),
}

// 2026-10-05: `stop-gate --version` and `sweep --version` print `<name> <commit>` with the whole
// forty-character commit and exit 0, and a binary built without the stamp prints its package
// version instead, as `committed --version` printing `committed 1.1.11` does. Windows 11.
fn stamp_reported(binary: &BinaryName, output: &CommandOutput) -> Stamp {
    if !output.exited.succeeded() {
        return Stamp::Unreadable(format!(
            "`{binary} {VERSION_ARGUMENT}` failed, {}: {}",
            output.exited,
            output.standard_error.trim()
        ));
    }

    let printed = output.standard_output.trim();
    let reported = printed
        .split_once(' ')
        .filter(|(name, _)| *name == binary.as_ref())
        .map(|(_, build)| build);
    match reported {
        Some(UNRECORDED_BUILD) => Stamp::Unrecorded,
        Some(build) if is_a_commit(build) => Stamp::Commit(Revision::from(build)),
        Some(_) | None => Stamp::Unreadable(format!(
            "it reports \"{printed}\" rather than its name and the commit it was built from"
        )),
    }
}

fn is_a_commit(build: &str) -> bool {
    build.len() == COMMIT_LENGTH
        && build
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
}

fn stamp_of(binary: &BinaryName, machine: &impl ReadMachine) -> Option<Stamp> {
    let own_copy = machine.cargo_binaries_directory().join(binary.file_name());
    if !machine.path_exists(&own_copy) {
        return None;
    }

    Some(
        match machine.report_version(&own_copy, &[VERSION_ARGUMENT.to_owned()]) {
            Ok(output) => stamp_reported(binary, &output),
            Err(error) => Stamp::Unreadable(format!("it could not be run: {error:#}")),
        },
    )
}

pub type OwnCopies = BTreeMap<CrateName, Vec<(BinaryName, OwnCopy)>>;

pub fn own_copies_of(
    reading: &WorkspaceReading,
    repository_path: &Path,
    machine: &impl ReadMachine,
) -> OwnCopies {
    let stamps: BTreeMap<CrateName, Vec<(BinaryName, Option<Stamp>)>> = reading
        .members
        .iter()
        .map(|(crate_name, member)| {
            let stamps = member
                .binaries
                .iter()
                .map(|binary| (binary.clone(), stamp_of(binary, machine)))
                .collect();
            (crate_name.clone(), stamps)
        })
        .collect();

    let stamped_commits: BTreeSet<&Revision> = stamps
        .values()
        .flatten()
        .filter_map(|(_, stamp)| match stamp {
            Some(Stamp::Commit(commit)) if *commit != reading.revision => Some(commit),
            Some(_) | None => None,
        })
        .collect();
    let builds: BTreeMap<Revision, Result<BTreeMap<CrateName, Fingerprint>, String>> =
        stamped_commits
            .into_iter()
            .map(|commit| {
                let members = machine
                    .read_cargo_workspace_at(repository_path, commit)
                    .map_err(|error| format!("{error:#}"));
                (commit.clone(), members)
            })
            .collect();

    stamps
        .into_iter()
        .map(|(crate_name, stamps)| {
            let desired = &reading.members[&crate_name].desired;
            let own_copies = stamps
                .into_iter()
                .map(|(binary, stamp)| {
                    let own_copy = match stamp {
                        None => OwnCopy::Absent,
                        Some(Stamp::Commit(commit)) if commit == reading.revision => {
                            OwnCopy::Current
                        }
                        Some(Stamp::Commit(commit)) => {
                            built_at(&commit, &crate_name, desired, &builds)
                        }
                        Some(Stamp::Unrecorded) => OwnCopy::Stale(Staleness::Unrecorded),
                        Some(Stamp::Unreadable(reason)) => {
                            OwnCopy::Stale(Staleness::Unreadable(reason))
                        }
                    };
                    (binary, own_copy)
                })
                .collect();
            (crate_name, own_copies)
        })
        .collect()
}

fn built_at(
    commit: &Revision,
    crate_name: &CrateName,
    desired: &Fingerprint,
    builds: &BTreeMap<Revision, Result<BTreeMap<CrateName, Fingerprint>, String>>,
) -> OwnCopy {
    let members = match &builds[commit] {
        Ok(members) => members,
        Err(reason) => {
            return OwnCopy::Stale(Staleness::UnreadableCommit {
                commit: commit.clone(),
                reason: reason.clone(),
            });
        }
    };

    match members.get(crate_name) {
        None => OwnCopy::Stale(Staleness::NoSuchMemberAt(commit.clone())),
        Some(built) => match desired.difference_from(built) {
            None => OwnCopy::Current,
            Some(difference) => OwnCopy::Stale(Staleness::ContentDiffers(difference)),
        },
    }
}

pub fn resolution_of(
    binary: &BinaryName,
    search_path: &SearchPathReading,
    machine: &impl ReadMachine,
) -> Resolution {
    let own_copy = machine.cargo_binaries_directory().join(binary.file_name());
    match search_path.resolving(&binary.file_name(), |candidate| {
        machine.path_exists(candidate)
    }) {
        None => Resolution::Nothing,
        Some(resolved)
            if path_folding::comparable(&resolved) == path_folding::comparable(&own_copy) =>
        {
            Resolution::Ours
        }
        Some(resolved) => Resolution::Another(resolved),
    }
}

pub fn judged(binaries: impl IntoIterator<Item = (BinaryName, OwnCopy, Resolution)>) -> Assessment {
    let mut installs = Vec::new();
    let mut findings = Vec::new();
    for (binary, own_copy, resolution) in binaries {
        match (own_copy, resolution) {
            (OwnCopy::Current, Resolution::Ours) => {}
            (OwnCopy::Current, Resolution::Another(path)) => {
                findings.push(format!("{binary} is shadowed by {}", path.display()));
            }
            (OwnCopy::Current, Resolution::Nothing) => {
                findings.push(format!("nothing on PATH resolves {binary}"));
            }
            (OwnCopy::Absent, Resolution::Ours | Resolution::Nothing) => {
                installs.push(format!("{binary} is absent"));
            }
            (OwnCopy::Absent, Resolution::Another(path)) => {
                installs.push(format!(
                    "{binary} is absent, and is shadowed by {}",
                    path.display()
                ));
            }
            (OwnCopy::Stale(staleness), Resolution::Ours | Resolution::Nothing) => {
                installs.push(format!("{binary} is stale: {staleness}"));
            }
            (OwnCopy::Stale(staleness), Resolution::Another(path)) => {
                installs.push(format!(
                    "{binary} is stale: {staleness}, and is shadowed by {}",
                    path.display()
                ));
            }
        }
    }

    if !installs.is_empty() {
        return Assessment::Drifted(installs.join("; ").into());
    }
    if !findings.is_empty() {
        return Assessment::Found(Finding::from(findings.join("; ")));
    }
    Assessment::Converged
}

#[cfg(test)]
mod tests;

use {
    crate::{
        configuration::{BinaryName, GitHubRepository, Notice},
        convergence::{
            member_currency::{Stamp, stamp_at},
            withdrawal::{Withdrawal, claimed_binary_files},
        },
        machine::{
            INCOMING_SUFFIX, PARTIAL_DOWNLOAD_SUFFIX, ReadMachine, SUPERSEDED_SUFFIX,
            workspace_reading::{Revision, WorkspaceReadFailure},
        },
    },
    std::{
        collections::BTreeSet,
        path::{Path, PathBuf},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attribution {
    Attributed {
        workspace: GitHubRepository,
        commit: Revision,
    },
    NotInAnyClone {
        commit: Revision,
    },
    CloneUnreadable,
    Unstamped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredClone {
    Read {
        workspace: GitHubRepository,
        clone_directory: PathBuf,
    },
    Unread,
}

pub(crate) fn attribution_of(
    stamp: &Stamp,
    clones: &[DeclaredClone],
    resolve_commit: impl Fn(&Path, &Revision) -> Result<(), WorkspaceReadFailure>,
) -> Attribution {
    let Stamp::Commit(commit) = stamp else {
        return Attribution::Unstamped;
    };
    if clones.contains(&DeclaredClone::Unread) {
        return Attribution::CloneUnreadable;
    }

    let mut a_lookup_failed = false;
    for clone in clones {
        let DeclaredClone::Read {
            workspace,
            clone_directory,
        } = clone
        else {
            continue;
        };
        match resolve_commit(clone_directory, commit) {
            Ok(()) => {
                return Attribution::Attributed {
                    workspace: workspace.clone(),
                    commit: commit.clone(),
                };
            }
            Err(WorkspaceReadFailure::RevisionNotInClone { .. }) => {}
            Err(_) => a_lookup_failed = true,
        }
    }

    match a_lookup_failed {
        true => Attribution::CloneUnreadable,
        false => Attribution::NotInAnyClone {
            commit: commit.clone(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnclaimedBinary {
    pub binary: BinaryName,
    pub path: PathBuf,
    pub attribution: Attribution,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Decided {
    pub withdrawals: Vec<Withdrawal>,
    pub notices: Vec<Notice>,
}

pub fn decided(unclaimed: Vec<UnclaimedBinary>, declared: &BTreeSet<BinaryName>) -> Decided {
    if unclaimed
        .iter()
        .any(|binary| binary.attribution == Attribution::CloneUnreadable)
    {
        return Decided::default();
    }

    let mut decided = Decided::default();
    for UnclaimedBinary {
        binary,
        path,
        attribution,
    } in unclaimed
    {
        if declared.contains(&binary) {
            continue;
        }
        match attribution {
            Attribution::Attributed { workspace, commit } => {
                decided.withdrawals.push(Withdrawal::Remove {
                    binary,
                    workspace,
                    commit,
                });
            }
            Attribution::NotInAnyClone { commit } => {
                decided.notices.push(Notice::UnattributedBinary {
                    path,
                    binary,
                    commit: commit.to_string(),
                });
            }
            Attribution::CloneUnreadable | Attribution::Unstamped => {}
        }
    }
    decided
}

pub fn binary_named_by(file_name: &str) -> Option<BinaryName> {
    let left_by_a_write = [SUPERSEDED_SUFFIX, INCOMING_SUFFIX, PARTIAL_DOWNLOAD_SUFFIX]
        .iter()
        .any(|suffix| file_name.ends_with(suffix));
    if left_by_a_write {
        return None;
    }

    file_name
        .strip_suffix(std::env::consts::EXE_SUFFIX)
        .filter(|stem| !stem.is_empty())
        .map(BinaryName::from)
}

pub fn unclaimed_binaries(
    listing: &str,
    clones: &[DeclaredClone],
    declared: &BTreeSet<BinaryName>,
    machine: &impl ReadMachine,
) -> Vec<UnclaimedBinary> {
    let claimed = claimed_binary_files(listing);
    let Ok(entries) = machine.directory_entries(&machine.cargo_binaries_directory()) else {
        return Vec::new();
    };

    entries
        .into_iter()
        .filter_map(|path| {
            let file_name = path.file_name()?.to_str()?;
            if claimed.contains(file_name) {
                return None;
            }
            let binary = binary_named_by(file_name)?;
            if declared.contains(&binary) {
                return None;
            }
            let attribution = attribution_of(
                &stamp_at(&binary, &path, machine),
                clones,
                |clone, commit| machine.resolve_commit(clone, commit),
            );
            Some(UnclaimedBinary {
                binary,
                path,
                attribution,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;

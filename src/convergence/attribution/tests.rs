use {
    super::*,
    crate::{
        configuration::{RepositoryName, RepositoryOwner},
        convergence::member_currency::UnreadableStamp,
    },
    std::sync::Arc,
};

const COMMIT: &str = "2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c";

fn repository(name: &str) -> GitHubRepository {
    GitHubRepository {
        owner: RepositoryOwner::from("Alice"),
        repository: RepositoryName::from(name),
    }
}

fn clone_of(name: &str) -> DeclaredClone {
    DeclaredClone::Read {
        workspace: repository(name),
        clone_directory: PathBuf::from("C:\\Repositories\\Alice").join(name),
    }
}

fn stamped() -> Stamp {
    Stamp::Commit(Revision::from(COMMIT))
}

fn git_error() -> Arc<git2::Error> {
    Arc::new(git2::Error::from_str("refused"))
}

fn not_in_this_clone(revision: &Revision) -> WorkspaceReadFailure {
    WorkspaceReadFailure::RevisionNotInClone {
        revision: revision.clone(),
        error: git_error(),
    }
}

fn holding_the_commit_only_in(
    name: &str,
) -> impl Fn(&Path, &Revision) -> Result<(), WorkspaceReadFailure> {
    let holder = PathBuf::from("C:\\Repositories\\Alice").join(name);
    move |clone, revision| match clone == holder {
        true => Ok(()),
        false => Err(not_in_this_clone(revision)),
    }
}

#[test]
fn a_stamp_whose_commit_a_declared_clone_holds_is_attributed_to_that_workspace() {
    let attribution = attribution_of(
        &stamped(),
        &[clone_of("dotfiles"), clone_of("tools")],
        holding_the_commit_only_in("tools"),
    );

    assert_eq!(
        attribution,
        Attribution::Attributed {
            workspace: repository("tools"),
            commit: Revision::from(COMMIT),
        }
    );
}

#[test]
fn a_stamp_whose_commit_no_declared_clone_holds_is_in_no_clone() {
    let attribution = attribution_of(
        &stamped(),
        &[clone_of("dotfiles"), clone_of("tools")],
        holding_the_commit_only_in("elsewhere"),
    );

    assert_eq!(
        attribution,
        Attribution::NotInAnyClone {
            commit: Revision::from(COMMIT),
        }
    );
}

#[test]
fn a_clone_that_cannot_be_opened_to_look_the_commit_up_decides_no_attribution() {
    let attribution = attribution_of(&stamped(), &[clone_of("dotfiles")], |clone, _| {
        Err(WorkspaceReadFailure::CloneUnopened {
            clone: clone.to_path_buf(),
            error: git_error(),
        })
    });

    assert_eq!(attribution, Attribution::CloneUnreadable);
}

#[test]
fn a_declared_clone_whose_workspace_could_not_be_read_decides_no_attribution() {
    let attribution = attribution_of(
        &stamped(),
        &[clone_of("dotfiles"), DeclaredClone::Unread],
        holding_the_commit_only_in("dotfiles"),
    );

    assert_eq!(attribution, Attribution::CloneUnreadable);
}

#[test]
fn a_binary_reporting_something_other_than_its_name_and_a_commit_is_unstamped() {
    let printed = Stamp::Unreadable(UnreadableStamp::NamesNoCommit {
        printed: "rustup 1.28.2 (e4f3ad6f8 2025-04-28)".to_owned(),
    });

    let attribution = attribution_of(
        &printed,
        &[clone_of("dotfiles")],
        holding_the_commit_only_in("dotfiles"),
    );

    assert_eq!(attribution, Attribution::Unstamped);
}

#[test]
fn a_binary_built_without_recording_its_commit_is_unstamped() {
    let attribution = attribution_of(
        &Stamp::Unrecorded,
        &[clone_of("dotfiles")],
        holding_the_commit_only_in("dotfiles"),
    );

    assert_eq!(attribution, Attribution::Unstamped);
}

fn unclaimed(binary: &str, attribution: Attribution) -> UnclaimedBinary {
    UnclaimedBinary {
        binary: BinaryName::from(binary),
        path: PathBuf::from("C:\\Users\\Alice\\.cargo\\bin").join(format!("{binary}.exe")),
        attribution,
    }
}

fn attributed() -> Attribution {
    Attribution::Attributed {
        workspace: repository("dotfiles"),
        commit: Revision::from(COMMIT),
    }
}

fn in_no_clone() -> Attribution {
    Attribution::NotInAnyClone {
        commit: Revision::from(COMMIT),
    }
}

fn declaring(binaries: &[&str]) -> BTreeSet<BinaryName> {
    binaries
        .iter()
        .map(|binary| BinaryName::from(*binary))
        .collect()
}

#[test]
fn an_attributed_binary_no_member_declares_is_withdrawn() {
    let decided = decided(
        vec![unclaimed("session-census", attributed())],
        &declaring(&["stop-gate"]),
    );

    assert_eq!(
        decided,
        Decided {
            withdrawals: vec![Withdrawal::Remove {
                binary: BinaryName::from("session-census"),
                workspace: repository("dotfiles"),
                commit: Revision::from(COMMIT),
            }],
            notices: Vec::new(),
        }
    );
}

#[test]
fn an_attributed_binary_a_member_declares_is_kept() {
    let decided = decided(
        vec![unclaimed("session-census", attributed())],
        &declaring(&["session-census"]),
    );

    assert_eq!(decided, Decided::default());
}

#[test]
fn a_binary_in_no_clone_is_kept_and_named_with_its_stamp() {
    let decided = decided(
        vec![unclaimed("session-census", in_no_clone())],
        &declaring(&["stop-gate"]),
    );

    assert_eq!(
        decided,
        Decided {
            withdrawals: Vec::new(),
            notices: vec![Notice::UnattributedBinary {
                path: PathBuf::from("C:\\Users\\Alice\\.cargo\\bin\\session-census.exe"),
                binary: BinaryName::from("session-census"),
                commit: COMMIT.to_owned(),
            }],
        }
    );
}

#[test]
fn an_unreadable_clone_beside_any_binary_decides_nothing_for_every_binary() {
    let decided = decided(
        vec![
            unclaimed("session-census", attributed()),
            unclaimed("reach", in_no_clone()),
            unclaimed("sweep", Attribution::CloneUnreadable),
        ],
        &declaring(&["stop-gate"]),
    );

    assert_eq!(decided, Decided::default());
}

#[test]
fn an_unstamped_binary_is_left_alone_and_unnamed() {
    let decided = decided(
        vec![unclaimed("rustup", Attribution::Unstamped)],
        &declaring(&["stop-gate"]),
    );

    assert_eq!(decided, Decided::default());
}

#[test]
fn an_image_a_swap_left_beside_a_binary_is_not_itself_a_binary() {
    let installed = BinaryName::from("stop-gate").file_name();
    let named: Vec<Option<BinaryName>> = [
        format!("{installed}{SUPERSEDED_SUFFIX}"),
        format!("{installed}{INCOMING_SUFFIX}"),
        installed.clone(),
    ]
    .iter()
    .map(|file_name| binary_named_by(file_name))
    .collect();

    assert_eq!(named, vec![None, None, Some(BinaryName::from("stop-gate"))]);
}

use {
    super::*,
    crate::{
        configuration::{RepositoryName, RepositoryOwner},
        machine::workspace_reading::{Fingerprint, MemberReading, ObjectHash, Revision},
    },
    std::collections::{BTreeMap, BTreeSet},
};

const EARLIER: &str = "0c532c68bd883d1e6b002ad5bb1f40aa2ed70f66";
const LATER: &str = "e54928664eb4222b2ea79c7ef56126c5c58c7e01";

fn file(binary: &str) -> String {
    BinaryName::from(binary).file_name()
}

fn listed(header: &str, binaries: &[&str]) -> String {
    let mut lines = format!("{header}\n");
    for binary in binaries {
        lines.push_str(&format!("    {}\n", file(binary)));
    }
    lines
}

fn listing_of_two_disjoint_records_of_one_member() -> String {
    [
        listed("committed v1.1.11:", &["committed"]),
        listed(
            &format!(
                "session-mining v0.1.0 \
                 (https://Alice@github.com/Alice/dotfiles?rev={EARLIER}#0c532c68):"
            ),
            &["session-census", "reach"],
        ),
        listed(
            &format!(
                "session-mining v0.1.0 \
                 (https://Alice@github.com/Alice/dotfiles?rev={LATER}#e5492866):"
            ),
            &["sweep", "tool-use-statistics"],
        ),
        listed(
            "stop-gate v0.1.0 (C:\\Repositories\\Alice\\dotfiles\\tools\\stop-gate):",
            &["stop-gate"],
        ),
    ]
    .concat()
}

fn dotfiles() -> GitHubRepository {
    GitHubRepository {
        owner: RepositoryOwner::from("Alice"),
        repository: RepositoryName::from("dotfiles"),
    }
}

fn workspace_declaring(crate_name: &str, binaries: &[&str]) -> WorkspaceReading {
    WorkspaceReading {
        revision: Revision::from(LATER),
        members: BTreeMap::from([(
            CrateName::from(crate_name),
            MemberReading {
                desired: Fingerprint {
                    crate_subtree: ObjectHash::from("aaa"),
                    workspace_bindings: BTreeMap::new(),
                    lock_closure: ObjectHash::from("ccc"),
                    dependency_subtrees: BTreeMap::new(),
                },
                binaries: binaries
                    .iter()
                    .map(|name| BinaryName::from(*name))
                    .collect(),
            },
        )]),
    }
}

fn withdrawn_binaries(withdrawals: &[Withdrawal]) -> Vec<String> {
    withdrawals
        .iter()
        .map(|Withdrawal::Remove { binary, .. }| binary.to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[test]
fn only_git_records_are_read_each_with_the_binaries_listed_beneath_it() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());

    let read: Vec<(String, Vec<String>)> = records
        .iter()
        .map(|record| (record.crate_name.to_string(), record.binary_files.clone()))
        .collect();
    assert_eq!(
        read,
        vec![
            (
                "session-mining".to_owned(),
                vec![file("session-census"), file("reach")]
            ),
            (
                "session-mining".to_owned(),
                vec![file("sweep"), file("tool-use-statistics")]
            ),
        ]
    );
}

#[test]
fn a_record_is_specified_by_its_whole_source_so_that_two_of_one_package_are_told_apart() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());

    assert_eq!(
        records[0].specification(),
        format!("git+https://Alice@github.com/Alice/dotfiles?rev={EARLIER}#session-mining@0.1.0")
    );
}

#[test]
fn the_binaries_an_earlier_record_of_a_member_holds_that_it_no_longer_declares_are_withdrawn() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());
    let workspace = workspace_declaring(
        "session-mining",
        &["sweep", "tool-use-statistics", "session-census"],
    );

    let withdrawals = withdrawals([(&dotfiles(), &workspace)], &records);

    assert_eq!(withdrawn_binaries(&withdrawals), vec!["reach"]);
}

#[test]
fn a_record_installed_from_another_repository_is_never_withdrawn() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());
    let elsewhere = GitHubRepository {
        owner: RepositoryOwner::from("Bob"),
        repository: RepositoryName::from("dotfiles"),
    };

    let withdrawals = withdrawals(
        [(
            &elsewhere,
            &workspace_declaring("session-mining", &["sweep"]),
        )],
        &records,
    );

    assert!(withdrawals.is_empty(), "{withdrawals:?}");
}

fn reasons(withdrawals: &[Withdrawal]) -> BTreeSet<RemovalReason> {
    withdrawals
        .iter()
        .map(|Withdrawal::Remove { reason, .. }| *reason)
        .collect()
}

#[test]
fn every_binary_of_a_record_whose_crate_the_workspace_no_longer_holds_is_withdrawn() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());

    let withdrawals = withdrawals(
        [(
            &dotfiles(),
            &workspace_declaring("stop-gate", &["stop-gate"]),
        )],
        &records,
    );

    assert_eq!(
        withdrawn_binaries(&withdrawals),
        vec!["reach", "session-census", "sweep", "tool-use-statistics"]
    );
}

#[test]
fn a_binary_whose_crate_the_workspace_no_longer_holds_is_withdrawn_as_a_member_gone() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());

    let withdrawals = withdrawals(
        [(
            &dotfiles(),
            &workspace_declaring("stop-gate", &["stop-gate"]),
        )],
        &records,
    );

    assert_eq!(
        reasons(&withdrawals),
        BTreeSet::from([RemovalReason::MemberGone])
    );
}

#[test]
fn a_binary_a_member_of_another_declared_workspace_declares_is_never_withdrawn() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());
    let tools = GitHubRepository {
        owner: RepositoryOwner::from("Alice"),
        repository: RepositoryName::from("tools"),
    };

    let withdrawals = withdrawals(
        [
            (
                &dotfiles(),
                &workspace_declaring("stop-gate", &["stop-gate"]),
            ),
            (&tools, &workspace_declaring("session-tools", &["reach"])),
        ],
        &records,
    );

    assert_eq!(
        withdrawn_binaries(&withdrawals),
        vec!["session-census", "sweep", "tool-use-statistics"]
    );
}

#[test]
fn a_binary_a_member_stops_declaring_that_another_member_declares_is_never_withdrawn() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());
    let mut workspace = workspace_declaring(
        "session-mining",
        &["sweep", "tool-use-statistics", "session-census"],
    );
    workspace
        .members
        .extend(workspace_declaring("session-tools", &["reach"]).members);

    let withdrawals = withdrawals([(&dotfiles(), &workspace)], &records);

    assert!(withdrawals.is_empty(), "{withdrawals:?}");
}

#[test]
fn a_binary_a_member_no_longer_declares_is_withdrawn_as_dropped() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());
    let workspace = workspace_declaring(
        "session-mining",
        &["sweep", "tool-use-statistics", "session-census"],
    );

    let withdrawals = withdrawals([(&dotfiles(), &workspace)], &records);

    assert_eq!(
        reasons(&withdrawals),
        BTreeSet::from([RemovalReason::BinaryDropped])
    );
}

#[test]
fn a_record_fetched_as_another_account_still_belongs_to_the_repository() {
    let listing = listed(
        &format!(
            "session-mining v0.1.0 \
             (https://Carol@github.com/alice/Dotfiles?rev={EARLIER}#0c532c68):"
        ),
        &["reach"],
    );

    let withdrawals = withdrawals(
        [(
            &dotfiles(),
            &workspace_declaring("session-mining", &["sweep"]),
        )],
        &install_records(&listing),
    );

    assert_eq!(withdrawn_binaries(&withdrawals), vec!["reach"]);
}

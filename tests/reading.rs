//! What a change set costs the sources it reads. Planning is otherwise covered by the scenarios
//! in `features/planning.feature`; a read count is not something a person using the tool observes,
//! so it is held here rather than in Gherkin.

#![allow(clippy::disallowed_macros)]

#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    dotfiles_configurator::{
        configuration::{
            Application, ArchiveEntry, AssetPattern, CargoPackage, CargoSource, CargoWorkspace,
            CrateName, DesiredState, GitHubRepository, MachineSettings, Package, ReleasedBinary,
            RepositoryName, RepositoryOwner, Resource, VersionWord, WingetPackage,
        },
        convergence::plan,
        machine::{
            ReadInvocation,
            release_reading::ReleaseReading,
            workspace_reading::{
                Fingerprint, MemberReading, ObjectHash, Revision, WorkspaceReading,
            },
        },
        reporting::RunReport,
        version::Version,
    },
    fake_machine::FakeMachine,
    std::{collections::BTreeSet, num::NonZeroUsize, path::PathBuf},
};

fn desired_state(resources: Vec<Resource>) -> DesiredState {
    DesiredState {
        resources,
        workspaces: Vec::new(),
        notices: Vec::new(),
        machine: MachineSettings {
            repositories_directory_path: PathBuf::from("/repositories"),
            github_username: "Alice".to_owned(),
            dotfiles_repository: GitHubRepository {
                owner: RepositoryOwner::from("Alice"),
                repository: RepositoryName::from("dotfiles"),
            },
        },
    }
}

fn winget_package(id: &str) -> Resource {
    Resource::Package(Package::Winget(WingetPackage { id: id.into() }))
}

fn cargo_package(crate_name: &str) -> Resource {
    Resource::Package(Package::Cargo(CargoPackage {
        crate_name: crate_name.into(),
        source: CargoSource::Registry,
    }))
}

#[tokio::test]
async fn many_packages_from_one_manager_ask_it_once() {
    let machine = FakeMachine::default();
    let desired_state = desired_state(vec![
        winget_package("Microsoft.PowerShell"),
        winget_package("Git.Git"),
        winget_package("Neovim.Neovim"),
    ]);

    plan(&desired_state, &machine, &RunReport::discarded())
        .await
        .unwrap();

    assert_eq!(
        machine.times_read(&ReadInvocation::WingetInstalledPackages),
        1
    );
}

#[tokio::test]
async fn each_manager_is_asked_once_when_several_are_declared_against() {
    let machine = FakeMachine::default();
    let desired_state = desired_state(vec![
        winget_package("Microsoft.PowerShell"),
        cargo_package("committed"),
        winget_package("Git.Git"),
        cargo_package("stop-gate"),
    ]);

    plan(&desired_state, &machine, &RunReport::discarded())
        .await
        .unwrap();

    assert_eq!(
        machine.times_read(&ReadInvocation::WingetInstalledPackages),
        1
    );
    assert_eq!(machine.times_read(&ReadInvocation::CargoInstalledCrates), 1);
}

#[tokio::test]
async fn a_manager_nothing_is_declared_against_is_never_asked() {
    let machine = FakeMachine::default();
    let desired_state = desired_state(vec![winget_package("Microsoft.PowerShell")]);

    plan(&desired_state, &machine, &RunReport::discarded())
        .await
        .unwrap();

    assert_eq!(machine.times_read(&ReadInvocation::CargoInstalledCrates), 0);
}

#[tokio::test]
async fn a_package_whose_manager_is_absent_leaves_that_manager_unasked() {
    let machine = FakeMachine::default();
    machine.remove_tool(dotfiles_configurator::machine::Tool::Winget);
    let desired_state = desired_state(vec![winget_package("Microsoft.PowerShell")]);

    plan(&desired_state, &machine, &RunReport::discarded())
        .await
        .unwrap();

    assert_eq!(
        machine.times_read(&ReadInvocation::WingetInstalledPackages),
        0
    );
}

fn workspace_holding(crate_names: &[&str]) -> WorkspaceReading {
    WorkspaceReading {
        revision: Revision::from("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c"),
        members: crate_names
            .iter()
            .map(|crate_name| {
                (
                    CrateName::from(*crate_name),
                    MemberReading {
                        desired: Fingerprint {
                            crate_subtree: ObjectHash::from(*crate_name),
                            workspace_manifest: ObjectHash::from("the workspace manifest"),
                            lockfile: ObjectHash::from("the lockfile"),
                        },
                        installed: None,
                        absent_binaries: BTreeSet::new(),
                    },
                )
            })
            .collect(),
    }
}

#[tokio::test]
async fn every_crate_in_one_workspace_opens_its_repository_once() {
    let machine = FakeMachine::default();
    machine.clone_dotfiles_repository();
    machine.hold_cargo_workspace(
        PathBuf::from("/repositories/dotfiles"),
        workspace_holding(&[
            "stop-gate",
            "ci-checks",
            "claude-workspace",
            "session-mining",
        ]),
    );
    let mut desired_state = desired_state(Vec::new());
    desired_state.workspaces = vec![CargoWorkspace {
        repository: GitHubRepository {
            owner: RepositoryOwner::from("Alice"),
            repository: RepositoryName::from("dotfiles"),
        },
    }];

    let change_set = plan(&desired_state, &machine, &RunReport::discarded())
        .await
        .unwrap();

    assert_eq!(change_set.changes.len(), 4);
    assert_eq!(machine.cargo_workspace_reads().len(), 1);
}

#[tokio::test]
async fn a_configuration_declaring_no_workspace_never_opens_a_repository() {
    let machine = FakeMachine::default();
    let desired_state = desired_state(vec![cargo_package("committed")]);

    plan(&desired_state, &machine, &RunReport::discarded())
        .await
        .unwrap();

    assert!(machine.cargo_workspace_reads().is_empty());
}

fn ripgrep() -> GitHubRepository {
    GitHubRepository {
        owner: RepositoryOwner::from("BurntSushi"),
        repository: RepositoryName::from("ripgrep"),
    }
}

fn released_binary(entry: &str) -> Resource {
    Resource::Application(Application::ReleasedBinary(ReleasedBinary {
        repository: ripgrep(),
        asset: AssetPattern::EndsWith(".zip".to_owned()),
        entry: ArchiveEntry::try_from(entry.to_owned()).unwrap(),
        version_arguments: vec!["--version".to_owned()],
        version_word: VersionWord::from(NonZeroUsize::new(2).unwrap()),
    }))
}

#[tokio::test]
async fn several_binaries_out_of_one_repository_ask_it_for_its_release_once() {
    let machine = FakeMachine::default();
    machine.publish_release(
        ripgrep(),
        ReleaseReading {
            version: Version::try_from("15.1.0").unwrap(),
            assets: Vec::new(),
        },
    );
    let desired_state = desired_state(vec![
        released_binary("rg.exe"),
        released_binary("rg-imports.exe"),
    ]);

    plan(&desired_state, &machine, &RunReport::discarded())
        .await
        .unwrap();

    assert_eq!(machine.release_reads(&ripgrep()), 1);
}

#[tokio::test]
async fn a_configuration_declaring_no_released_binary_never_asks_for_a_release() {
    let machine = FakeMachine::default();
    let desired_state = desired_state(vec![winget_package("Microsoft.PowerShell")]);

    plan(&desired_state, &machine, &RunReport::discarded())
        .await
        .unwrap();

    assert_eq!(machine.release_reads(&ripgrep()), 0);
}

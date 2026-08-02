//! What a change set costs the sources it reads. Planning is otherwise covered by the scenarios
//! in `features/planning.feature`; a read count is not something a person using the tool observes,
//! so it is held here rather than in Gherkin.

#![allow(clippy::disallowed_macros)]

#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    dotfiles::{
        configuration::{
            CargoPackage, CargoSource, DesiredState, GitHubRepository, MachineSettings, Package,
            RepositoryName, RepositoryOwner, Resource, WingetPackage,
        },
        convergence::plan,
        machine::ReadInvocation,
    },
    fake_machine::FakeMachine,
    std::path::PathBuf,
};

fn desired_state(resources: Vec<Resource>) -> DesiredState {
    DesiredState {
        resources,
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

#[test]
fn many_packages_from_one_manager_ask_it_once() {
    let machine = FakeMachine::default();
    let desired_state = desired_state(vec![
        winget_package("Microsoft.PowerShell"),
        winget_package("Git.Git"),
        winget_package("Neovim.Neovim"),
    ]);

    plan(&desired_state, &machine);

    assert_eq!(
        machine.times_read(&ReadInvocation::WingetInstalledPackages),
        1
    );
}

#[test]
fn each_manager_is_asked_once_when_several_are_declared_against() {
    let machine = FakeMachine::default();
    let desired_state = desired_state(vec![
        winget_package("Microsoft.PowerShell"),
        cargo_package("committed"),
        winget_package("Git.Git"),
        cargo_package("stop-gate"),
    ]);

    plan(&desired_state, &machine);

    assert_eq!(
        machine.times_read(&ReadInvocation::WingetInstalledPackages),
        1
    );
    assert_eq!(machine.times_read(&ReadInvocation::CargoInstalledCrates), 1);
}

#[test]
fn a_manager_nothing_is_declared_against_is_never_asked() {
    let machine = FakeMachine::default();
    let desired_state = desired_state(vec![winget_package("Microsoft.PowerShell")]);

    plan(&desired_state, &machine);

    assert_eq!(machine.times_read(&ReadInvocation::CargoInstalledCrates), 0);
}

#[test]
fn a_package_whose_manager_is_absent_leaves_that_manager_unasked() {
    let machine = FakeMachine::default();
    machine.remove_tool(dotfiles::machine::Tool::Winget);
    let desired_state = desired_state(vec![winget_package("Microsoft.PowerShell")]);

    plan(&desired_state, &machine);

    assert_eq!(
        machine.times_read(&ReadInvocation::WingetInstalledPackages),
        0
    );
}

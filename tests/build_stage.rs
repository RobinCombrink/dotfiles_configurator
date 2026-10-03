#![allow(clippy::disallowed_macros)]

#[path = "common/declarations.rs"]
mod declarations;
#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    declarations::{Reporting, declaring, dotfiles_repository},
    dotfiles_configurator::{
        configuration::{
            CargoPackage, CargoSource, CargoWorkspace, CrateName, Package, Resource, WingetPackage,
        },
        confirmation::Operator,
        convergence::{ApplyOutcome, Enactment, apply::apply},
        currency::SelfReplacement,
        machine::workspace_reading::{
            Fingerprint, InstalledState, MemberReading, ObjectHash, Revision, WorkspaceReading,
        },
        reporting::RunKind,
    },
    fake_machine::{CargoCommand, FakeMachine, dotfiles_repository_path},
    std::collections::{BTreeMap, BTreeSet},
};

const REVISION: &str = "2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c";

fn content_held_now() -> Fingerprint {
    Fingerprint {
        crate_subtree: ObjectHash::from("what the workspace holds now"),
        workspace_manifest: ObjectHash::from("the workspace manifest"),
        lock_closure: ObjectHash::from("the lock closure"),
        dependency_subtrees: BTreeMap::new(),
    }
}

fn never_installed() -> MemberReading {
    MemberReading {
        desired: content_held_now(),
        installed: InstalledState::NotInstalled,
        absent_binaries: BTreeSet::new(),
    }
}

fn installed_from_what_the_workspace_holds_now() -> MemberReading {
    MemberReading {
        installed: InstalledState::At(content_held_now()),
        ..never_installed()
    }
}

fn machine_holding_the_workspace(members: &[(&str, MemberReading)]) -> FakeMachine {
    let machine = FakeMachine::default();
    machine.clone_dotfiles_repository();
    machine.hold_cargo_workspace(
        dotfiles_repository_path(),
        WorkspaceReading {
            revision: Revision::from(REVISION),
            members: members
                .iter()
                .map(|(name, reading)| (CrateName::from(*name), reading.clone()))
                .collect(),
        },
    );
    machine
}

async fn applying(resources: Vec<Resource>, machine: &FakeMachine) -> ApplyOutcome {
    let desired_state = declaring(
        resources,
        vec![CargoWorkspace {
            repository: dotfiles_repository(),
        }],
    );

    let enactment = apply(
        &desired_state,
        machine,
        Reporting::opening(RunKind::Apply).report(),
        &Operator::AnsweredInAdvance,
        &SelfReplacement::Available,
    )
    .await
    .unwrap();

    match enactment {
        Enactment::Enacted(outcome) => outcome,
        Enactment::Declined | Enactment::ReplacedItself => {
            panic!("the apply did not enact its change set")
        }
    }
}

fn built(members: &[&str]) -> CargoCommand {
    CargoCommand::Built {
        clone_directory: dotfiles_repository_path(),
        revision: Revision::from(REVISION),
        members: members.iter().map(|name| CrateName::from(*name)).collect(),
    }
}

fn installed(crate_name: &str) -> CargoCommand {
    CargoCommand::Installed(CrateName::from(crate_name))
}

#[tokio::test]
async fn the_changed_members_of_a_workspace_build_once_at_its_revision_before_any_of_them_installs()
{
    let machine = machine_holding_the_workspace(&[
        ("claude-session", never_installed()),
        ("stop-gate", installed_from_what_the_workspace_holds_now()),
        ("session-mining", never_installed()),
    ]);

    applying(Vec::new(), &machine).await;

    assert_eq!(
        machine.cargo_commands(),
        vec![
            built(&["claude-session", "session-mining"]),
            installed("claude-session"),
            installed("session-mining"),
        ]
    );
}

#[tokio::test]
async fn a_crate_from_outside_any_workspace_installs_without_a_build_before_it() {
    let machine = machine_holding_the_workspace(&[(
        "stop-gate",
        installed_from_what_the_workspace_holds_now(),
    )]);
    let ripgrep = Resource::Package(Package::Cargo(CargoPackage {
        crate_name: CrateName::from("ripgrep"),
        source: CargoSource::Registry { version: None },
    }));

    applying(vec![ripgrep], &machine).await;

    assert_eq!(machine.cargo_commands(), vec![installed("ripgrep")]);
}

#[tokio::test]
async fn a_member_a_pass_already_attempted_is_not_built_again_on_a_later_pass() {
    let machine = machine_holding_the_workspace(&[("session-mining", never_installed())]);
    machine.execute_binary_that_cannot_be_displaced("tool-use-statistics");
    let powershell = Resource::Package(Package::Winget(WingetPackage {
        id: "Microsoft.PowerShell".into(),
    }));

    let outcome = applying(vec![powershell], &machine).await;

    assert!(outcome.passes > 1, "{outcome}");
    assert_eq!(
        machine.cargo_commands(),
        vec![built(&["session-mining"]), installed("session-mining")]
    );
}

#[tokio::test]
async fn a_build_that_fails_leaves_each_member_to_build_in_its_own_install() {
    let machine = machine_holding_the_workspace(&[
        ("claude-session", never_installed()),
        ("session-mining", never_installed()),
    ]);
    machine.make_workspace_builds_fail();

    let outcome = applying(Vec::new(), &machine).await;

    assert!(outcome.failed.is_empty(), "{outcome}");
    assert_eq!(outcome.converged.len(), 2, "{outcome}");
}

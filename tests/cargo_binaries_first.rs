#![cfg(target_family = "windows")]
#![allow(clippy::disallowed_macros)]

#[path = "common/declarations.rs"]
mod declarations;
#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    declarations::{Reporting, declaring, dotfiles_repository},
    dotfiles_configurator::{
        configuration::{
            BinaryName, CargoPackage, CargoSource, CargoWorkspace, CrateName, EnvironmentVariable,
            Package, PackageCurrency, Resource, Variable, VariableName, VariableValue,
            WingetPackage,
        },
        confirmation::Operator,
        convergence::{
            ApplyOutcome, Assessment, ChangeSet, Enactment, Impediment, SourceReadings,
            apply::apply, assess, plan,
        },
        currency::SelfReplacement,
        desired_state::{ChangeSetResource, DesiredState},
        machine::{
            HeldReason, ReadMachine,
            workspace_reading::{
                Fingerprint, MemberReading, ObjectHash, Revision, WorkspaceReading,
            },
        },
        reporting::RunKind,
    },
    fake_machine::{FakeMachine, filesystem_root},
    std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
    },
};

fn cargo_package(crate_name: &str) -> Resource {
    Resource::Package(Package::Cargo(CargoPackage {
        crate_name: crate_name.into(),
        source: CargoSource::Registry {
            version: PackageCurrency::Latest,
        },
    }))
}

fn winget_package(id: &str) -> Resource {
    Resource::Package(Package::Winget(WingetPackage {
        id: id.into(),
        version: PackageCurrency::Latest,
    }))
}

fn editor_variable() -> Resource {
    Resource::EnvironmentVariable(EnvironmentVariable::Variable(Variable {
        name: VariableName::try_from("EDITOR").unwrap(),
        value: VariableValue::from("nvim"),
    }))
}

fn github_cli() -> PathBuf {
    filesystem_root().join("Program Files").join("GitHub CLI")
}

fn a_machine_path_without_cargo_binaries_first(machine: &FakeMachine) {
    machine.clear_the_search_path();
    machine.hold_user_search_path_entry(machine.binaries_directory());
    machine.hold_user_search_path_entry(machine.cargo_binaries_directory());
    machine.hold_machine_search_path_entry(github_cli());
}

async fn planned(desired_state: &DesiredState, machine: &FakeMachine) -> ChangeSet {
    plan(
        desired_state,
        machine,
        Reporting::opening(RunKind::Plan).report(),
    )
    .await
    .unwrap()
    .0
}

fn holds_cargo_binaries_first(change_set: &ChangeSet) -> bool {
    change_set
        .changes
        .iter()
        .map(|change| &change.resource)
        .chain(&change_set.converged)
        .chain(change_set.blocked.iter().map(|blocked| &blocked.resource))
        .any(|resource| *resource == ChangeSetResource::CargoBinariesFirst)
}

async fn assessed(machine: &FakeMachine) -> Assessment {
    let desired_state = declaring(vec![cargo_package("ripgrep")], Vec::new());
    let readings = SourceReadings::read_for(&desired_state, machine).await;

    assess(&ChangeSetResource::CargoBinariesFirst, machine, &readings)
}

#[tokio::test]
async fn a_declared_cargo_package_derives_cargo_binaries_first() {
    let machine = FakeMachine::default();

    let change_set = planned(
        &declaring(vec![cargo_package("ripgrep")], Vec::new()),
        &machine,
    )
    .await;

    assert!(holds_cargo_binaries_first(&change_set));
}

#[tokio::test]
async fn a_workspace_member_derives_cargo_binaries_first() {
    let machine = FakeMachine::default();
    machine.clone_dotfiles_repository();
    machine.hold_cargo_workspace(
        machine.dotfiles_repository_path().to_path_buf(),
        WorkspaceReading {
            revision: Revision::from("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c"),
            members: BTreeMap::from([(
                CrateName::from("gh-routed"),
                MemberReading {
                    desired: Fingerprint {
                        crate_subtree: ObjectHash::from("gh-routed"),
                        workspace_bindings: BTreeMap::new(),
                        lock_closure: ObjectHash::from("the lock closure"),
                        dependency_subtrees: BTreeMap::new(),
                    },
                    binaries: BTreeSet::from([BinaryName::from("gh")]),
                },
            )]),
        },
    );
    let desired_state = declaring(
        Vec::new(),
        vec![CargoWorkspace {
            repository: dotfiles_repository(),
        }],
    );

    let change_set = planned(&desired_state, &machine).await;

    assert!(holds_cargo_binaries_first(&change_set));
}

#[tokio::test]
async fn nothing_installing_into_the_cargo_binaries_directory_derives_nothing() {
    let machine = FakeMachine::default();
    a_machine_path_without_cargo_binaries_first(&machine);

    let change_set = planned(
        &declaring(
            vec![winget_package("Git.Git"), editor_variable()],
            Vec::new(),
        ),
        &machine,
    )
    .await;

    assert!(!holds_cargo_binaries_first(&change_set));
}

#[tokio::test]
async fn a_machine_path_starting_with_the_cargo_binaries_directory_is_converged() {
    let machine = FakeMachine::default();

    assert_eq!(assessed(&machine).await, Assessment::Converged);
}

#[tokio::test]
async fn a_machine_path_naming_the_cargo_binaries_directory_after_another_entry_is_a_change() {
    let machine = FakeMachine::default();
    machine.hold_machine_search_path_entry_first(github_cli());

    let Assessment::Drifted(_) = assessed(&machine).await else {
        panic!("a Machine PATH starting with GitHub CLI was not a change");
    };
}

#[tokio::test]
async fn a_machine_path_not_naming_the_cargo_binaries_directory_is_a_change() {
    let machine = FakeMachine::default();
    a_machine_path_without_cargo_binaries_first(&machine);

    let Assessment::Drifted(_) = assessed(&machine).await else {
        panic!("a Machine PATH without cargo's bin directory was not a change");
    };
}

#[tokio::test]
async fn an_unreadable_machine_path_is_blocked_as_unreadable() {
    let machine = FakeMachine::default();
    machine.make_the_machine_search_path_unreadable();

    let Assessment::Unassessable(Impediment::ActualStateUnreadable(_)) = assessed(&machine).await
    else {
        panic!("an unreadable Machine PATH was assessed");
    };
}

async fn applied(machine: &FakeMachine) -> ApplyOutcome {
    let desired_state = declaring(
        vec![cargo_package("ripgrep"), editor_variable()],
        Vec::new(),
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
        Enactment::Enacted(outcome) => *outcome,
        Enactment::Declined | Enactment::ReplacedItself => {
            panic!("the apply did not enact its change set")
        }
    }
}

fn converged_cargo_binaries_first(outcome: &ApplyOutcome) -> bool {
    outcome
        .converged
        .contains(&ChangeSetResource::CargoBinariesFirst)
}

#[tokio::test]
async fn an_unelevated_apply_puts_the_cargo_binaries_directory_first_through_elevation() {
    let machine = FakeMachine::default();
    a_machine_path_without_cargo_binaries_first(&machine);

    let outcome = applied(&machine).await;

    assert_eq!(
        (
            converged_cargo_binaries_first(&outcome),
            machine.elevation_prompts(),
            machine.machine_search_path(),
        ),
        (
            true,
            1,
            vec![machine.cargo_binaries_directory(), github_cli()]
        )
    );
}

#[tokio::test]
async fn an_elevated_apply_puts_the_cargo_binaries_directory_first_without_asking() {
    let machine = FakeMachine::default();
    a_machine_path_without_cargo_binaries_first(&machine);
    machine.start_the_apply_elevated();

    let outcome = applied(&machine).await;

    assert_eq!(
        (
            converged_cargo_binaries_first(&outcome),
            machine.elevation_prompts(),
            machine.machine_search_path(),
        ),
        (
            true,
            0,
            vec![machine.cargo_binaries_directory(), github_cli()]
        )
    );
}

#[tokio::test]
async fn a_declined_elevation_holds_cargo_binaries_first_and_lets_the_rest_converge() {
    let machine = FakeMachine::default();
    a_machine_path_without_cargo_binaries_first(&machine);
    machine.decline_elevation();

    let outcome = applied(&machine).await;

    let held: Vec<(&ChangeSetResource, &HeldReason)> = outcome
        .held
        .iter()
        .map(|held| (&held.resource, &held.reason))
        .collect();
    assert_eq!(
        (
            held,
            machine.environment_variable(&VariableName::try_from("EDITOR").unwrap()),
            machine.machine_search_path(),
        ),
        (
            vec![(
                &ChangeSetResource::CargoBinariesFirst,
                &HeldReason::ElevationDeclined
            )],
            Some(VariableValue::from("nvim")),
            vec![github_cli()],
        )
    );
}

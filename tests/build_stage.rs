#![allow(clippy::disallowed_macros)]

#[path = "common/declarations.rs"]
mod declarations;
#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    declarations::{Reporting, declaring, dotfiles_repository},
    dotfiles_configurator::{
        configuration::{
            BinaryName, CargoPackage, CargoSource, CargoWorkspace, CrateName, Package,
            PackageCurrency, Resource, WingetPackage,
        },
        confirmation::Operator,
        convergence::{ApplyOutcome, Enactment, apply::apply},
        currency::SelfReplacement,
        machine::workspace_reading::{
            Fingerprint, MemberReading, ObjectHash, Revision, WorkspaceReading,
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
        workspace_bindings: BTreeMap::new(),
        lock_closure: ObjectHash::from("the lock closure"),
        dependency_subtrees: BTreeMap::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Installed {
    Never,
    FromWhatTheWorkspaceHoldsNow,
}

fn never_installed() -> Installed {
    Installed::Never
}

fn installed_from_what_the_workspace_holds_now() -> Installed {
    Installed::FromWhatTheWorkspaceHoldsNow
}

fn machine_holding_the_workspace(members: &[(&str, Installed)]) -> FakeMachine {
    let machine = FakeMachine::default();
    machine.clone_dotfiles_repository();
    machine.hold_cargo_workspace(
        dotfiles_repository_path(),
        WorkspaceReading {
            revision: Revision::from(REVISION),
            members: members
                .iter()
                .map(|(name, _)| {
                    (
                        CrateName::from(*name),
                        MemberReading {
                            desired: content_held_now(),
                            binaries: BTreeSet::from([BinaryName::from(*name)]),
                        },
                    )
                })
                .collect(),
        },
    );
    for (name, installed) in members {
        if *installed == Installed::FromWhatTheWorkspaceHoldsNow {
            machine.hold_cargo_binary(name, format!("{name} {REVISION}\n"));
        }
    }
    machine
}

async fn applying(resources: Vec<Resource>, machine: &FakeMachine) -> ApplyOutcome {
    applying_reported_to(resources, machine, &Reporting::opening(RunKind::Apply)).await
}

async fn applying_reported_to(
    resources: Vec<Resource>,
    machine: &FakeMachine,
    reporting: &Reporting,
) -> ApplyOutcome {
    let desired_state = declaring(
        resources,
        vec![CargoWorkspace {
            repository: dotfiles_repository(),
        }],
    );

    let enactment = apply(
        &desired_state,
        machine,
        reporting.report(),
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

fn builds_and_installs(machine: &FakeMachine) -> Vec<CargoCommand> {
    machine
        .cargo_commands()
        .into_iter()
        .filter(|command| !matches!(command, CargoCommand::ReapedBuildsOtherThan(_)))
        .collect()
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
        builds_and_installs(&machine),
        vec![
            built(&["claude-session", "session-mining"]),
            installed("claude-session"),
            installed("session-mining"),
        ]
    );
}

#[tokio::test]
async fn the_builds_of_other_revisions_are_reaped_once_before_the_stage_rather_than_per_install() {
    let machine = machine_holding_the_workspace(&[
        ("claude-session", never_installed()),
        ("session-mining", never_installed()),
    ]);

    applying(Vec::new(), &machine).await;

    let commands = machine.cargo_commands();
    let reaped: Vec<&CargoCommand> = commands
        .iter()
        .filter(|command| matches!(command, CargoCommand::ReapedBuildsOtherThan(_)))
        .collect();
    assert_eq!(
        reaped,
        vec![&CargoCommand::ReapedBuildsOtherThan(BTreeSet::from([
            Revision::from(REVISION)
        ]))]
    );
    assert_eq!(commands.first(), reaped.first().copied());
}

#[tokio::test]
async fn a_crate_from_outside_any_workspace_installs_with_no_build_or_reaping_before_it() {
    let machine = machine_holding_the_workspace(&[(
        "stop-gate",
        installed_from_what_the_workspace_holds_now(),
    )]);
    let ripgrep = Resource::Package(Package::Cargo(CargoPackage {
        crate_name: CrateName::from("ripgrep"),
        source: CargoSource::Registry {
            version: PackageCurrency::Latest,
        },
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

    assert!(outcome.passes > 1, "{outcome:?}");
    assert_eq!(
        builds_and_installs(&machine),
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

    assert!(outcome.failed.is_empty(), "{outcome:?}");
    assert_eq!(outcome.converged.len(), 2, "{outcome:?}");
}

fn states_of_the_build(shown: &str) -> Vec<&str> {
    shown
        .lines()
        .take_while(|line| *line != "diagnostics")
        .filter_map(|line| line.strip_prefix("[cargo] build: "))
        .map(|state| state.split(' ').next().unwrap_or(state))
        .collect()
}

#[tokio::test]
async fn a_workspace_build_is_reported_as_a_cargo_entry_from_building_to_built() {
    let machine = machine_holding_the_workspace(&[("claude-session", never_installed())]);
    let reporting = Reporting::opening(RunKind::Apply);

    applying_reported_to(Vec::new(), &machine, &reporting).await;

    let shown = reporting.shown();
    assert_eq!(
        states_of_the_build(&shown),
        ["building", "built"],
        "{shown}"
    );
}

#[tokio::test]
async fn a_workspace_build_that_fails_is_reported_failed_in_its_own_entry() {
    let machine = machine_holding_the_workspace(&[("claude-session", never_installed())]);
    machine.make_workspace_builds_fail();
    let reporting = Reporting::opening(RunKind::Apply);

    applying_reported_to(Vec::new(), &machine, &reporting).await;

    let shown = reporting.shown();
    assert_eq!(
        states_of_the_build(&shown),
        ["building", "failed"],
        "{shown}"
    );
}

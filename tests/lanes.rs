#![allow(clippy::disallowed_macros)]

#[path = "common/declarations.rs"]
mod declarations;
#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    declarations::{
        Reporting, declaring, dotfiles_repository, named_repository,
        reporting_its_version_in_the_second_word,
    },
    dotfiles_configurator::{
        configuration::{
            Application, ApplicationName, ApplicationSource, CargoPackage, CargoSource,
            CargoWorkspace, ClaudeMcpServer, Command, CrateName, EnvironmentVariable, Installer,
            McpScope, McpServerName, Package, PresenceCheck, Registration, ReleasedBinary,
            Resource, Shell, Symlink, UvToolPackage, UvToolVersion, Variable, VariableName,
            VariableValue, WingetPackage,
        },
        confirmation::Operator,
        convergence::{ApplyOutcome, Enactment, apply::apply},
        currency::{SelfReplacement, own_currency},
        machine::{
            release_reading::{ReleaseAsset, ReleaseReading},
            workspace_reading::{
                Fingerprint, InstalledState, MemberReading, ObjectHash, Revision, WorkspaceReading,
            },
        },
        reporting::RunKind,
        version::Version,
    },
    fake_machine::{FakeMachine, Step, dotfiles_repository_path},
    std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
    },
    url::Url,
};

const RIPGREP_RELEASE: &str = "BurntSushi/ripgrep";
const NOTES_REPOSITORY: &str = "Alice/notes";

fn ripgrep() -> ReleasedBinary {
    reporting_its_version_in_the_second_word("rg.exe", RIPGREP_RELEASE)
}

fn released_ripgrep() -> Resource {
    Resource::Application(Application::ReleasedBinary(ripgrep()))
}

fn neovim() -> Resource {
    Resource::Application(Application::Installer(Installer {
        name: ApplicationName::from("Neovim"),
        source: ApplicationSource::Uri {
            uri: Url::parse("https://example.invalid/installer.exe").unwrap(),
            installer_file_name: "Neovim.exe".to_owned(),
        },
        presence_check: PresenceCheck::CommandOnPath {
            command: "Neovim".to_owned(),
        },
    }))
}

fn winget(id: &str) -> Resource {
    Resource::Package(Package::Winget(WingetPackage { id: id.into() }))
}

fn one_entry_of_every_kind() -> Vec<Resource> {
    vec![
        Resource::Repository(named_repository(NOTES_REPOSITORY).into()),
        Resource::Package(Package::Cargo(CargoPackage {
            crate_name: CrateName::from("ripgrep"),
            source: CargoSource::Registry { version: None },
        })),
        winget("Microsoft.PowerShell"),
        winget("Git.Git"),
        neovim(),
        Resource::Package(Package::UvTool(UvToolPackage {
            name: "ruff".into(),
            python: None,
        })),
        released_ripgrep(),
        Resource::EnvironmentVariable(EnvironmentVariable::Variable(Variable {
            name: VariableName::try_from("EDITOR").unwrap(),
            value: VariableValue::from("nvim"),
        })),
        Resource::Symlink(Symlink {
            source_path: PathBuf::from("nvim"),
            link_path: PathBuf::from(".config/nvim"),
        }),
        Resource::Registration(Registration::ClaudeMcpServer(ClaudeMcpServer {
            name: McpServerName::from("context7"),
            scope: McpScope::User,
            command: "context7".to_owned(),
            args: vec!["start-mcp-server".to_owned()],
            environment: BTreeMap::new(),
        })),
        Resource::Command(Command {
            shell: Shell::Bash,
            args: vec!["refresh-completions".to_owned()],
            presence_check: None,
        }),
    ]
}

fn machine_lacking_every_entry() -> FakeMachine {
    let machine = FakeMachine::default();
    machine.repository_holds(PathBuf::from("nvim"));
    machine.clone_dotfiles_repository();
    machine.hold_cargo_workspace(
        dotfiles_repository_path(),
        WorkspaceReading {
            revision: Revision::from("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c"),
            members: BTreeMap::from([(
                CrateName::from("claude-session"),
                MemberReading {
                    desired: Fingerprint {
                        crate_subtree: ObjectHash::from("what the workspace holds now"),
                        workspace_manifest: ObjectHash::from("the workspace manifest"),
                        lock_closure: ObjectHash::from("the lock closure"),
                        dependency_subtrees: BTreeMap::new(),
                    },
                    installed: InstalledState::NotInstalled,
                    absent_binaries: BTreeSet::new(),
                },
            )]),
        },
    );
    machine.publish_uv_tool(&"ruff".into(), &UvToolVersion::from("0.6.0".to_owned()));
    machine.publish_release(
        named_repository(RIPGREP_RELEASE),
        ReleaseReading {
            version: Version::try_from("v15.1.0").unwrap(),
            assets: vec![ReleaseAsset {
                name: "ripgrep-windows-x86_64.zip".to_owned(),
                download_url: Url::parse("https://example.invalid/release.zip").unwrap(),
            }],
        },
    );
    machine.publish_a_newer_configurator("10.0.0");
    machine.own_binary_is_executing_and_will_not_release();
    machine
}

async fn applying(machine: &FakeMachine) -> ApplyOutcome {
    applying_also(machine, Vec::new()).await
}

async fn applying_also(machine: &FakeMachine, also_declared: Vec<Resource>) -> ApplyOutcome {
    let reporting = Reporting::opening(RunKind::Apply);
    let mut declared = one_entry_of_every_kind();
    declared.extend(also_declared);
    let desired_state = declaring(
        declared,
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
        Enactment::Enacted(outcome) => outcome,
        Enactment::Declined | Enactment::ReplacedItself => {
            panic!("the apply did not enact its change set")
        }
    }
}

fn configurator_download() -> String {
    format!("download {}", own_currency().installed_name())
}

fn configurator_install() -> String {
    format!("install {}", own_currency().installed_name())
}

fn ripgrep_download() -> String {
    format!("download {}", ripgrep().installed_name())
}

const WINGET_INSTALLS: [&str; 2] = [
    "winget install Microsoft.PowerShell",
    "winget install Git.Git",
];

struct Journal(Vec<Step>);

impl Journal {
    fn position(&self, step: &Step) -> usize {
        self.0
            .iter()
            .position(|written| written == step)
            .unwrap_or_else(|| panic!("{step:?} is not in {:#?}", self.0))
    }

    fn began(&self, work: &str) -> usize {
        self.position(&Step::Began(work.to_owned()))
    }

    fn ended(&self, work: &str) -> usize {
        self.position(&Step::Ended(work.to_owned()))
    }

    fn holds(&self, work: &str) -> bool {
        self.0.contains(&Step::Began(work.to_owned()))
    }

    fn overlap(&self, first: &str, second: &str) -> bool {
        self.began(first) < self.ended(second) && self.began(second) < self.ended(first)
    }

    fn work(&self) -> Vec<&str> {
        self.0
            .iter()
            .filter_map(|step| match step {
                Step::Began(work) => Some(work.as_str()),
                Step::Ended(_) => None,
            })
            .collect()
    }
}

async fn journal_of_applying(machine: &FakeMachine) -> Journal {
    applying(machine).await;
    Journal(machine.journal())
}

#[tokio::test]
async fn the_running_configurator_is_replaced_before_any_other_entry_begins() {
    let journal = journal_of_applying(&machine_lacking_every_entry()).await;

    let replaced = journal.ended(&configurator_install());
    let others: Vec<&str> = journal
        .work()
        .into_iter()
        .filter(|work| *work != configurator_download() && *work != configurator_install())
        .collect();
    assert!(
        others.iter().all(|work| journal.began(work) > replaced),
        "{:#?}",
        journal.0
    );
}

#[tokio::test]
async fn repositories_are_cloned_after_the_replacement_and_before_any_download_or_lane() {
    let journal = journal_of_applying(&machine_lacking_every_entry()).await;

    let cloned = journal.ended(&format!("clone {NOTES_REPOSITORY}"));
    assert!(
        journal.began(&format!("clone {NOTES_REPOSITORY}"))
            > journal.ended(&configurator_install())
    );
    let later: Vec<&str> = journal
        .work()
        .into_iter()
        .filter(|work| {
            !work.starts_with("clone ")
                && *work != configurator_download()
                && *work != configurator_install()
        })
        .collect();
    assert!(
        later.iter().all(|work| journal.began(work) > cloned),
        "{:#?}",
        journal.0
    );
}

#[tokio::test]
async fn a_declared_command_runs_only_once_every_other_entry_has_ended() {
    let journal = journal_of_applying(&machine_lacking_every_entry()).await;

    let command = journal.began("command refresh-completions");
    let others: Vec<&str> = journal
        .work()
        .into_iter()
        .filter(|work| !work.starts_with("command "))
        .collect();
    assert!(
        others.iter().all(|work| journal.ended(work) < command),
        "{:#?}",
        journal.0
    );
}

#[tokio::test]
async fn the_downloads_overlap_one_another_and_end_before_any_lane_begins() {
    let journal = journal_of_applying(&machine_lacking_every_entry()).await;

    assert!(
        journal.overlap("download Neovim", &ripgrep_download()),
        "{:#?}",
        journal.0
    );
    let downloaded = journal
        .ended("download Neovim")
        .max(journal.ended(&ripgrep_download()));
    for first_of_a_lane in [
        "cargo build",
        "winget source update",
        "uv ruff",
        "set EDITOR",
    ] {
        assert!(
            journal.began(first_of_a_lane) > downloaded,
            "{first_of_a_lane}: {:#?}",
            journal.0
        );
    }
}

#[tokio::test]
async fn the_lanes_run_alongside_one_another() {
    let journal = journal_of_applying(&machine_lacking_every_entry()).await;

    assert!(
        journal.overlap("cargo build", "winget source update")
            && journal.overlap("cargo build", "uv ruff")
            && journal.overlap("winget source update", "uv ruff"),
        "{:#?}",
        journal.0
    );
}

#[tokio::test]
async fn winget_updates_its_sources_before_any_winget_install_begins() {
    let journal = journal_of_applying(&machine_lacking_every_entry()).await;

    let updated = journal.ended("winget source update");
    assert!(
        WINGET_INSTALLS
            .iter()
            .all(|install| journal.began(install) > updated),
        "{:#?}",
        journal.0
    );
}

#[tokio::test]
async fn the_winget_installs_run_alongside_one_another() {
    let journal = journal_of_applying(&machine_lacking_every_entry()).await;

    assert!(
        journal.overlap(WINGET_INSTALLS[0], WINGET_INSTALLS[1]),
        "{:#?}",
        journal.0
    );
}

#[tokio::test]
async fn the_configurators_own_installers_begin_only_once_every_winget_install_has_ended() {
    let journal = journal_of_applying(&machine_lacking_every_entry()).await;

    let installing = journal.began("install Neovim");
    assert!(
        WINGET_INSTALLS
            .iter()
            .all(|install| journal.ended(install) < installing),
        "{:#?}",
        journal.0
    );
}

#[tokio::test]
async fn a_download_that_fails_fails_its_own_entry_and_no_other() {
    let machine = machine_lacking_every_entry();
    machine.make_downloading_fail("Neovim");

    let outcome = applying(&machine).await;

    let failed: Vec<Resource> = outcome
        .failed
        .iter()
        .map(|failure| failure.resource.declared().clone())
        .collect();
    assert_eq!(failed, vec![neovim()]);
    assert!(!Journal(machine.journal()).holds("install Neovim"));
}

#[tokio::test]
async fn winget_sources_that_fail_to_update_still_let_every_winget_install_converge() {
    let machine = machine_lacking_every_entry();
    machine.make_winget_sources_fail_to_update();

    let outcome = applying(&machine).await;

    assert!(
        [winget("Microsoft.PowerShell"), winget("Git.Git")]
            .iter()
            .all(|declared| outcome
                .converged
                .iter()
                .any(|resource| resource.declared() == declared)),
        "{outcome:#?}"
    );
}

#[tokio::test]
async fn an_entry_that_fails_in_one_lane_lets_every_other_lane_finish() {
    let unpublished = Resource::Package(Package::UvTool(UvToolPackage {
        name: "unpublished".into(),
        python: None,
    }));

    let outcome = applying_also(&machine_lacking_every_entry(), vec![unpublished.clone()]).await;

    let failed: Vec<Resource> = outcome
        .failed
        .iter()
        .map(|failure| failure.resource.declared().clone())
        .collect();
    assert_eq!(failed, vec![unpublished], "{outcome:#?}");
    assert_eq!(
        outcome.converged.len() + outcome.held.len(),
        one_entry_of_every_kind().len() + ["claude-session", "the configurator"].len(),
        "{outcome:#?}"
    );
}

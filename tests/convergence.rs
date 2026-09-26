#![allow(clippy::disallowed_macros)]

#[path = "common/declarations.rs"]
mod declarations;
#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    cucumber::{World, given, then, when},
    declarations::{
        dotfiles_repository, named_repository, read_as_two_accounts, read_out_of_a_checkout,
        read_out_of_the_dotfiles_repository, reporting_its_version_in_the_second_word,
    },
    dotfiles_configurator::{
        configuration::{
            Application, ApplicationName, ApplicationSource, AssetPattern, BUILD_GENERATION,
            BinaryName, CargoWorkspace, ClaudeMcpServer, Configuration, ConfigurationName, Context,
            CrateName, DeclaredNotice, EnvironmentVariable, EstateName, EstateOwner, Estates,
            GitHubAccount, Installer, MachineClass, MachineManifest, McpScope, McpServerName,
            Migration, Notice, OLDEST_READABLE_GENERATION, Package, PresenceCheck,
            PythonInterpreter, Registration, Resource, SearchPathDirectory, SearchPathEntry, Shell,
            Symlink, Tool, UvToolPackage, UvToolVersion, Variable, VariableName, VariableValue,
        },
        configuration_source::{AbsoluteDirectory, ConfigurationSource, load_desired_state},
        confirmation::{Confirm, Confirmation, Operator},
        convergence::{
            ApplyOutcome, ChangeSet,
            apply::{Enactment, apply},
            plan,
        },
        currency::SelfReplacement,
        desired_state::DesiredState,
        github::GitHubAccess,
        machine::{
            CommandOutput, ReadInvocation, ReadMachine, WriteMachine,
            release_reading::{ReleaseAsset, ReleaseReading},
            workspace_reading::{
                Fingerprint, InstalledState, MemberReading, ObjectHash, Revision, WorkspaceReading,
            },
        },
        reporting::{RunKind, RunReport},
        version::Version,
    },
    fake_machine::{
        CONFIGURATOR_VERSION, FakeMachine, LauncherCopy, REPOSITORIES_ROOT,
        dotfiles_repository_path,
    },
    std::{
        cell::Cell,
        collections::{BTreeMap, BTreeSet},
        env, fs,
        path::{Path, PathBuf},
        process,
        sync::atomic::{AtomicUsize, Ordering},
    },
    tempfile::TempDir,
    url::Url,
};

#[derive(Debug, World)]
#[world(init = Self::new)]
struct MachineWorld {
    machine: FakeMachine,
    resources: Vec<Resource>,
    workspaces: Vec<CargoWorkspace>,
    members: BTreeMap<CrateName, MemberReading>,
    notices: Vec<DeclaredNotice>,
    configurations_come_from_a_repository: bool,
    configurations_are_inside_a_checkout: bool,
    configurations_are_named_relative_to_the_checkout: bool,
    files_the_checkout_holds: Vec<PathBuf>,
    links_already_into_the_checkout: Vec<(PathBuf, PathBuf)>,
    checkout: Option<PathBuf>,
    documents: Vec<String>,
    employers_documents: Vec<String>,
    employers_resources: Vec<Resource>,
    stray_file_names: Vec<String>,
    change_set: Option<ChangeSet>,
    second_change_set: Option<ChangeSet>,
    enactment: Option<Enactment>,
    answering: Answering,
    self_replacement: SelfReplacement,
    migrations: Vec<Migration>,
    announcements: Vec<Notice>,
    fingerprint_before: Option<String>,
    loading_error: Option<String>,
    loaded: Option<DesiredState>,
    log_directory: TempDir,
    report: Option<RunReport>,
}

impl MachineWorld {
    fn new() -> Self {
        Self {
            machine: FakeMachine::default(),
            resources: Vec::new(),
            workspaces: Vec::new(),
            members: BTreeMap::new(),
            notices: Vec::new(),
            configurations_come_from_a_repository: false,
            configurations_are_inside_a_checkout: true,
            configurations_are_named_relative_to_the_checkout: false,
            files_the_checkout_holds: Vec::new(),
            links_already_into_the_checkout: Vec::new(),
            checkout: None,
            documents: Vec::new(),
            employers_documents: Vec::new(),
            employers_resources: Vec::new(),
            stray_file_names: Vec::new(),
            change_set: None,
            second_change_set: None,
            enactment: None,
            answering: Answering::default(),
            self_replacement: SelfReplacement::Available,
            migrations: Vec::new(),
            announcements: Vec::new(),
            fingerprint_before: None,
            loading_error: None,
            loaded: None,
            log_directory: tempfile::tempdir().expect("a directory to write run logs into"),
            report: None,
        }
    }

    fn open_a_report(&self, kind: RunKind) -> RunReport {
        RunReport::open_in(self.log_directory.path(), kind).unwrap()
    }

    fn logged_runs(&self) -> usize {
        fs::read_dir(self.log_directory.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|kind| kind == "log"))
            .count()
    }

    fn desired_state(&self) -> DesiredState {
        if !self.employers_resources.is_empty() {
            return read_as_two_accounts(self.resources.clone(), self.employers_resources.clone());
        }

        let read = match self.configurations_come_from_a_repository {
            true => read_out_of_the_dotfiles_repository,
            false => read_out_of_a_checkout,
        };
        read(
            self.resources.clone(),
            self.workspaces.clone(),
            self.notices.clone(),
        )
        .also_reporting(self.migrations.clone(), self.announcements.clone())
    }

    fn linked_paths(&self) -> Vec<String> {
        self.loaded
            .as_ref()
            .expect("nothing was loaded")
            .resources
            .iter()
            .filter_map(|resource| match resource.declared() {
                Resource::Symlink(symlink) => Some(symlink.link_path.display().to_string()),
                _ => None,
            })
            .collect()
    }

    fn member(&mut self, crate_name: &str) -> &mut MemberReading {
        self.members
            .get_mut(&CrateName::from(crate_name))
            .expect("the scenario has not said the workspace holds that crate")
    }

    fn publish_workspace(&self) {
        if self.workspaces.is_empty() {
            return;
        }

        self.machine.hold_cargo_workspace(
            dotfiles_repository_path(),
            WorkspaceReading {
                revision: Revision::from("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c"),
                members: self.members.clone(),
            },
        );
    }

    fn change_set(&self) -> &ChangeSet {
        self.change_set
            .as_ref()
            .expect("the scenario has not planned yet")
    }

    fn enactment(&self) -> &Enactment {
        self.enactment
            .as_ref()
            .expect("the scenario has not applied yet")
    }

    fn outcome(&self) -> &ApplyOutcome {
        match self.enactment() {
            Enactment::Enacted(outcome) => outcome,
            Enactment::Declined => panic!("the scenario declined the change set"),
            Enactment::ReplacedItself => {
                panic!("the run was handed to the newer configurator")
            }
        }
    }

    fn was_declined(&self) -> bool {
        match self.enactment() {
            Enactment::Enacted(_) | Enactment::ReplacedItself => false,
            Enactment::Declined => true,
        }
    }

    fn handed_over(&self) -> bool {
        match self.enactment() {
            Enactment::ReplacedItself => true,
            Enactment::Enacted(_) | Enactment::Declined => false,
        }
    }
}

#[derive(Debug)]
struct Answering {
    answer: Answer,
    questions_put: Cell<usize>,
}

#[derive(Debug)]
enum Answer {
    InAdvance(Operator),
    Alice(Confirmation),
}

impl Default for Answering {
    fn default() -> Self {
        Answering::of(Answer::Alice(Confirmation::Proceed))
    }
}

impl Answering {
    fn of(answer: Answer) -> Self {
        Self {
            answer,
            questions_put: Cell::new(0),
        }
    }

    fn declining() -> Self {
        Answering::of(Answer::Alice(Confirmation::Declined))
    }
}

impl Confirm for Answering {
    fn confirmation(&self, question: &str) -> Confirmation {
        self.questions_put.set(self.questions_put.get() + 1);

        match &self.answer {
            Answer::InAdvance(operator) => operator.confirmation(question),
            Answer::Alice(confirmation) => *confirmation,
        }
    }
}

fn application(name: &str) -> Application {
    application_checked_by(
        name,
        PresenceCheck::CommandOnPath {
            command: name.to_owned(),
        },
    )
}

fn application_checked_by(name: &str, presence_check: PresenceCheck) -> Application {
    Application::Installer(Installer {
        name: ApplicationName::from(name),
        source: ApplicationSource::Uri {
            uri: Url::parse("https://example.invalid/installer.exe").unwrap(),
            installer_file_name: format!("{name}.exe"),
        },
        presence_check,
    })
}

fn application_installed_from_a_release_of(name: &str, owner_and_name: &str) -> Application {
    let repository = named_repository(owner_and_name);
    Application::Installer(Installer {
        name: ApplicationName::from(name),
        source: ApplicationSource::GitHubRelease {
            owner: repository.owner,
            repository: repository.repository,
            asset: AssetPattern::EndsWith(".zip".to_owned()),
        },
        presence_check: PresenceCheck::CommandOnPath {
            command: name.to_owned(),
        },
    })
}

#[given(expr = "Alice declares the application {string}")]
fn declare_application(world: &mut MachineWorld, name: String) {
    world
        .resources
        .push(Resource::Application(application(&name)));
}

#[given(expr = "Alice declares the application {string} checked by the path {string}")]
fn declare_application_checked_by_a_path(world: &mut MachineWorld, name: String, path: String) {
    world
        .resources
        .push(Resource::Application(application_checked_by(
            &name,
            PresenceCheck::PathExists {
                path: PathBuf::from(path),
            },
        )));
}

#[given(expr = "{string} is on Alice's machine")]
fn a_path_is_on_alices_machine(world: &mut MachineWorld, path: String) {
    let resolved = world.machine.resolve_against_home(Path::new(&path));
    world.machine.add_own_file(resolved);
}

#[given(expr = "Alice declares the released binary {string} from {string}")]
fn declare_released_binary(world: &mut MachineWorld, entry: String, owner_and_name: String) {
    world
        .resources
        .push(Resource::Application(Application::ReleasedBinary(
            reporting_its_version_in_the_second_word(&entry, &owner_and_name),
        )));
}

#[given(expr = "the latest release of {string} is {string}")]
fn latest_release_is(world: &mut MachineWorld, owner_and_name: String, tag: String) {
    let repository = named_repository(&owner_and_name);
    world.machine.publish_release(
        repository.clone(),
        ReleaseReading {
            version: Version::try_from(tag.as_str()).unwrap(),
            assets: vec![ReleaseAsset {
                name: format!("{}-windows-x86_64.zip", repository.repository),
                download_url: Url::parse("https://example.invalid/release.zip").unwrap(),
            }],
        },
    );
}

#[given(expr = "{string} is installed and reports {string}")]
fn binary_is_installed_reporting(world: &mut MachineWorld, name: String, printed: String) {
    let path = world.machine.binaries_directory().join(&name);
    world.machine.hold_binary(path, printed);
}

#[then(expr = "{string} is installed in the tool directory")]
fn binary_is_installed(world: &mut MachineWorld, name: String) {
    let path = world.machine.binaries_directory().join(&name);

    assert!(
        world.machine.path_exists(&path),
        "nothing is installed at {}",
        path.display()
    );
}

#[then(expr = "{string} reports {string}")]
fn binary_reports(world: &mut MachineWorld, name: String, expected: String) {
    let path = world.machine.binaries_directory().join(&name);
    let printed = world
        .machine
        .binary_reports(&path)
        .unwrap_or_else(|| panic!("nothing is installed at {}", path.display()));

    assert!(printed.contains(&expected), "{name} reports {printed:?}");
}

const A_NEWER_CONFIGURATOR: &str = "10.0.0";

#[given(expr = "a newer configurator than this machine holds has been released")]
fn a_newer_configurator_is_released(world: &mut MachineWorld) {
    world
        .machine
        .publish_a_newer_configurator(A_NEWER_CONFIGURATOR);
}

#[then(expr = "the configurator reports the version of its latest release")]
fn configurator_reports_the_latest_release(world: &mut MachineWorld) {
    let printed = world
        .machine
        .configurator_reports()
        .expect("the configurator is installed in the tool directory");

    assert!(printed.contains(A_NEWER_CONFIGURATOR), "{printed}");
}

#[then(expr = "the configurator still reports the version the machine held")]
fn configurator_still_reports_the_version_held(world: &mut MachineWorld) {
    let printed = world
        .machine
        .configurator_reports()
        .expect("the configurator is installed in the tool directory");

    assert!(printed.ends_with(CONFIGURATOR_VERSION), "{printed}");
}

#[given(expr = "Alice's run is being carried on by the build that replaced {string}")]
fn run_carried_on_by_the_build_that_replaced(world: &mut MachineWorld, replaced: String) {
    world.self_replacement = SelfReplacement::Spent {
        replaced: Version::try_from(replaced.as_str()).expect("a version"),
    };
}

#[then(expr = "the rest of the run is handed to the newer configurator")]
fn run_is_handed_to_the_newer_configurator(world: &mut MachineWorld) {
    assert!(
        world.handed_over(),
        "the run was not handed over: {:?}",
        world.enactment()
    );
}

#[then(regex = r"^(.+) is not yet installed on Alice's machine$")]
fn then_application_is_not_yet_installed(world: &mut MachineWorld, name: String) {
    assert!(
        !world
            .machine
            .application_is_installed(&ApplicationName::from(name.as_str()))
    );
}

#[given(expr = "Alice's machine is running the configurator and will not let it be replaced")]
fn configurator_is_running_and_undisplaceable(world: &mut MachineWorld) {
    world.machine.own_binary_is_executing_and_will_not_release();
}

#[given(expr = "Alice declares the winget package {string}")]
fn declare_winget_package(world: &mut MachineWorld, id: String) {
    world.resources.push(Resource::Package(
        dotfiles_configurator::configuration::Package::Winget(
            dotfiles_configurator::configuration::WingetPackage { id: id.into() },
        ),
    ));
}

#[given(expr = "winget holds {string} on Alice's machine")]
fn winget_holds_package(world: &mut MachineWorld, id: String) {
    world.machine.install_winget_package(&id.into());
}

#[given(
    expr = "winget holds {string} on Alice's machine, found only when asked for it by identifier"
)]
fn winget_holds_package_matched_only_by_identifier(world: &mut MachineWorld, id: String) {
    world
        .machine
        .install_winget_package_matched_only_by_identifier(&id.into());
}

#[given(expr = "winget cannot be asked about {string} by identifier on Alice's machine")]
fn winget_cannot_be_asked_by_identifier(world: &mut MachineWorld, id: String) {
    world.machine.answer_reading_with(
        ReadInvocation::WingetPackage { id: id.into() },
        CommandOutput {
            succeeded: false,
            standard_output: String::new(),
            standard_error: "Failed when opening source(s); try the 'source reset' command"
                .to_owned(),
        },
    );
}

fn declare_uv_tool(world: &mut MachineWorld, name: String, python: Option<PythonInterpreter>) {
    world
        .resources
        .push(Resource::Package(Package::UvTool(UvToolPackage {
            name: name.into(),
            python,
        })));
}

#[given(expr = "Alice declares the uv tool {string}")]
fn declare_uv_tool_with_any_interpreter(world: &mut MachineWorld, name: String) {
    declare_uv_tool(world, name, None);
}

#[given(expr = "Alice declares the uv tool {string} built with Python {string}")]
fn declare_uv_tool_built_with(world: &mut MachineWorld, name: String, python: String) {
    declare_uv_tool(world, name, Some(python.into()));
}

#[given(expr = "uv holds {string} at {string} on Alice's machine")]
fn uv_holds_tool(world: &mut MachineWorld, name: String, version: String) {
    world
        .machine
        .install_uv_tool(&name.into(), &UvToolVersion::from(version));
}

#[given(expr = "the newest version of {string} that resolves is {string}")]
fn newest_uv_tool_version(world: &mut MachineWorld, name: String, version: String) {
    world
        .machine
        .publish_uv_tool(&name.into(), &UvToolVersion::from(version));
}

#[given(expr = "uv cannot reach the index its tools resolve against on Alice's machine")]
fn uv_cannot_reach_its_index(world: &mut MachineWorld) {
    world.machine.make_reading_fail(
        ReadInvocation::UvOutdatedTools,
        "error: Failed to fetch: `https://pypi.org/simple/serena-agent/`",
    );
}

#[then(expr = "uv holds {string} at {string} on Alice's machine")]
fn uv_now_holds_tool(world: &mut MachineWorld, name: String, version: String) {
    assert_eq!(
        world.machine.uv_tool_version(&name.into()),
        Some(UvToolVersion::from(version))
    );
}

#[given(
    expr = "Alice's machine is running the launcher of {string}, which the upgrade leaves unchanged"
)]
fn running_an_unchanged_launcher(world: &mut MachineWorld, name: String) {
    world
        .machine
        .run_uv_tool_launcher(&name.into(), LauncherCopy::Identical);
}

#[given(expr = "Alice's machine is running the launcher of {string}, which the upgrade changes")]
fn running_a_changed_launcher(world: &mut MachineWorld, name: String) {
    world
        .machine
        .run_uv_tool_launcher(&name.into(), LauncherCopy::Differs);
}

#[given(expr = "upgrading {string} fails on Alice's machine")]
fn upgrading_fails(world: &mut MachineWorld, name: String) {
    world.machine.make_uv_tool_upgrade_fail(&name.into());
}

#[then(expr = "uv built {string} with Python {string} on Alice's machine")]
fn uv_built_tool_with(world: &mut MachineWorld, name: String, python: String) {
    assert_eq!(
        world.machine.uv_tool_interpreter(&name.into()),
        Some(PythonInterpreter::from(python))
    );
}

fn declared_mcp_server(name: &str) -> ClaudeMcpServer {
    ClaudeMcpServer {
        name: McpServerName::from(name),
        scope: McpScope::User,
        command: name.to_owned(),
        args: vec!["start-mcp-server".to_owned()],
        environment: BTreeMap::from([("MCP_LOG_LEVEL".to_owned(), "info".to_owned())]),
    }
}

#[given(expr = "Alice declares the claude mcp server {string}")]
fn declare_claude_mcp_server(world: &mut MachineWorld, name: String) {
    world
        .resources
        .push(Resource::Registration(Registration::ClaudeMcpServer(
            declared_mcp_server(&name),
        )));
}

#[given(expr = "claude holds {string} as Alice declared it on Alice's machine")]
fn claude_holds_the_declared_server(world: &mut MachineWorld, name: String) {
    world
        .machine
        .hold_claude_mcp_server(declared_mcp_server(&name));
}

#[given(expr = "claude holds {string} started with {string} on Alice's machine")]
fn claude_holds_a_server_started_with(world: &mut MachineWorld, name: String, arguments: String) {
    world.machine.hold_claude_mcp_server(ClaudeMcpServer {
        args: arguments.split(' ').map(str::to_owned).collect(),
        ..declared_mcp_server(&name)
    });
}

#[given(expr = "claude refuses to add {string} on Alice's machine")]
fn claude_refuses_to_add(world: &mut MachineWorld, name: String) {
    world
        .machine
        .refuse_to_add_claude_mcp_server(&McpServerName::from(name.as_str()));
}

#[then(expr = "claude holds {string} as Alice declared it")]
fn claude_now_holds_the_declared_server(world: &mut MachineWorld, name: String) {
    assert_eq!(
        world
            .machine
            .claude_mcp_server(&McpServerName::from(name.as_str())),
        Some(declared_mcp_server(&name))
    );
}

#[then(expr = "claude holds no server {string}")]
fn claude_holds_no_server(world: &mut MachineWorld, name: String) {
    assert_eq!(
        world
            .machine
            .claude_mcp_server(&McpServerName::from(name.as_str())),
        None
    );
}

#[then(expr = "the run reports a failure mentioning {string}")]
fn run_reports_a_failure_mentioning(world: &mut MachineWorld, text: String) {
    let rendered = world.outcome().to_string();

    assert!(
        rendered.contains(&text),
        "expected a failure mentioning {text:?}, got:\n{rendered}"
    );
}

#[then(expr = "the run reports no failure mentioning {string}")]
fn run_reports_no_failure_mentioning(world: &mut MachineWorld, text: String) {
    let rendered = world.outcome().to_string();

    assert!(
        !rendered.contains(&text),
        "expected nothing mentioning {text:?}, got:\n{rendered}"
    );
}

#[given(expr = "Alice declares the notice {string}")]
fn declare_notice(world: &mut MachineWorld, message: String) {
    world.notices.push(DeclaredNotice::from(message.as_str()));
}

#[given(expr = "Alice declares the symlink {string} at {string}")]
fn declare_symlink(world: &mut MachineWorld, source_path: String, link_path: String) {
    world.resources.push(Resource::Symlink(Symlink {
        source_path: PathBuf::from(source_path),
        link_path: PathBuf::from(link_path),
    }));
}

#[given(expr = "Alice's configurations come from the dotfiles repository")]
fn configurations_come_from_a_repository(world: &mut MachineWorld) {
    world.configurations_come_from_a_repository = true;
}

#[given(expr = "Alice declares the command {string} with no presence check")]
fn declare_command_without_check(world: &mut MachineWorld, command: String) {
    world.resources.push(Resource::Command(
        dotfiles_configurator::configuration::Command {
            shell: Shell::Bash,
            args: vec![command],
            presence_check: None,
        },
    ));
}

#[given(
    expr = "Alice's employer's configuration declares the command {string} with no presence check"
)]
fn employers_configuration_declares_a_command(world: &mut MachineWorld, command: String) {
    world.employers_resources.push(Resource::Command(
        dotfiles_configurator::configuration::Command {
            shell: Shell::Bash,
            args: vec![command],
            presence_check: None,
        },
    ));
}

fn output_check(invocation: &str, shell: Shell) -> PresenceCheck {
    PresenceCheck::CommandOutputContains {
        shell,
        args: invocation.split(' ').map(str::to_owned).collect(),
        contains: "ready".to_owned(),
    }
}

#[given(expr = "Alice declares the command {string} checked by the output of {string}")]
fn declare_command_checked_by_output(world: &mut MachineWorld, command: String, check: String) {
    world.resources.push(Resource::Command(
        dotfiles_configurator::configuration::Command {
            shell: Shell::Bash,
            args: vec![command],
            presence_check: Some(output_check(&check, Shell::Bash)),
        },
    ));
}

#[given(expr = "Alice declares the command {string} checked through WSL")]
fn declare_command_checked_through_wsl(world: &mut MachineWorld, command: String) {
    world.resources.push(Resource::Command(
        dotfiles_configurator::configuration::Command {
            shell: Shell::Bash,
            args: vec![command],
            presence_check: Some(output_check("completions --status", Shell::Wsl)),
        },
    ));
}

#[given(expr = "the check {string} already passes on Alice's machine")]
fn the_check_already_passes(world: &mut MachineWorld, invocation: String) {
    world
        .machine
        .answer_presence_check(output_check(&invocation, Shell::Bash), true);
}

#[given(expr = "the check {string} already passes through WSL on Alice's machine")]
fn the_check_already_passes_through_wsl(world: &mut MachineWorld, invocation: String) {
    world
        .machine
        .answer_presence_check(output_check(&invocation, Shell::Wsl), true);
}

#[given(expr = "the check {string} does not pass on Alice's machine")]
fn the_check_does_not_pass(world: &mut MachineWorld, invocation: String) {
    world
        .machine
        .answer_presence_check(output_check(&invocation, Shell::Bash), false);
}

#[given(expr = "the check {string} cannot be run on Alice's machine")]
fn the_check_cannot_be_run(world: &mut MachineWorld, invocation: String) {
    world
        .machine
        .make_presence_check_fail_to_run(output_check(&invocation, Shell::Bash));
}

#[given(expr = "the presence check for {string} cannot be run on Alice's machine")]
fn the_presence_check_for_an_application_cannot_be_run(world: &mut MachineWorld, name: String) {
    world
        .machine
        .make_presence_check_fail_to_run(PresenceCheck::CommandOnPath { command: name });
}

#[given(expr = "{string} cannot be asked for its latest release")]
fn the_latest_release_cannot_be_read(world: &mut MachineWorld, owner_and_name: String) {
    world
        .machine
        .make_release_reading_fail(named_repository(&owner_and_name));
}

#[given(expr = "winget cannot be read on Alice's machine")]
fn winget_cannot_be_read(world: &mut MachineWorld) {
    world.machine.make_reading_fail(
        ReadInvocation::WingetInstalledPackages,
        "winget exited with an error",
    );
}

#[given(expr = "winget lists its packages without the columns that name them")]
fn winget_lists_without_columns(world: &mut MachineWorld) {
    world.machine.answer_reading_with(
        ReadInvocation::WingetInstalledPackages,
        CommandOutput {
            succeeded: true,
            standard_output: "a listing with no header row\n".to_owned(),
            standard_error: String::new(),
        },
    );
}

#[given(regex = r"^(.+) is installed on Alice's machine$")]
fn application_is_installed(world: &mut MachineWorld, name: String) {
    world
        .machine
        .install_application(&ApplicationName::from(name.as_str()));
}

#[given(regex = r"^(.+) is not installed on Alice's machine$")]
fn application_is_not_installed(world: &mut MachineWorld, name: String) {
    let name = ApplicationName::from(name.as_str());
    assert!(
        !world.machine.application_is_installed(&name),
        "{name} is installed on Alice's machine"
    );
}

#[given(regex = r"^installing (.+) fails on Alice's machine$")]
fn installing_fails(world: &mut MachineWorld, name: String) {
    world
        .machine
        .make_installing_fail(&ApplicationName::from(name.as_str()));
}

#[given(regex = r"^installing (.+) reports success without installing anything$")]
fn installing_does_nothing(world: &mut MachineWorld, name: String) {
    world
        .machine
        .make_installing_silently_do_nothing(&ApplicationName::from(name.as_str()));
}

#[given(expr = "Alice already has a file of her own at {string}")]
fn alice_has_her_own_file(world: &mut MachineWorld, path: String) {
    let resolved = world.machine.resolve_against_home(Path::new(&path));
    world.machine.add_own_file(resolved);
}

#[given(expr = "winget is absent from Alice's machine")]
fn winget_is_absent(world: &mut MachineWorld) {
    world.machine.remove_tool(Tool::Winget);
}

#[given(expr = "git is absent from Alice's machine")]
fn git_is_absent(world: &mut MachineWorld) {
    world.machine.remove_tool(Tool::Git);
}

#[given(expr = "wsl is absent from Alice's machine")]
fn wsl_is_absent(world: &mut MachineWorld) {
    world.machine.remove_tool(Tool::Wsl);
}

#[given(expr = "the dotfiles repository holds {string}")]
fn repository_holds(world: &mut MachineWorld, path: String) {
    world.machine.repository_holds(PathBuf::from(path));
}

#[given(expr = "the dotfiles repository has been cloned on Alice's machine")]
fn dotfiles_repository_is_cloned(world: &mut MachineWorld) {
    world.machine.clone_dotfiles_repository();
}

#[given(expr = "the dotfiles repository has not been cloned on Alice's machine")]
fn dotfiles_repository_is_not_cloned(world: &mut MachineWorld) {
    assert!(
        !world.machine.dotfiles_repository_is_cloned(),
        "the dotfiles repository is cloned on Alice's machine"
    );
}

#[given(expr = "Alice has a configuration declaring version {string}")]
fn configuration_with_version(world: &mut MachineWorld, version: String) {
    world.documents.push(document(&version, "everywhere", "[]"));
}

#[given(expr = "Alice has a configuration declaring a variable named {string}")]
fn configuration_declaring_a_variable_named(world: &mut MachineWorld, name: String) {
    world.documents.push(document(
        &BUILD_GENERATION.to_string(),
        "everywhere",
        &format!(
            r#"[{{ "kind": "environment_variable", "shape": "variable",
                   "name": "{name}", "value": "C:\\only\\this" }}]"#
        ),
    ));
}

#[given(expr = "Alice has a configuration for work machines declaring version {string}")]
fn work_configuration_with_version(world: &mut MachineWorld, version: String) {
    world.documents.push(document(&version, "work", "[]"));
}

#[given(expr = "Alice has a configuration declaring a generation beyond this build")]
fn configuration_beyond_this_build(world: &mut MachineWorld) {
    world.documents.push(document(
        &BUILD_GENERATION.stepped_by(1).to_string(),
        "everywhere",
        "[]",
    ));
}

#[given(
    expr = "Alice has a configuration for every machine declaring the generation below this build \
            linking {string} to {string}"
)]
fn configuration_for_every_machine_a_generation_back_linking(
    world: &mut MachineWorld,
    link_path: String,
    source_path: String,
) {
    world
        .documents
        .push(outgoing_document(&symlink(&link_path, &source_path)));
}

fn outgoing_document(resources: &str) -> String {
    format!(
        r#"{{
            "version": "{OLDEST_READABLE_GENERATION}",
            "applies_to": "everywhere",
            "github_account": "Alice",
            "resources": {resources}
        }}"#
    )
}

#[given(expr = "Alice has a configuration declaring a generation this build has outgrown")]
fn configuration_this_build_has_outgrown(world: &mut MachineWorld) {
    world.documents.push(document(
        &OLDEST_READABLE_GENERATION.stepped_by(-1).to_string(),
        "everywhere",
        "[]",
    ));
}

#[given(expr = "Alice keeps a {string} alongside her configurations")]
fn stray_file_alongside_configurations(world: &mut MachineWorld, file_name: String) {
    world.stray_file_names.push(file_name);
}

#[given(expr = "Alice's checkout holds {string}")]
fn checkout_holds(world: &mut MachineWorld, path: String) {
    world.files_the_checkout_holds.push(PathBuf::from(path));
}

#[given(expr = "the link {string} already resolves to {string} in Alice's checkout")]
fn link_already_into_the_checkout(world: &mut MachineWorld, link_path: String, held: String) {
    world
        .links_already_into_the_checkout
        .push((PathBuf::from(link_path), PathBuf::from(held)));
}

#[given(expr = "Alice names her configurations relative to the checkout she runs in")]
fn configurations_named_relative_to_the_checkout(world: &mut MachineWorld) {
    world.configurations_are_named_relative_to_the_checkout = true;
}

#[given(expr = "Alice keeps her configurations outside any checkout")]
fn configurations_outside_any_checkout(world: &mut MachineWorld) {
    world.configurations_are_inside_a_checkout = false;
}

#[given(
    expr = "Alice's employer's repository has a configuration for work machines linking {string} \
            to {string}"
)]
fn employers_configuration_linking(
    world: &mut MachineWorld,
    link_path: String,
    source_path: String,
) {
    world.employers_documents.push(document(
        &BUILD_GENERATION.to_string(),
        "work",
        &symlink(&link_path, &source_path),
    ));
}

#[given(expr = "Alice's employer's configuration declares the repository {string}")]
fn employers_configuration_declares_a_repository(world: &mut MachineWorld, owner_and_name: String) {
    world
        .employers_resources
        .push(Resource::Repository(named_repository(&owner_and_name)));
}

#[given(
    expr = "Alice's employer's configuration declares the application {string} released by \
            {string}"
)]
fn employers_configuration_declares_an_application(
    world: &mut MachineWorld,
    name: String,
    owner_and_name: String,
) {
    world.employers_resources.push(Resource::Application(
        application_installed_from_a_release_of(&name, &owner_and_name),
    ));
}

#[then(expr = "{string} is cloned as {string}")]
fn repository_is_cloned_as(world: &mut MachineWorld, owner_and_name: String, account: String) {
    assert_eq!(
        world
            .machine
            .account_cloning(&named_repository(&owner_and_name)),
        Some(GitHubAccount::from(account.as_str()))
    );
}

#[then(expr = "{string} is installed as {string}")]
fn application_is_installed_as(world: &mut MachineWorld, name: String, account: String) {
    assert_eq!(
        world
            .machine
            .account_installing(&ApplicationName::from(name.as_str())),
        Some(GitHubAccount::from(account.as_str()))
    );
}

#[given(expr = "Alice has a configuration that declares no machines it is for")]
fn configuration_without_a_context(world: &mut MachineWorld) {
    world.documents.push(format!(
        r#"{{ "version": "{BUILD_GENERATION}", "github_account": "Alice", "resources": [] }}"#
    ));
}

#[given(expr = "Alice has a configuration naming no account to act as")]
fn configuration_without_an_account(world: &mut MachineWorld) {
    world.documents.push(format!(
        r#"{{ "version": "{BUILD_GENERATION}", "applies_to": "everywhere", "resources": [] }}"#
    ));
}

#[given(expr = "Alice has a configuration for every machine linking {string} to {string}")]
fn configuration_for_every_machine_linking(
    world: &mut MachineWorld,
    link_path: String,
    source_path: String,
) {
    world.documents.push(document(
        &BUILD_GENERATION.to_string(),
        "everywhere",
        &symlink(&link_path, &source_path),
    ));
}

#[given(expr = "Alice has a configuration for personal machines linking {string} to {string}")]
fn personal_configuration_linking(
    world: &mut MachineWorld,
    link_path: String,
    source_path: String,
) {
    world.documents.push(document(
        &BUILD_GENERATION.to_string(),
        "personal",
        &symlink(&link_path, &source_path),
    ));
}

#[given(expr = "Alice has a configuration for work machines linking {string} to {string}")]
fn work_configuration_linking(world: &mut MachineWorld, link_path: String, source_path: String) {
    world.documents.push(document(
        &BUILD_GENERATION.to_string(),
        "work",
        &symlink(&link_path, &source_path),
    ));
}

fn symlink(link_path: &str, source_path: &str) -> String {
    format!(
        r#"[{{ "kind": "symlink", "source_path": "{source_path}", "link_path": "{link_path}" }}]"#
    )
}

fn document(version: &str, applies_to: &str, resources: &str) -> String {
    format!(
        r#"{{
            "version": "{version}",
            "applies_to": "{applies_to}",
            "github_account": "Alice",
            "resources": {resources}
        }}"#
    )
}

fn document_declaring_an_estate(
    applies_to: &str,
    estate: &str,
    workspaces: &str,
    resources: &str,
) -> String {
    document_acting_as_declaring_an_estate("Alice", applies_to, estate, workspaces, resources)
}

fn document_acting_as_declaring_an_estate(
    account: &str,
    applies_to: &str,
    estate: &str,
    workspaces: &str,
    resources: &str,
) -> String {
    format!(
        r#"{{
            "version": "{BUILD_GENERATION}",
            "applies_to": "{applies_to}",
            "github_account": "{account}",
            "estate": {estate},
            "workspaces": {workspaces},
            "resources": {resources}
        }}"#
    )
}

fn workspace_of(owner_and_name: &str) -> String {
    let repository = named_repository(owner_and_name);
    format!(
        r#"[{{ "repository": {{ "owner": "{}", "repository": "{}" }} }}]"#,
        repository.owner, repository.repository
    )
}

#[given(expr = "Alice has a configuration for every machine declaring the estate {string}")]
fn every_machine_declaring_an_estate(world: &mut MachineWorld, estate: String) {
    world.documents.push(document_declaring_an_estate(
        "everywhere",
        &format!(r#"{{ "name": "{estate}" }}"#),
        "[]",
        "[]",
    ));
}

#[given(
    expr = "Alice has a configuration for every machine declaring the estate {string} and building \
            the workspace {string}"
)]
fn every_machine_declaring_an_estate_building(
    world: &mut MachineWorld,
    estate: String,
    workspace: String,
) {
    world.documents.push(document_declaring_an_estate(
        "everywhere",
        &format!(r#"{{ "name": "{estate}" }}"#),
        &workspace_of(&workspace),
        "[]",
    ));
}

#[given(
    expr = "Alice has a configuration for every machine declaring the estate {string} and cloning \
            {string}"
)]
fn every_machine_declaring_an_estate_cloning(
    world: &mut MachineWorld,
    estate: String,
    cloned: String,
) {
    let repository = named_repository(&cloned);
    world.documents.push(document_declaring_an_estate(
        "everywhere",
        &format!(r#"{{ "name": "{estate}" }}"#),
        "[]",
        &format!(
            r#"[{{ "kind": "repository", "owner": "{}", "repository": "{}" }}]"#,
            repository.owner, repository.repository
        ),
    ));
}

#[given(
    expr = "Alice has a configuration for personal machines declaring the estate {string} with the \
            owner {string}"
)]
fn personal_declaring_an_estate_with_an_owner(
    world: &mut MachineWorld,
    estate: String,
    owner: String,
) {
    world.documents.push(document_declaring_an_estate(
        "personal",
        &format!(r#"{{ "name": "{estate}", "owners": ["{owner}"] }}"#),
        "[]",
        "[]",
    ));
}

#[given(
    expr = "Alice has a configuration for personal machines acting as {string}, declaring the \
            estate {string} with the owner {string}"
)]
fn personal_acting_as_declaring_an_estate_with_an_owner(
    world: &mut MachineWorld,
    account: String,
    estate: String,
    owner: String,
) {
    world.documents.push(document_acting_as_declaring_an_estate(
        &account,
        "personal",
        &format!(r#"{{ "name": "{estate}", "owners": ["{owner}"] }}"#),
        "[]",
        "[]",
    ));
}

impl MachineWorld {
    fn loaded_estates(&self) -> Estates {
        let loaded = self.loaded.as_ref().expect("a desired state was loaded");
        loaded
            .resources
            .iter()
            .find_map(|resource| match resource.declared() {
                Resource::Registration(Registration::MachineManifest(manifest)) => {
                    Some(manifest.estates.clone())
                }
                _ => None,
            })
            .expect("every desired state carries the machine manifest")
    }
}

#[then(expr = "the machine's manifest places {string} in the estate {string}")]
fn manifest_places_owner_in_estate(world: &mut MachineWorld, owner: String, estate: String) {
    let estates = world.loaded_estates();
    let owners = estates.get(&EstateName::try_from(estate.as_str()).unwrap());

    assert!(
        owners.is_some_and(|owners| owners.contains(&EstateOwner::from(owner.as_str()))),
        "expected {owner} in the estate {estate}, got {estates:?}"
    );
}

#[then(expr = "the machine's manifest places {string} in no estate")]
fn manifest_places_owner_in_no_estate(world: &mut MachineWorld, owner: String) {
    let estates = world.loaded_estates();

    assert!(
        !estates
            .values()
            .any(|owners| owners.contains(&EstateOwner::from(owner.as_str()))),
        "expected {owner} in no estate, got {estates:?}"
    );
}

#[then(expr = "the machine's manifest names no estate")]
fn manifest_names_no_estate(world: &mut MachineWorld) {
    let estates = world.loaded_estates();

    assert!(estates.is_empty(), "{estates:?}");
}

#[given(expr = "Alice declares the cargo package {string}")]
fn declare_cargo_package(world: &mut MachineWorld, crate_name: String) {
    world.resources.push(Resource::Package(
        dotfiles_configurator::configuration::Package::Cargo(
            dotfiles_configurator::configuration::CargoPackage {
                crate_name: CrateName::from(crate_name.as_str()),
                source: dotfiles_configurator::configuration::CargoSource::Registry,
            },
        ),
    ));
}

#[given(expr = "Alice declares the cargo workspace in the dotfiles repository")]
fn declare_cargo_workspace(world: &mut MachineWorld) {
    world.workspaces.push(CargoWorkspace {
        repository: dotfiles_repository(),
    });
}

#[given(expr = "the workspace holds the crate {string}")]
fn workspace_holds_crate(world: &mut MachineWorld, crate_name: String) {
    world.members.insert(
        CrateName::from(crate_name.as_str()),
        MemberReading {
            desired: content_named("what the workspace holds now"),
            installed: InstalledState::NotInstalled,
            absent_binaries: BTreeSet::new(),
        },
    );
}

#[given(expr = "cargo installed {string} from the content the workspace holds now")]
fn installed_from_current_content(world: &mut MachineWorld, crate_name: String) {
    let member = world.member(&crate_name);
    member.installed = InstalledState::At(member.desired.clone());
}

#[given(expr = "the binary {string} of {string} is gone from where cargo installs it")]
fn binary_is_gone(world: &mut MachineWorld, binary_name: String, crate_name: String) {
    world
        .member(&crate_name)
        .absent_binaries
        .insert(BinaryName::from(binary_name.as_str()));
}

#[given(expr = "Alice's machine is executing the binary {string}")]
fn machine_is_executing(world: &mut MachineWorld, binary_name: String) {
    world.machine.execute_binary(&binary_name);
}

#[given(expr = "Alice's machine is executing the binary {string} and will not release it")]
fn machine_will_not_release(world: &mut MachineWorld, binary_name: String) {
    world
        .machine
        .execute_binary_that_cannot_be_displaced(&binary_name);
}

#[given(expr = "an earlier run superseded the binary {string} on Alice's machine")]
fn an_earlier_run_superseded(world: &mut MachineWorld, binary_name: String) {
    world.machine.leave_superseded_image(&binary_name);
}

#[given(expr = "an earlier run superseded the configurator's own binary on Alice's machine")]
fn an_earlier_run_superseded_the_configurator(world: &mut MachineWorld) {
    world.machine.leave_a_superseded_image_of_the_configurator();
}

#[then(expr = "{int} resource(s) is/are reported as held")]
fn resources_reported_as_held(world: &mut MachineWorld, expected: usize) {
    assert_eq!(world.outcome().held.len(), expected);
}

#[then(expr = "{int} binary/binaries is/are superseded on Alice's machine")]
fn binaries_superseded(world: &mut MachineWorld, expected: usize) {
    assert_eq!(world.machine.superseded_image_count(), expected);
}

#[then(expr = "cargo was asked to install {int} time(s)")]
fn cargo_asked_to_install(world: &mut MachineWorld, expected: usize) {
    assert_eq!(world.machine.cargo_installs(), expected);
}

#[given(expr = "cargo installed {string} from content the workspace has since changed")]
fn installed_from_older_content(world: &mut MachineWorld, crate_name: String) {
    world.member(&crate_name).installed = InstalledState::At(content_named("what it held before"));
}

fn content_named(content: &str) -> Fingerprint {
    Fingerprint {
        crate_subtree: ObjectHash::from(content),
        workspace_manifest: ObjectHash::from("the workspace manifest"),
        lockfile: ObjectHash::from("the lockfile"),
        dependency_subtrees: BTreeMap::new(),
    }
}

#[when(expr = "Alice plans")]
async fn alice_plans(world: &mut MachineWorld) {
    world.publish_workspace();
    world.fingerprint_before = Some(world.machine.fingerprint());

    let report = world.open_a_report(RunKind::Plan);
    world.change_set = Some(
        plan(&world.desired_state(), &world.machine, &report)
            .await
            .unwrap()
            .0,
    );
    world.report = Some(report);
}

#[when(expr = "Alice plans twice")]
async fn alice_plans_twice(world: &mut MachineWorld) {
    world.publish_workspace();
    world.fingerprint_before = Some(world.machine.fingerprint());

    let report = world.open_a_report(RunKind::Plan);
    world.change_set = Some(
        plan(&world.desired_state(), &world.machine, &report)
            .await
            .unwrap()
            .0,
    );
    world.second_change_set = Some(
        plan(&world.desired_state(), &world.machine, &report)
            .await
            .unwrap()
            .0,
    );
    world.report = Some(report);
}

async fn alice_applies_once(world: &mut MachineWorld) {
    let report = world.open_a_report(RunKind::Apply);
    world.enactment = Some(
        apply(
            &world.desired_state(),
            &world.machine,
            &report,
            &world.answering,
            &world.self_replacement,
        )
        .await
        .unwrap(),
    );
    world.report = Some(report);
}

#[when(expr = "Alice applies")]
async fn alice_applies(world: &mut MachineWorld) {
    world.publish_workspace();
    world.fingerprint_before = Some(world.machine.fingerprint());

    alice_applies_once(world).await;
}

#[when(expr = "Alice applies twice")]
async fn alice_applies_twice(world: &mut MachineWorld) {
    world.publish_workspace();
    world.fingerprint_before = Some(world.machine.fingerprint());

    for _ in 0..2 {
        alice_applies_once(world).await;
    }
}

#[when(expr = "Alice withdraws the declaration of {string}")]
fn withdraw_declaration(world: &mut MachineWorld, name: String) {
    let withdrawn = ApplicationName::from(name.as_str());
    world.resources.retain(|resource| match resource {
        Resource::Application(Application::Installer(installer)) => installer.name != withdrawn,
        _ => true,
    });
}

#[when(expr = "Alice loads her configurations for a personal machine")]
async fn alice_loads_for_a_personal_machine(world: &mut MachineWorld) {
    alice_loads(world, MachineClass::Personal).await;
}

#[when(expr = "Alice applies her configurations for a personal machine")]
async fn alice_applies_her_configurations_for_a_personal_machine(world: &mut MachineWorld) {
    alice_loads(world, MachineClass::Personal).await;
    let desired_state = world.loaded.as_ref().unwrap_or_else(|| {
        panic!(
            "loading was refused: {}",
            world.loading_error.as_deref().unwrap_or_default()
        )
    });

    let report = world.open_a_report(RunKind::Apply);
    let enactment = apply(
        desired_state,
        &world.machine,
        &report,
        &world.answering,
        &world.self_replacement,
    )
    .await
    .unwrap();
    world.enactment = Some(enactment);
    world.report = Some(report);
}

#[when(expr = "Alice plans her configurations for a personal machine")]
async fn alice_plans_her_configurations_for_a_personal_machine(world: &mut MachineWorld) {
    alice_loads(world, MachineClass::Personal).await;
    let desired_state = world.loaded.as_ref().unwrap_or_else(|| {
        panic!(
            "loading was refused: {}",
            world.loading_error.as_deref().unwrap_or_default()
        )
    });

    let report = world.open_a_report(RunKind::Plan);
    world.change_set = Some(
        plan(desired_state, &world.machine, &report)
            .await
            .unwrap()
            .0,
    );
    world.report = Some(report);
}

async fn alice_loads(world: &mut MachineWorld, machine: MachineClass) {
    let directory = write_configurations(
        &world.documents,
        &world.stray_file_names,
        world.configurations_are_inside_a_checkout,
    );
    let checkout = directory
        .parent()
        .expect("configurations are written into a directory of a checkout")
        .to_path_buf();
    for held in &world.files_the_checkout_holds {
        world.machine.add_own_file(checkout.join(held));
    }
    for (link_path, held) in &world.links_already_into_the_checkout {
        world
            .machine
            .create_link(
                &world.machine.resolve_against_home(link_path),
                &checkout.join(held),
            )
            .expect("the fake machine links wherever it is asked");
    }
    let first_source = match world.configurations_are_named_relative_to_the_checkout {
        true => named_relative_to(&checkout, &directory),
        false => named_in_full(directory),
    };
    world.checkout = Some(checkout);

    let mut sources = vec![first_source];
    if !world.employers_documents.is_empty() {
        sources.push(named_in_full(write_configurations(
            &world.employers_documents,
            &[],
            true,
        )));
    }

    match load_desired_state(
        &sources,
        machine,
        Path::new(REPOSITORIES_ROOT),
        &GitHubAccess::new(),
    )
    .await
    {
        Ok(desired_state) => world.loaded = Some(desired_state),
        Err(error) => world.loading_error = Some(format!("{error:#}")),
    }
}

fn named_in_full(directory: PathBuf) -> ConfigurationSource {
    ConfigurationSource::LocalDirectory(
        AbsoluteDirectory::of(directory).expect("a temporary directory is absolute"),
    )
}

fn named_relative_to(working_directory: &Path, directory: &Path) -> ConfigurationSource {
    let relative = directory
        .strip_prefix(working_directory)
        .expect("the configurations are inside the working directory");
    ConfigurationSource::named(
        &format!("local:{}", relative.display()),
        &AbsoluteDirectory::of(working_directory.to_path_buf())
            .expect("a temporary directory is absolute"),
    )
    .expect("a relative local source names a directory under the working directory")
}

fn write_configurations(
    documents: &[String],
    stray_file_names: &[String],
    inside_a_checkout: bool,
) -> PathBuf {
    static NEXT_CHECKOUT_NUMBER: AtomicUsize = AtomicUsize::new(0);

    let checkout = env::temp_dir().join(format!(
        "dotfiles_loading_scenarios/{}/{}",
        process::id(),
        NEXT_CHECKOUT_NUMBER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&checkout);
    let directory = checkout.join("config");
    fs::create_dir_all(&directory).unwrap();
    if inside_a_checkout {
        fs::create_dir_all(checkout.join(".git")).unwrap();
    }

    for (position, contents) in documents.iter().enumerate() {
        fs::write(
            directory.join(format!("{position:02}.dotconfig.json")),
            contents,
        )
        .unwrap();
    }
    for file_name in stray_file_names {
        fs::write(directory.join(file_name), "not a configuration").unwrap();
    }

    directory
}

#[given(expr = "{int} runs have already been logged")]
fn earlier_runs_have_been_logged(world: &mut MachineWorld, count: usize) {
    for _ in 0..count {
        drop(world.open_a_report(RunKind::Apply));
    }
}

#[then(expr = "the log of Alice's run names {string}")]
fn the_log_names(world: &mut MachineWorld, expected: String) {
    let path = world
        .report
        .as_ref()
        .expect("the scenario has not run yet")
        .log_path();
    let written = fs::read_to_string(&path).unwrap();

    assert!(
        written.contains(&expected),
        "expected the log to name {expected:?}, got:\n{written}"
    );
}

#[then(expr = "{int} runs are logged")]
fn runs_are_logged(world: &mut MachineWorld, expected: usize) {
    assert_eq!(world.logged_runs(), expected);
}

#[then(expr = "the change set reports {int} change(s)")]
fn change_set_reports_changes(world: &mut MachineWorld, expected: usize) {
    assert_eq!(world.change_set().changes.len(), expected);
}

#[then(expr = "the change set reports {int} blocked resource(s)")]
fn change_set_reports_blocked(world: &mut MachineWorld, expected: usize) {
    assert_eq!(world.change_set().blocked.len(), expected);
}

#[then(expr = "the change set mentions {string}")]
fn change_set_mentions(world: &mut MachineWorld, text: String) {
    let rendered = world.change_set().to_string();
    assert!(
        rendered.contains(&text),
        "expected the change set to mention {text:?}, got:\n{rendered}"
    );
}

#[then(expr = "the run reports a notice mentioning {string}")]
fn the_run_reports_a_notice_mentioning(world: &mut MachineWorld, text: String) {
    let notices: Vec<String> = world
        .outcome()
        .notices
        .iter()
        .map(ToString::to_string)
        .collect();

    assert!(
        notices.iter().any(|notice| notice.contains(&text)),
        "{notices:?}"
    );
}

#[then(expr = "the change set reports the machine as converged")]
fn change_set_reports_converged(world: &mut MachineWorld) {
    assert!(world.change_set().is_converged());
}

#[then(expr = "the change set does not report the machine as converged")]
fn change_set_reports_unconverged(world: &mut MachineWorld) {
    assert!(!world.change_set().is_converged());
}

#[then(expr = "the change set lists the application before the symlink")]
fn application_precedes_symlink(world: &mut MachineWorld) {
    let kinds: Vec<_> = world
        .change_set()
        .changes
        .iter()
        .map(|change| change.resource.kind())
        .collect();
    let application = kinds
        .iter()
        .position(|kind| *kind == dotfiles_configurator::configuration::ResourceKind::Application);
    let symlink = kinds
        .iter()
        .position(|kind| *kind == dotfiles_configurator::configuration::ResourceKind::Symlink);

    assert!(
        application < symlink,
        "expected the application ahead of the symlink, got {kinds:?}"
    );
}

#[then(expr = "both change sets are the same")]
fn both_change_sets_are_the_same(world: &mut MachineWorld) {
    assert_eq!(world.change_set, world.second_change_set);
}

#[then(expr = "nothing on Alice's machine has changed")]
fn nothing_changed(world: &mut MachineWorld) {
    assert_eq!(
        world.fingerprint_before.as_deref(),
        Some(world.machine.fingerprint().as_str())
    );
}

#[then(expr = "the machine is reported as converged")]
fn machine_reported_converged(world: &mut MachineWorld) {
    assert!(world.outcome().is_converged(), "{}", world.outcome());
}

#[then(expr = "the machine is not reported as converged")]
fn machine_reported_unconverged(world: &mut MachineWorld) {
    assert!(!world.outcome().is_converged());
}

#[then(expr = "{int} resource(s) is/are reported as failed")]
fn resources_failed(world: &mut MachineWorld, expected: usize) {
    assert_eq!(world.outcome().failed.len(), expected);
}

#[then(expr = "{int} resource(s) is/are reported as blocked")]
fn resources_blocked(world: &mut MachineWorld, expected: usize) {
    assert_eq!(world.outcome().blocked.len(), expected);
}

#[then(expr = "{int} resource(s) is/are reported as not having taken")]
fn resources_unverified(world: &mut MachineWorld, expected: usize) {
    assert_eq!(world.outcome().unverified.len(), expected);
}

#[then(expr = "Alice's own file at {string} is still there")]
fn own_file_is_still_there(world: &mut MachineWorld, path: String) {
    let resolved = world.machine.resolve_against_home(Path::new(&path));
    assert!(
        world.machine.own_file_is_intact(&resolved),
        "{path} is no longer the file Alice put there"
    );
}

#[then(regex = r"^(.+) is installed on Alice's machine$")]
fn then_application_is_installed(world: &mut MachineWorld, name: String) {
    assert!(
        world
            .machine
            .application_is_installed(&ApplicationName::from(name.as_str()))
    );
}

#[then(regex = r"^(.+) is still installed on Alice's machine$")]
fn then_application_is_still_installed(world: &mut MachineWorld, name: String) {
    assert!(
        world
            .machine
            .application_is_installed(&ApplicationName::from(name.as_str()))
    );
}

#[then(regex = r"^installing (.+) was attempted (\d+) times?$")]
fn install_attempts(world: &mut MachineWorld, name: String, expected: usize) {
    assert_eq!(
        world
            .machine
            .install_attempts(&ApplicationName::from(name.as_str())),
        expected
    );
}

#[then(expr = "the command {string} was run {int} time(s)")]
fn declared_command_was_run(world: &mut MachineWorld, command: String, expected: usize) {
    assert_eq!(
        world.machine.times_the_declared_command_ran(&command),
        expected
    );
}

#[then(expr = "the dotfiles repository is cloned on Alice's machine")]
fn then_repository_is_cloned(world: &mut MachineWorld) {
    assert!(world.machine.dotfiles_repository_is_cloned());
}

#[then(expr = "the link {string} points into the dotfiles repository")]
fn link_points_into_repository(world: &mut MachineWorld, link_path: String) {
    let resolved = world.machine.resolve_against_home(Path::new(&link_path));
    let target = world
        .machine
        .link_at(&resolved)
        .unwrap_or_else(|| panic!("nothing is linked at {}", resolved.display()));

    assert!(
        target.starts_with(dotfiles_repository_path()),
        "expected the link to point into the dotfiles repository, got {}",
        target.display()
    );
}

#[then(expr = "the link {string} resolves to {string} in Alice's checkout")]
fn link_resolves_into_the_checkout(world: &mut MachineWorld, link_path: String, held: String) {
    let checkout = world.checkout.as_ref().expect("nothing was loaded");
    let link = world.machine.resolve_against_home(Path::new(&link_path));
    let link_directory = link.parent().expect("a link sits in a directory");

    let resolved = world
        .machine
        .link_at(&link)
        .map(|target| link_directory.join(target));

    assert_eq!(resolved, Some(checkout.join(held)));
}

#[then(expr = "the link {string} is planned as converged")]
fn link_is_planned_as_converged(world: &mut MachineWorld, link_path: String) {
    let change_set = world.change_set();
    let converged = change_set.converged.iter().any(|resource| {
        let Resource::Symlink(symlink) = resource.declared() else {
            return false;
        };
        symlink.link_path == Path::new(&link_path)
    });

    assert!(converged, "{change_set}");
}

#[then(expr = "loading is refused")]
fn loading_is_refused(world: &mut MachineWorld) {
    assert!(
        world.loading_error.is_some(),
        "loading succeeded when it should have been refused"
    );
}

#[then(expr = "no desired state is loaded")]
fn no_desired_state_is_loaded(world: &mut MachineWorld) {
    assert!(
        world.loaded.is_none(),
        "a desired state was loaded from configurations that could not all be read"
    );
}

#[then(expr = "the refusal mentions {string}")]
fn refusal_mentions(world: &mut MachineWorld, text: String) {
    let error = world
        .loading_error
        .as_ref()
        .expect("loading was not refused");
    assert!(
        error.contains(&text),
        "expected the refusal to mention {text:?}, got: {error}"
    );
}

#[then(expr = "the refusal mentions the generation the configuration needs")]
fn refusal_mentions_the_generation_needed(world: &mut MachineWorld) {
    refusal_mentions(
        world,
        format!("generation {}", BUILD_GENERATION.stepped_by(1)),
    );
}

#[then(expr = "the refusal mentions the generation this build is")]
fn refusal_mentions_the_generation_of_this_build(world: &mut MachineWorld) {
    refusal_mentions(world, format!("generation {BUILD_GENERATION}"));
}

#[then(expr = "{int} configuration(s) is/are waiting to be rewritten")]
fn configurations_waiting_to_be_rewritten(world: &mut MachineWorld, expected: usize) {
    let migrations = &world
        .loaded
        .as_ref()
        .expect("nothing was loaded")
        .migrations;

    assert_eq!(migrations.len(), expected, "{migrations:?}");
}

#[then(expr = "the desired state holds {int} symlink(s)")]
fn desired_state_holds_symlinks(world: &mut MachineWorld, expected: usize) {
    let symlinks = world
        .loaded
        .as_ref()
        .expect("nothing was loaded")
        .resources
        .iter()
        .filter(|resource| matches!(resource.declared(), Resource::Symlink(_)))
        .count();

    assert_eq!(symlinks, expected);
}

#[then(expr = "the desired state links {string}")]
fn desired_state_links(world: &mut MachineWorld, link_path: String) {
    let linked = world.linked_paths();
    assert!(
        linked.contains(&link_path),
        "expected {link_path:?} to be linked, got {linked:?}"
    );
}

#[then(expr = "the desired state does not link {string}")]
fn desired_state_does_not_link(world: &mut MachineWorld, link_path: String) {
    let linked = world.linked_paths();
    assert!(
        !linked.contains(&link_path),
        "expected {link_path:?} not to be linked, got {linked:?}"
    );
}

fn under_alices_home(world: &MachineWorld, path: &str) -> PathBuf {
    world.machine.home_directory().join(path)
}

fn inside_the_clone_of(owner_and_name: &str, path: &str) -> PathBuf {
    Path::new(REPOSITORIES_ROOT)
        .join("Personal")
        .join(named_repository(owner_and_name).repository.as_ref())
        .join(path)
}

fn a_variable_named(name: &str) -> VariableName {
    VariableName::try_from(name).expect("a name a configuration may set")
}

#[given(expr = "Alice declares the environment variable {string} as {string}")]
fn declare_variable(world: &mut MachineWorld, name: String, value: String) {
    world.resources.push(Resource::EnvironmentVariable(
        EnvironmentVariable::Variable(Variable {
            name: a_variable_named(&name),
            value: VariableValue::from(value.as_str()),
        }),
    ));
}

#[given(expr = "Alice declares the search path entry {string} under her home directory")]
fn declare_search_path_entry_under_home(world: &mut MachineWorld, path: String) {
    world.resources.push(Resource::EnvironmentVariable(
        EnvironmentVariable::SearchPathEntry(SearchPathEntry {
            directory: SearchPathDirectory::Home {
                path: PathBuf::from(path),
            },
        }),
    ));
}

#[given(expr = "Alice declares the search path entry {string} in the repository {string}")]
fn declare_search_path_entry_in_repository(
    world: &mut MachineWorld,
    path: String,
    owner_and_name: String,
) {
    world.resources.push(Resource::EnvironmentVariable(
        EnvironmentVariable::SearchPathEntry(SearchPathEntry {
            directory: SearchPathDirectory::Repository {
                repository: named_repository(&owner_and_name),
                path: PathBuf::from(path),
            },
        }),
    ));
}

#[given(expr = "{string} is set to {string} on Alice's machine")]
fn machine_already_sets_variable(world: &mut MachineWorld, name: String, value: String) {
    world.machine.hold_environment_variable(
        &a_variable_named(&name),
        &VariableValue::from(value.as_str()),
    );
}

#[then(expr = "{string} is set to {string} on Alice's machine")]
fn machine_now_sets_variable(world: &mut MachineWorld, name: String, value: String) {
    assert_eq!(
        world.machine.environment_variable(&a_variable_named(&name)),
        Some(VariableValue::from(value.as_str()))
    );
}

#[given(expr = "{string} is already on Alice's own search path")]
fn directory_is_on_alices_search_path(world: &mut MachineWorld, path: String) {
    let directory = under_alices_home(world, &path);
    world.machine.hold_user_search_path_entry(directory);
}

#[given(expr = "{string} is already on the machine-wide search path")]
fn directory_is_on_the_machine_search_path(world: &mut MachineWorld, path: String) {
    let directory = under_alices_home(world, &path);
    world.machine.hold_machine_search_path_entry(directory);
}

#[given(expr = "nothing is on Alice's search path")]
fn nothing_is_on_the_search_path(world: &mut MachineWorld) {
    world.machine.clear_the_search_path();
}

#[given(expr = "Alice's machine holds no manifest")]
fn the_machine_holds_no_manifest(world: &mut MachineWorld) {
    world.machine.forget_the_machine_manifest();
}

#[then(expr = "Alice's machine holds a manifest naming the repositories directory {string}")]
fn the_manifest_names_the_repositories_directory(world: &mut MachineWorld, leaf: String) {
    let expected = String::try_from(&MachineManifest {
        repositories_directory_path: Path::new(REPOSITORIES_ROOT).join(leaf),
        estates: Estates::new(),
    })
    .expect("a manifest that serialises");

    assert_eq!(world.machine.machine_manifest(), Some(expected));
}

#[then(expr = "the directory this program installs binaries into is on Alice's own search path")]
fn the_binaries_directory_is_on_alices_search_path(world: &mut MachineWorld) {
    let directory = world.machine.binaries_directory();
    let search_path = world.machine.user_search_path();

    assert!(search_path.contains(&directory), "{search_path:?}");
}

#[then(expr = "{string} is on Alice's own search path")]
fn directory_is_now_on_alices_search_path(world: &mut MachineWorld, path: String) {
    let directory = under_alices_home(world, &path);
    let search_path = world.machine.user_search_path();

    assert!(search_path.contains(&directory), "{search_path:?}");
}

#[then(expr = "{string} is on Alice's own search path exactly once")]
fn directory_is_on_alices_search_path_once(world: &mut MachineWorld, path: String) {
    let directory = under_alices_home(world, &path);
    let search_path = world.machine.user_search_path();
    let named = search_path
        .iter()
        .filter(|entry| *entry == &directory)
        .count();

    assert_eq!(named, 1, "{search_path:?}");
}

#[then(expr = "{string} is not on Alice's own search path")]
fn directory_is_not_on_alices_search_path(world: &mut MachineWorld, path: String) {
    let directory = under_alices_home(world, &path);
    let search_path = world.machine.user_search_path();

    assert!(!search_path.contains(&directory), "{search_path:?}");
}

#[then(expr = "{string} inside the clone of {string} is on Alice's own search path")]
fn directory_inside_a_clone_is_on_alices_search_path(
    world: &mut MachineWorld,
    path: String,
    owner_and_name: String,
) {
    let directory = inside_the_clone_of(&owner_and_name, &path);
    let search_path = world.machine.user_search_path();

    assert!(search_path.contains(&directory), "{search_path:?}");
}

#[given(expr = "Alice declines the change set")]
fn alice_declines(world: &mut MachineWorld) {
    world.answering = Answering::declining();
}

#[given(expr = "Alice has answered in advance")]
fn alice_answered_in_advance(world: &mut MachineWorld) {
    world.answering = Answering::of(Answer::InAdvance(
        Operator::of_this_run(true).expect("a run answered in advance is never refused"),
    ));
}

fn a_configuration_a_generation_behind() -> Migration {
    let configuration = Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Everywhere,
        github_account: GitHubAccount::from("Alice"),
        estate: None,
        workspaces: Vec::new(),
        resources: Vec::new(),
        notices: Vec::new(),
    };

    Migration::of(
        &migrated_configuration_path(),
        &configuration,
        BUILD_GENERATION.stepped_by(-1),
    )
    .expect("a configuration that can be written back")
}

fn migrated_configuration_path() -> PathBuf {
    dotfiles_repository_path()
        .join("config")
        .join("everywhere.dotconfig.json")
}

#[given(expr = "Alice's configuration is waiting to be rewritten a generation forward")]
fn a_configuration_is_waiting_to_be_rewritten(world: &mut MachineWorld) {
    world.migrations.push(a_configuration_a_generation_behind());
}

#[given(expr = "Alice's configuration is a generation back in a source that cannot be written")]
fn a_configuration_a_generation_back_cannot_be_rewritten(world: &mut MachineWorld) {
    world.announcements.push(Notice::SourceCannotBeRewritten {
        source: ConfigurationName::from("Alice/dotfiles/config/everywhere.dotconfig.json"),
        from: BUILD_GENERATION.stepped_by(-1),
    });
}

#[then(expr = "Alice's configuration was rewritten")]
fn the_configuration_was_rewritten(world: &mut MachineWorld) {
    assert!(world.machine.path_exists(&migrated_configuration_path()));
}

#[then(expr = "Alice's configuration was not rewritten")]
fn the_configuration_was_not_rewritten(world: &mut MachineWorld) {
    assert!(!world.machine.path_exists(&migrated_configuration_path()));
}

#[then(expr = "the run is reported as having done nothing")]
fn the_run_did_nothing(world: &mut MachineWorld) {
    assert!(world.was_declined());
}

#[then(expr = "the run asked once")]
fn the_run_asked_once(world: &mut MachineWorld) {
    assert_eq!(world.answering.questions_put.get(), 1);
}

#[then(expr = "the run asked nothing")]
fn the_run_asked_nothing(world: &mut MachineWorld) {
    assert_eq!(world.answering.questions_put.get(), 0);
}

#[then(expr = "Alice was shown {string} before it was converged")]
fn alice_was_shown_before_converging(world: &mut MachineWorld, resource: String) {
    let path = world
        .report
        .as_ref()
        .expect("the scenario has not run yet")
        .log_path();
    let written = fs::read_to_string(&path).unwrap();

    let shown = written
        .find(&format!("change  {resource}"))
        .unwrap_or_else(|| {
            panic!("the change set naming {resource:?} was never shown:\n{written}")
        });
    let converged = written
        .find(&format!("converged {resource}"))
        .unwrap_or_else(|| panic!("{resource:?} was never converged:\n{written}"));

    assert!(shown < converged, "{written}");
}

#[tokio::main]
async fn main() {
    MachineWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("tests/features")
        .await;
}

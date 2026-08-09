#![allow(clippy::disallowed_macros)]

#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    cucumber::{World, given, then, when},
    dotfiles_configurator::{
        configuration::{
            Application, ApplicationName, ApplicationSource, ArchiveEntry, AssetPattern,
            BEYOND_BUILD_GENERATION, BUILD_GENERATION, CargoWorkspace, Context, CrateName,
            DesiredState, GitHubRepository, Installer, MachineSettings, Notice, PresenceCheck,
            ReleasedBinary, RepositoryName, RepositoryOwner, Resource, Shell, Symlink, VersionWord,
        },
        configuration_source::{ConfigurationSource, load_desired_state},
        convergence::{ApplyOutcome, ChangeSet, apply::apply, plan},
        machine::{
            ReadMachine, Tool,
            release_reading::{ReleaseAsset, ReleaseReading},
            workspace_reading::{
                BinaryName, Fingerprint, MemberReading, ObjectHash, Revision, WorkspaceReading,
            },
        },
        reporting::{RunKind, RunReport},
        version::Version,
    },
    fake_machine::FakeMachine,
    std::{
        collections::{BTreeMap, BTreeSet},
        env, fs,
        num::NonZeroUsize,
        path::{Path, PathBuf},
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
    notices: Vec<Notice>,
    /// Configurations as they are written down, for the scenarios about loading them.
    documents: Vec<String>,
    /// Files kept beside the configurations that are not configurations themselves.
    stray_file_names: Vec<String>,
    change_set: Option<ChangeSet>,
    second_change_set: Option<ChangeSet>,
    outcome: Option<ApplyOutcome>,
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
            documents: Vec::new(),
            stray_file_names: Vec::new(),
            change_set: None,
            second_change_set: None,
            outcome: None,
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
        DesiredState {
            machine: machine_settings(),
            workspaces: self.workspaces.clone(),
            resources: self.resources.clone(),
            notices: self.notices.clone(),
        }
    }

    fn linked_paths(&self) -> Vec<String> {
        self.loaded
            .as_ref()
            .expect("nothing was loaded")
            .resources
            .iter()
            .filter_map(|resource| match resource {
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
            machine_settings().dotfiles_repository_path(),
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

    fn outcome(&self) -> &ApplyOutcome {
        self.outcome
            .as_ref()
            .expect("the scenario has not applied yet")
    }
}

fn machine_settings() -> MachineSettings {
    MachineSettings {
        repositories_directory_path: PathBuf::from("/repositories"),
        github_username: "Alice".to_owned(),
        dotfiles_repository: GitHubRepository {
            owner: RepositoryOwner::from("Alice"),
            repository: RepositoryName::from("dotfiles"),
        },
    }
}

/// An application whose presence is read as "a program of that name is on the path", which is
/// what most of the live declarations use.
fn application(name: &str) -> Application {
    Application::Installer(Installer {
        name: ApplicationName::from(name),
        source: ApplicationSource::Uri {
            uri: Url::parse("https://example.invalid/installer.exe").unwrap(),
            installer_file_name: format!("{name}.exe"),
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

fn named_repository(owner_and_name: &str) -> GitHubRepository {
    let (owner, repository) = owner_and_name
        .split_once('/')
        .expect("a repository is written owner/name");
    GitHubRepository {
        owner: RepositoryOwner::from(owner),
        repository: RepositoryName::from(repository),
    }
}

fn released_binary(entry: &str, owner_and_name: &str) -> ReleasedBinary {
    ReleasedBinary {
        repository: named_repository(owner_and_name),
        asset: AssetPattern::EndsWith(".zip".to_owned()),
        entry: ArchiveEntry::try_from(entry.to_owned()).unwrap(),
        version_arguments: vec!["--version".to_owned()],
        version_word: VersionWord::from(NonZeroUsize::new(2).unwrap()),
    }
}

#[given(expr = "Alice declares the released binary {string} from {string}")]
fn declare_released_binary(world: &mut MachineWorld, entry: String, owner_and_name: String) {
    world
        .resources
        .push(Resource::Application(Application::ReleasedBinary(
            released_binary(&entry, &owner_and_name),
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

#[given(expr = "Alice declares the notice {string}")]
fn declare_notice(world: &mut MachineWorld, message: String) {
    world.notices.push(Notice::from(message.as_str()));
}

#[given(expr = "Alice declares the symlink {string} at {string}")]
fn declare_symlink(world: &mut MachineWorld, source_path: String, link_path: String) {
    world.resources.push(Resource::Symlink(Symlink {
        source_path: PathBuf::from(source_path),
        link_path: PathBuf::from(link_path),
    }));
}

#[given(expr = "Alice declares the dotfiles repository")]
fn declare_dotfiles_repository(world: &mut MachineWorld) {
    world
        .resources
        .push(Resource::Repository(machine_settings().dotfiles_repository));
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

#[given(regex = r"^(.+) is installed on Alice's machine$")]
fn application_is_installed(world: &mut MachineWorld, name: String) {
    world
        .machine
        .install_application(&ApplicationName::from(name.as_str()));
}

#[given(regex = r"^(.+) is not installed on Alice's machine$")]
fn application_is_not_installed(_world: &mut MachineWorld, _name: String) {
    // A fresh machine has nothing installed; the step is here so the scenario says so.
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

#[given(expr = "the dotfiles repository holds {string}")]
fn repository_holds(world: &mut MachineWorld, path: String) {
    world.machine.repository_holds(PathBuf::from(path));
}

#[given(expr = "the dotfiles repository has been cloned on Alice's machine")]
fn dotfiles_repository_is_cloned(world: &mut MachineWorld) {
    world.machine.clone_dotfiles_repository();
}

#[given(expr = "the dotfiles repository has not been cloned on Alice's machine")]
fn dotfiles_repository_is_not_cloned(_world: &mut MachineWorld) {
    // A fresh machine holds no clone; the step is here so the scenario says so.
}

#[given(expr = "Alice has a configuration declaring version {string}")]
fn configuration_with_version(world: &mut MachineWorld, version: String) {
    world.documents.push(document(&version, "everywhere", "[]"));
}

#[given(expr = "Alice has a configuration for work machines declaring version {string}")]
fn work_configuration_with_version(world: &mut MachineWorld, version: String) {
    world.documents.push(document(&version, "work", "[]"));
}

#[given(expr = "Alice has a configuration declaring a generation beyond this build")]
fn configuration_beyond_this_build(world: &mut MachineWorld) {
    world.documents.push(document(
        &BEYOND_BUILD_GENERATION.to_string(),
        "everywhere",
        "[]",
    ));
}

#[given(
    expr = "Alice has a configuration for every machine declaring version {string} linking \
            {string} to {string}"
)]
fn configuration_for_every_machine_with_version_linking(
    world: &mut MachineWorld,
    version: String,
    link_path: String,
    source_path: String,
) {
    world.documents.push(document(
        &version,
        "everywhere",
        &symlink(&link_path, &source_path),
    ));
}

#[given(expr = "Alice keeps a {string} alongside her configurations")]
fn stray_file_alongside_configurations(world: &mut MachineWorld, file_name: String) {
    world.stray_file_names.push(file_name);
}

#[given(expr = "Alice has a configuration that declares no machines it is for")]
fn configuration_without_a_context(world: &mut MachineWorld) {
    world.documents.push(format!(
        r#"{{
            "version": "{BUILD_GENERATION}",
            "machine": {{
                "repositories_directory_path": "/repositories",
                "github_username": "Alice",
                "dotfiles_repository": {{ "owner": "Alice", "repository": "dotfiles" }}
            }},
            "resources": []
        }}"#
    ));
}

#[given(
    expr = "Alice has a configuration whose machine settings omit the repositories directory path"
)]
fn configuration_without_a_repositories_directory_path(world: &mut MachineWorld) {
    world.documents.push(format!(
        r#"{{
            "version": "{BUILD_GENERATION}",
            "applies_to": "everywhere",
            "machine": {{
                "github_username": "Alice",
                "dotfiles_repository": {{ "owner": "Alice", "repository": "dotfiles" }}
            }},
            "resources": []
        }}"#
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
            "machine": {{
                "repositories_directory_path": "/repositories",
                "github_username": "Alice",
                "dotfiles_repository": {{ "owner": "Alice", "repository": "dotfiles" }}
            }},
            "resources": {resources}
        }}"#
    )
}

#[given(expr = "Alice declares the cargo workspace in the dotfiles repository")]
fn declare_cargo_workspace(world: &mut MachineWorld) {
    world.workspaces.push(CargoWorkspace {
        repository: machine_settings().dotfiles_repository,
    });
}

#[given(expr = "the workspace holds the crate {string}")]
fn workspace_holds_crate(world: &mut MachineWorld, crate_name: String) {
    world.members.insert(
        CrateName::from(crate_name.as_str()),
        MemberReading {
            desired: content_named("what the workspace holds now"),
            installed: None,
            absent_binaries: BTreeSet::new(),
        },
    );
}

#[given(expr = "cargo installed {string} from the content the workspace holds now")]
fn installed_from_current_content(world: &mut MachineWorld, crate_name: String) {
    let member = world.member(&crate_name);
    member.installed = Some(member.desired.clone());
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
    world.member(&crate_name).installed = Some(content_named("what it held before"));
}

fn content_named(content: &str) -> Fingerprint {
    Fingerprint {
        crate_subtree: ObjectHash::from(content),
        workspace_manifest: ObjectHash::from("the workspace manifest"),
        lockfile: ObjectHash::from("the lockfile"),
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
            .unwrap(),
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
            .unwrap(),
    );
    world.second_change_set = Some(
        plan(&world.desired_state(), &world.machine, &report)
            .await
            .unwrap(),
    );
    world.report = Some(report);
}

#[when(expr = "Alice applies")]
async fn alice_applies(world: &mut MachineWorld) {
    world.publish_workspace();
    world.fingerprint_before = Some(world.machine.fingerprint());

    let report = world.open_a_report(RunKind::Apply);
    world.outcome = Some(
        apply(&world.desired_state(), &world.machine, &report)
            .await
            .unwrap(),
    );
    world.report = Some(report);
}

#[when(expr = "Alice applies twice")]
async fn alice_applies_twice(world: &mut MachineWorld) {
    world.publish_workspace();
    world.fingerprint_before = Some(world.machine.fingerprint());

    for _ in 0..2 {
        let report = world.open_a_report(RunKind::Apply);
        world.outcome = Some(
            apply(&world.desired_state(), &world.machine, &report)
                .await
                .unwrap(),
        );
        world.report = Some(report);
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
    alice_loads(world, Context::Personal).await;
}

#[when(expr = "Alice loads her configurations for a machine of no class")]
async fn alice_loads_for_a_machine_of_no_class(world: &mut MachineWorld) {
    alice_loads(world, Context::Everywhere).await;
}

async fn alice_loads(world: &mut MachineWorld, context: Context) {
    let directory = configuration_directory();
    for (position, contents) in world.documents.iter().enumerate() {
        fs::write(
            directory.join(format!("{position:02}.dotconfig.json")),
            contents,
        )
        .unwrap();
    }
    for file_name in &world.stray_file_names {
        fs::write(directory.join(file_name), "not a configuration").unwrap();
    }

    match load_desired_state(&[ConfigurationSource::LocalDirectory(directory)], context).await {
        Ok(desired_state) => world.loaded = Some(desired_state),
        Err(error) => world.loading_error = Some(format!("{error:#}")),
    }
}

fn configuration_directory() -> PathBuf {
    static NEXT_DIRECTORY_NUMBER: AtomicUsize = AtomicUsize::new(0);

    let directory = env::temp_dir().join(format!(
        "dotfiles_loading_scenarios/{}",
        NEXT_DIRECTORY_NUMBER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
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
        .log_path()
        .expect("the run wrote no log");
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
        target.starts_with("/repositories/dotfiles"),
        "expected the link to point into the dotfiles repository, got {}",
        target.display()
    );
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
    refusal_mentions(world, format!("generation {BEYOND_BUILD_GENERATION}"));
}

#[then(expr = "the refusal mentions the generation this build is")]
fn refusal_mentions_the_generation_of_this_build(world: &mut MachineWorld) {
    refusal_mentions(world, format!("generation {BUILD_GENERATION}"));
}

#[then(expr = "the desired state holds {int} symlink(s)")]
fn desired_state_holds_symlinks(world: &mut MachineWorld, expected: usize) {
    let symlinks = world
        .loaded
        .as_ref()
        .expect("nothing was loaded")
        .resources
        .iter()
        .filter(|resource| matches!(resource, Resource::Symlink(_)))
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

#[tokio::main]
async fn main() {
    MachineWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("tests/features")
        .await;
}

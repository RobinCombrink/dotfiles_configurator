#![allow(clippy::disallowed_macros)]

#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    cucumber::{World, given, then, when},
    dotfiles::{
        configuration::{
            Application, ApplicationName, ApplicationSource, DesiredState, GitHubRepository,
            MachineSettings, Notice, PresenceCheck, RepositoryName, RepositoryOwner, Resource,
            Shell, Symlink,
        },
        configuration_source::{ConfigurationSource, load_desired_state},
        convergence::{ApplyOutcome, ChangeSet, apply::apply, plan},
        machine::{ReadMachine, Tool},
    },
    fake_machine::FakeMachine,
    std::{
        env, fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    },
    url::Url,
};

#[derive(Debug, World)]
#[world(init = Self::new)]
struct MachineWorld {
    machine: FakeMachine,
    resources: Vec<Resource>,
    notices: Vec<Notice>,
    /// Configurations as they are written down, for the scenarios about loading them.
    documents: Vec<String>,
    change_set: Option<ChangeSet>,
    second_change_set: Option<ChangeSet>,
    outcome: Option<ApplyOutcome>,
    fingerprint_before: Option<String>,
    loading_error: Option<String>,
    loaded: Option<DesiredState>,
}

impl MachineWorld {
    fn new() -> Self {
        Self {
            machine: FakeMachine::default(),
            resources: Vec::new(),
            notices: Vec::new(),
            documents: Vec::new(),
            change_set: None,
            second_change_set: None,
            outcome: None,
            fingerprint_before: None,
            loading_error: None,
            loaded: None,
        }
    }

    fn desired_state(&self) -> DesiredState {
        DesiredState {
            machine: machine_settings(),
            resources: self.resources.clone(),
            notices: self.notices.clone(),
        }
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
    Application {
        name: ApplicationName::from(name),
        source: ApplicationSource::Uri {
            uri: Url::parse("https://example.invalid/installer.exe").unwrap(),
            installer_file_name: format!("{name}.exe"),
        },
        presence_check: PresenceCheck::CommandOnPath {
            command: name.to_owned(),
        },
    }
}

#[given(expr = "Alice declares the application {string}")]
fn declare_application(world: &mut MachineWorld, name: String) {
    world
        .resources
        .push(Resource::Application(application(&name)));
}

#[given(expr = "Alice declares the winget package {string}")]
fn declare_winget_package(world: &mut MachineWorld, id: String) {
    world
        .resources
        .push(Resource::Package(dotfiles::configuration::Package::Winget(
            dotfiles::configuration::WingetPackage { id: id.into() },
        )));
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
    world
        .resources
        .push(Resource::Command(dotfiles::configuration::Command {
            shell: Shell::Bash,
            args: vec![command],
            presence_check: None,
        }));
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

#[given(expr = "Alice has a configuration declaring format version {string}")]
fn configuration_with_version(world: &mut MachineWorld, version: String) {
    world.documents.push(document(&version, "[]"));
}

#[given(
    expr = "Alice has a configuration whose machine settings omit the repositories directory path"
)]
fn configuration_without_a_repositories_directory_path(world: &mut MachineWorld) {
    world.documents.push(
        r#"{
            "version": "2",
            "machine": {
                "github_username": "Alice",
                "dotfiles_repository": { "owner": "Alice", "repository": "dotfiles" }
            },
            "resources": []
        }"#
        .to_owned(),
    );
}

#[given(expr = "Alice has a configuration linking {string} to {string}")]
fn configuration_linking(world: &mut MachineWorld, link_path: String, source_path: String) {
    let resources = format!(
        r#"[{{ "kind": "symlink", "source_path": "{source_path}", "link_path": "{link_path}" }}]"#
    );
    world.documents.push(document("2", &resources));
}

fn document(version: &str, resources: &str) -> String {
    format!(
        r#"{{
            "version": "{version}",
            "machine": {{
                "repositories_directory_path": "/repositories",
                "github_username": "Alice",
                "dotfiles_repository": {{ "owner": "Alice", "repository": "dotfiles" }}
            }},
            "resources": {resources}
        }}"#
    )
}

#[when(expr = "Alice plans")]
fn alice_plans(world: &mut MachineWorld) {
    world.fingerprint_before = Some(world.machine.fingerprint());
    world.change_set = Some(plan(&world.desired_state(), &world.machine));
}

#[when(expr = "Alice plans twice")]
fn alice_plans_twice(world: &mut MachineWorld) {
    world.fingerprint_before = Some(world.machine.fingerprint());
    world.change_set = Some(plan(&world.desired_state(), &world.machine));
    world.second_change_set = Some(plan(&world.desired_state(), &world.machine));
}

#[when(expr = "Alice applies")]
async fn alice_applies(world: &mut MachineWorld) {
    world.fingerprint_before = Some(world.machine.fingerprint());
    world.outcome = Some(apply(&world.desired_state(), &world.machine).await);
}

#[when(expr = "Alice withdraws the declaration of {string}")]
fn withdraw_declaration(world: &mut MachineWorld, name: String) {
    let withdrawn = ApplicationName::from(name.as_str());
    world.resources.retain(|resource| match resource {
        Resource::Application(application) => application.name != withdrawn,
        _ => true,
    });
}

#[when(expr = "Alice loads her configurations")]
async fn alice_loads(world: &mut MachineWorld) {
    let directory = configuration_directory();
    for (position, contents) in world.documents.iter().enumerate() {
        fs::write(
            directory.join(format!("{position:02}.dotconfig.json")),
            contents,
        )
        .unwrap();
    }

    match load_desired_state(&[ConfigurationSource::LocalDirectory(directory)]).await {
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
        .position(|kind| *kind == dotfiles::configuration::ResourceKind::Application);
    let symlink = kinds
        .iter()
        .position(|kind| *kind == dotfiles::configuration::ResourceKind::Symlink);

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

#[tokio::main]
async fn main() {
    MachineWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("tests/features")
        .await;
}

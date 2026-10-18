use {
    super::*,
    crate::configuration::{
        BUILD_GENERATION, MachineClass, RepositoryName, RepositoryOwner, Unreadable,
    },
};

const REPOSITORIES_ROOT: &str = "/repositories";
const HOME_DIRECTORY: &str = HOME_DIRECTORY_IN_TESTS;

fn dotfiles() -> GitHubRepository {
    GitHubRepository {
        owner: RepositoryOwner::from("Alice"),
        repository: RepositoryName::from("dotfiles"),
    }
}

fn document(applies_to: &str, body: &str) -> Configuration {
    document_acting_as("Alice", applies_to, body)
}

fn document_acting_as(account: &str, applies_to: &str, body: &str) -> Configuration {
    let written = format!(
        r#"{{ "version": "{BUILD_GENERATION}", "applies_to": "{applies_to}",
               "github_account": "{account}", {body} }}"#
    );
    crate::configuration::parse_configuration(
        &written,
        &ConfigurationName::from("the test configuration"),
    )
    .unwrap_or_else(|refusal: Unreadable| panic!("{refusal}"))
    .configuration
}

fn read_from_the_repository_of(
    account: &str,
    applies_to: &str,
    body: &str,
) -> ResolvedConfiguration {
    ResolvedConfiguration::read(
        ConfigurationName::from(applies_to),
        document_acting_as(account, applies_to, body),
        SourceLocation::Repository(GitHubRepository {
            owner: RepositoryOwner::from(account),
            repository: RepositoryName::from("dotfiles"),
        }),
        Path::new(REPOSITORIES_ROOT),
    )
}

fn read_from_the_dotfiles_repository(applies_to: &str, body: &str) -> ResolvedConfiguration {
    read_from_the_repository_of("Alice", applies_to, body)
}

fn read_from_a_checkout(applies_to: &str, body: &str) -> ResolvedConfiguration {
    ResolvedConfiguration::read(
        ConfigurationName::from(applies_to),
        document(applies_to, body),
        SourceLocation::Checkout(
            AbsoluteDirectory::of(std::env::temp_dir().join("checkout"))
                .expect("a temporary directory is absolute"),
        ),
        Path::new(REPOSITORIES_ROOT),
    )
}

fn merged(configurations: Vec<ResolvedConfiguration>) -> Result<DesiredState, Irreconcilable> {
    DesiredState::of(
        configurations,
        RenderedManifest::try_from(crate::configuration::MachineManifest {
            repositories_directory_path: crate::configuration::RepositoriesDirectory::try_from(
                Path::new(REPOSITORIES_ROOT)
                    .join(Context::from(MachineClass::Personal).repositories_leaf()),
            )
            .expect("a rooted directory"),
            estates: crate::configuration::Estates::default(),
            recorded_run: crate::configuration::RecordedRun {
                class: MachineClass::Personal,
                configuration_sources: Vec::new(),
            },
        })
        .expect("a manifest naming a directory spelled in UTF-8 renders"),
        Path::new(HOME_DIRECTORY),
    )
}

const SYMLINK: &str = r#""resources": [{
        "kind": "symlink", "source_path": "gitconfig/.gitconfig", "link_path": ".gitconfig"
    }]"#;

const EMPTY: &str = r#""resources": []"#;

fn a_readable_set(personal: &str) -> Result<DesiredState, Irreconcilable> {
    merged(vec![
        read_from_the_dotfiles_repository("everywhere", EMPTY),
        read_from_the_dotfiles_repository("personal", personal),
    ])
}

#[test]
fn a_configuration_read_from_a_repository_contributes_that_repository_as_a_clone() {
    let desired_state = a_readable_set(EMPTY).unwrap();

    assert_eq!(
        desired_state
            .other_than_what_every_change_set_carries()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["repository Alice/dotfiles".to_owned()]
    );
}

#[test]
fn every_change_set_carries_the_machine_manifest() {
    let desired_state = a_readable_set(EMPTY).unwrap();

    assert!(
        desired_state
            .undeclared
            .contains(&Identity::MachineManifest),
        "{:?}",
        desired_state.undeclared
    );
}

#[test]
fn the_machine_manifest_names_the_repositories_directory_of_the_class_the_run_is_for() {
    let desired_state = a_readable_set(EMPTY).unwrap();

    let manifests: Vec<String> = desired_state
        .resources
        .iter()
        .filter(|resource| {
            resource.identity(Path::new(HOME_DIRECTORY)) == Some(Identity::MachineManifest)
        })
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        manifests,
        vec![format!(
            "registration machine manifest naming {}",
            Path::new(REPOSITORIES_ROOT)
                .join(Context::from(MachineClass::Personal).repositories_leaf())
                .display()
        )]
    );
}

#[test]
fn every_change_set_carries_the_currency_of_the_build_producing_it() {
    let desired_state = a_readable_set(EMPTY).unwrap();

    let claimed: Vec<Identity> = desired_state
        .resources
        .iter()
        .filter_map(|resource| resource.identity(Path::new(HOME_DIRECTORY)))
        .collect();
    assert!(
        claimed.contains(&Identity::InstalledBinary(
            currency::own_currency().installed_name()
        )),
        "{claimed:?}"
    );
}

#[test]
fn the_update_of_the_running_build_is_enacted_before_every_other_resource() {
    let desired_state = merged(vec![
        read_from_the_dotfiles_repository("everywhere", SYMLINK),
        read_from_the_dotfiles_repository("personal", EMPTY),
    ])
    .unwrap();

    let mut ordered = desired_state.resources.clone();
    ordered.reverse();
    ordered.sort_by_key(ResolvedResource::precedence);

    assert!(
        ordered[0].replaces_the_running_build(),
        "{:?}",
        ordered.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
}

#[test]
fn every_change_set_carries_the_directory_this_program_installs_binaries_into() {
    let desired_state = a_readable_set(EMPTY).unwrap();

    let claimed: Vec<Identity> = desired_state
        .resources
        .iter()
        .filter_map(|resource| resource.identity(Path::new(HOME_DIRECTORY)))
        .collect();
    assert!(
        claimed.contains(&Identity::SearchPathEntry(
            SearchPathDirectory::ToolBinaries
        )),
        "{claimed:?}"
    );
}

#[test]
fn a_configuration_read_out_of_a_checkout_waits_for_no_clone() {
    let desired_state = merged(vec![
        read_from_a_checkout("everywhere", EMPTY),
        read_from_a_checkout("personal", SYMLINK),
    ])
    .unwrap();

    let symlink = desired_state
        .resources
        .iter()
        .find(|resource| resource.kind() == ResourceKind::Symlink)
        .expect("the personal configuration declared a symlink");
    assert!(
        symlink.requirements().is_empty(),
        "{:?}",
        symlink.requirements()
    );
}

#[test]
fn a_symlink_out_of_a_clone_waits_for_the_repository_that_holds_it() {
    let desired_state = a_readable_set(SYMLINK).unwrap();

    let symlink = desired_state
        .resources
        .iter()
        .find(|resource| resource.kind() == ResourceKind::Symlink)
        .expect("the personal configuration declared a symlink");
    assert_eq!(
        symlink.requirements(),
        vec![Requirement::DotfilesRepository(dotfiles())]
    );
}

#[test]
fn a_configuration_for_work_clones_into_the_work_tree() {
    let work = ResolvedConfiguration::read(
        ConfigurationName::from("work"),
        document("work", EMPTY),
        SourceLocation::Repository(dotfiles()),
        Path::new(REPOSITORIES_ROOT),
    );

    let desired_state = merged(vec![
        read_from_the_dotfiles_repository("everywhere", EMPTY),
        work,
    ])
    .unwrap();

    let claimed: Vec<Identity> = desired_state
        .resources
        .iter()
        .filter_map(|resource| resource.identity(Path::new(HOME_DIRECTORY)))
        .collect();
    assert!(
        claimed.contains(&Identity::ClonedRepository(
            Path::new(REPOSITORIES_ROOT).join("Work").join("dotfiles")
        )),
        "{claimed:?}"
    );
}

#[test]
fn a_set_holding_no_configuration_for_every_machine_is_refused() {
    let error = merged(vec![read_from_the_dotfiles_repository("personal", EMPTY)]).unwrap_err();

    assert!(error.to_string().contains("every machine"), "{error}");
}

#[test]
fn a_set_holding_no_configuration_for_this_machines_class_is_refused() {
    let error = merged(vec![read_from_the_dotfiles_repository("everywhere", EMPTY)]).unwrap_err();

    assert!(error.to_string().contains("machine's class"), "{error}");
}

#[test]
fn two_identical_claims_on_one_fact_collapse_to_a_single_resource() {
    let desired_state = merged(vec![
        read_from_the_dotfiles_repository("everywhere", SYMLINK),
        read_from_the_dotfiles_repository("personal", SYMLINK),
    ])
    .unwrap();

    assert_eq!(
        desired_state
            .resources
            .iter()
            .filter(|resource| resource.kind() == ResourceKind::Symlink)
            .count(),
        1
    );
}

#[test]
fn two_conflicting_claims_on_one_fact_are_rejected_naming_both_sources() {
    let elsewhere = r#""resources": [{
            "kind": "symlink", "source_path": "other/.gitconfig", "link_path": ".gitconfig"
        }]"#;

    let error = merged(vec![
        read_from_the_dotfiles_repository("everywhere", SYMLINK),
        read_from_the_dotfiles_repository("personal", elsewhere),
    ])
    .unwrap_err();

    let message = error.to_string();
    assert!(
        message.contains("everywhere") && message.contains("personal"),
        "{message}"
    );
}

#[test]
fn commands_never_conflict_with_one_another_because_they_claim_nothing() {
    let command = r#""resources": [
            { "kind": "command", "shell": "command_prompt", "args": ["wsl"] }
        ]"#;

    let desired_state = merged(vec![
        read_from_the_dotfiles_repository("everywhere", command),
        read_from_the_dotfiles_repository("personal", command),
    ])
    .unwrap();

    assert_eq!(
        desired_state
            .resources
            .iter()
            .filter(|resource| resource.kind() == ResourceKind::Command)
            .count(),
        2
    );
}

#[test]
fn a_workspace_declared_by_two_configurations_collapses_to_one() {
    let workspace = r#""workspaces": [
            { "repository": { "owner": "Alice", "repository": "dotfiles" } }
        ]"#;

    let desired_state = merged(vec![
        read_from_the_dotfiles_repository("everywhere", workspace),
        read_from_the_dotfiles_repository("personal", workspace),
    ])
    .unwrap();

    assert_eq!(desired_state.workspaces.len(), 1);
}

#[test]
fn a_command_declares_no_identity() {
    let desired_state = a_readable_set(
        r#""resources": [{ "kind": "command", "shell": "command_prompt", "args": ["wsl"] }]"#,
    )
    .unwrap();

    let command = desired_state
        .resources
        .iter()
        .find(|resource| resource.kind() == ResourceKind::Command)
        .expect("the personal configuration declared a command");
    assert_eq!(command.identity(Path::new(HOME_DIRECTORY)), None);
}

#[test]
fn a_released_binary_claims_the_name_its_entry_carries_rather_than_the_whole_path() {
    let desired_state = a_readable_set(
        r#""resources": [{
                "kind": "application", "shape": "released_binary",
                "repository": { "owner": "BurntSushi", "repository": "ripgrep" },
                "asset": { "match": "ends_with", "value": ".zip" },
                "entry": "bin/rg",
                "version_word": 2
            }]"#,
    )
    .unwrap();

    let binary = desired_state
        .other_than_what_every_change_set_carries()
        .into_iter()
        .find(|resource| resource.kind() == ResourceKind::Application)
        .expect("the personal configuration declared a released binary");
    assert_eq!(
        binary.identity(Path::new(HOME_DIRECTORY)),
        Some(Identity::InstalledBinary(
            crate::configuration::BinaryName::from("rg")
        ))
    );
}

#[test]
fn a_resource_acts_as_the_account_of_the_configuration_that_declared_it() {
    let desired_state = merged(vec![
        read_from_the_repository_of("Alice", "everywhere", EMPTY),
        read_from_the_repository_of("Employer", "work", SYMLINK),
    ])
    .unwrap();

    let symlink = desired_state
        .other_than_what_every_change_set_carries()
        .into_iter()
        .find(|resource| resource.kind() == ResourceKind::Symlink)
        .expect("the work configuration declared a symlink");

    assert_eq!(*symlink.account(), GitHubAccount::from("Employer"));
}

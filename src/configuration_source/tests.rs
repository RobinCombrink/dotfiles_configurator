use super::*;
use crate::configuration::{BUILD_GENERATION, OLDEST_READABLE_GENERATION};
use std::{env, fs::File, io::Write, process};

#[test]
fn a_local_source_resolves_its_files_root_by_walking_up_to_a_checkout() {
    let checkout = temporary_checkout("files_root");

    let location = ConfigurationSource::LocalDirectory(absolute(checkout.join("config")))
        .files_come_from()
        .unwrap();

    assert_eq!(location, SourceLocation::Checkout(absolute(checkout)));
}

#[test]
fn a_local_source_named_up_through_a_checkout_inside_another_resolves_to_the_outer_one() {
    let outer = temporary_checkout("named_up_through_a_checkout");
    let inner = outer.join("tools").join("inner");
    fs::create_dir_all(inner.join(".git")).unwrap();
    fs::create_dir_all(outer.join("tools").join("config")).unwrap();

    let location = ConfigurationSource::named("local:../config", &absolute(inner))
        .unwrap()
        .files_come_from()
        .unwrap();

    assert_eq!(location, SourceLocation::Checkout(absolute(outer)));
}

#[test]
fn a_local_source_is_read_from_the_directory_its_parent_and_current_steps_lead_to() {
    let source = ConfigurationSource::named(
        "local:./../beside/./config",
        &absolute(where_alice_runs().join("checkout")),
    )
    .unwrap();

    assert_eq!(
        directory_read(source),
        where_alice_runs().join("beside").join("config")
    );
}

#[test]
fn a_local_source_stepping_above_the_root_stays_at_the_root() {
    let root = where_alice_runs()
        .ancestors()
        .last()
        .expect("an absolute path has a root")
        .to_path_buf();
    let steps_above_the_root = where_alice_runs().components().count() + 1;

    let source = ConfigurationSource::named(
        &format!("local:{}config", "../".repeat(steps_above_the_root)),
        &absolute(where_alice_runs()),
    )
    .unwrap();

    assert_eq!(directory_read(source), root.join("config"));
}

#[test]
fn a_github_source_resolves_its_files_root_to_the_clone_of_that_repository() {
    let location = ConfigurationSource::GitHubRepository {
        repository: GitHubRepository {
            owner: RepositoryOwner::from("Alice"),
            repository: RepositoryName::from("dotfiles"),
        },
        directory: "config".to_owned(),
    }
    .files_come_from()
    .unwrap();

    assert_eq!(
        location,
        SourceLocation::Repository(GitHubRepository {
            owner: RepositoryOwner::from("Alice"),
            repository: RepositoryName::from("dotfiles"),
        })
    );
}

fn absolute(path: PathBuf) -> AbsoluteDirectory {
    AbsoluteDirectory::of(path).expect("a temporary directory is absolute")
}

fn where_alice_runs() -> PathBuf {
    env::temp_dir().join("where_alice_runs")
}

fn directory_read(source: ConfigurationSource) -> PathBuf {
    let ConfigurationSource::LocalDirectory(directory) = source else {
        panic!("expected a local directory, got {source}");
    };
    directory.as_ref().to_path_buf()
}

#[test]
fn a_local_source_named_relative_to_the_working_directory_is_read_from_under_it() {
    let source = ConfigurationSource::named("local:config", &absolute(where_alice_runs())).unwrap();

    assert_eq!(directory_read(source), where_alice_runs().join("config"));
}

#[test]
fn a_local_source_named_in_full_is_read_where_it_names_whatever_the_working_directory() {
    let checkout = env::temp_dir().join("alices_checkout").join("config");

    let source = ConfigurationSource::named(
        &format!("local:{}", checkout.display()),
        &absolute(where_alice_runs()),
    )
    .unwrap();

    assert_eq!(directory_read(source), checkout);
}

#[cfg(windows)]
#[test]
fn a_local_source_named_relative_to_a_drive_is_refused() {
    let refusal =
        ConfigurationSource::named("local:D:config", &absolute(where_alice_runs())).unwrap_err();

    assert!(refusal.contains("name it in full"), "{refusal}");
}

#[test]
fn a_configuration_declaring_the_account_that_owns_its_source_is_read() {
    let checked = refuse_an_account_other_than_the_sources_owner(
        &ConfigurationName::from("Alice/dotfiles/config/everywhere.dotconfig.json"),
        &GitHubAccount::from("Alice"),
        &RepositoryOwner::from("Alice"),
    );

    assert!(checked.is_ok(), "{:?}", checked.unwrap_err());
}

#[test]
fn a_configuration_declaring_an_account_other_than_its_sources_owner_names_both_in_the_refusal() {
    let refusal = refuse_an_account_other_than_the_sources_owner(
        &ConfigurationName::from("Employer/dotfiles/config/work.dotconfig.json"),
        &GitHubAccount::from("Alice"),
        &RepositoryOwner::from("Employer"),
    )
    .unwrap_err()
    .to_string();

    assert!(
        refusal.contains("Employer/dotfiles/config/work.dotconfig.json")
            && refusal.contains("Alice")
            && refusal.contains("Employer"),
        "{refusal}"
    );
}

#[test]
fn an_account_matching_its_sources_owner_in_every_letter_but_case_is_refused() {
    let checked = refuse_an_account_other_than_the_sources_owner(
        &ConfigurationName::from("Alice/dotfiles/config/everywhere.dotconfig.json"),
        &GitHubAccount::from("alice"),
        &RepositoryOwner::from("Alice"),
    );

    assert!(checked.is_err());
}

fn refusing(unreadable: Vec<Unreadable>) -> LoadFailure {
    LoadFailure::Unreadable(
        Refusal::of(unreadable).expect("at least one configuration could not be read"),
    )
}

#[test]
fn a_configuration_needing_a_newer_build_sends_this_one_looking_for_its_own_release() {
    let refusal = refusing(vec![Unreadable::TooNew {
        source: ConfigurationName::from("everywhere.dotconfig.json"),
        required: BUILD_GENERATION.stepped_by(1),
        available: BUILD_GENERATION,
    }]);

    assert!(refusal.is_answered_by_a_newer_build());
}

#[test]
fn a_configuration_a_person_must_repair_is_not_answered_by_updating_this_build() {
    let refusal = refusing(vec![Unreadable::Malformed(anyhow!(
        "everywhere.dotconfig.json is not valid JSON"
    ))]);

    assert!(!refusal.is_answered_by_a_newer_build());
}

#[test]
fn a_document_this_build_has_outgrown_is_not_answered_by_updating_this_build() {
    let refusal = refusing(vec![Unreadable::TooOld {
        source: ConfigurationName::from("everywhere.dotconfig.json"),
        stated: OLDEST_READABLE_GENERATION.stepped_by(-1),
        oldest_readable: OLDEST_READABLE_GENERATION,
    }]);

    assert!(!refusal.is_answered_by_a_newer_build());
}

#[test]
fn one_configuration_needing_a_newer_build_is_enough_to_go_looking_for_one() {
    let refusal = refusing(vec![
        Unreadable::Malformed(anyhow!("personal.dotconfig.json is not valid JSON")),
        Unreadable::TooNew {
            source: ConfigurationName::from("everywhere.dotconfig.json"),
            required: BUILD_GENERATION.stepped_by(1),
            available: BUILD_GENERATION,
        },
    ]);

    assert!(refusal.is_answered_by_a_newer_build());
}

async fn refused_as_unauthorised() -> octocrab::Error {
    use http_body_util::BodyExt;

    let body = http_body_util::Full::new(bytes::Bytes::from_static(
        br#"{ "message": "Bad credentials" }"#,
    ))
    .map_err(|never| match never {})
    .boxed();
    let response = http::Response::builder()
        .status(http::StatusCode::UNAUTHORIZED)
        .body(body)
        .expect("a response GitHub could have sent");

    octocrab::map_github_error(response)
        .await
        .expect_err("a 401 is a failure")
}

fn refusal_naming(failure: EnvironmentFailure) -> LoadFailure {
    refusal_of(vec![SourceFailure::Environment(failure)])
        .expect("a failure to read a source refuses the load")
}

#[tokio::test]
async fn a_source_github_refuses_to_authenticate_is_a_failure_of_the_environment() {
    let refusal = refusal_naming(EnvironmentFailure::GitHubFetch {
        path: "Alice/dotfiles/config".to_owned(),
        failure: refused_as_unauthorised().await,
    });

    let LoadFailure::Environment { .. } = refusal else {
        panic!("expected a failure of the environment, got {refusal:?}");
    };
}

#[tokio::test]
async fn a_source_github_refuses_to_authenticate_counts_as_no_unreadable_configuration() {
    let refusal = refusal_naming(EnvironmentFailure::GitHubFetch {
        path: "Alice/dotfiles/config".to_owned(),
        failure: refused_as_unauthorised().await,
    });

    assert!(refusal.unreadable().is_empty(), "{refusal}");
}

#[test]
fn an_account_the_github_cli_does_not_hold_is_a_failure_of_the_environment() {
    let refusal = refusal_naming(EnvironmentFailure::GitHubCli(cli::Refusal::AccountUnheld {
        account: "Alice".to_owned(),
    }));

    let LoadFailure::Environment { .. } = refusal else {
        panic!("expected a failure of the environment, got {refusal:?}");
    };
}

#[test]
fn an_account_the_github_cli_does_not_hold_counts_as_no_unreadable_configuration() {
    let refusal = refusal_naming(EnvironmentFailure::GitHubCli(cli::Refusal::AccountUnheld {
        account: "Alice".to_owned(),
    }));

    assert!(refusal.unreadable().is_empty(), "{refusal}");
}

#[test]
fn a_configuration_whose_content_cannot_be_read_is_unreadable_rather_than_the_environments() {
    let refusal = refusal_of(vec![SourceFailure::Unreadable(Unreadable::Malformed(
        anyhow!("personal.dotconfig.json is not valid JSON"),
    ))])
    .expect("an unreadable configuration refuses the load");

    let LoadFailure::Unreadable(_) = refusal else {
        panic!("expected an unreadable configuration, got {refusal:?}");
    };
}

#[tokio::test]
async fn a_run_failing_in_the_environment_and_on_content_reports_each_source_with_its_cause() {
    let refusal = refusal_of(vec![
        SourceFailure::Environment(EnvironmentFailure::GitHubFetch {
            path: "Employer/dotfiles/config".to_owned(),
            failure: refused_as_unauthorised().await,
        }),
        SourceFailure::Unreadable(Unreadable::Malformed(anyhow!(
            "personal.dotconfig.json is not valid JSON"
        ))),
    ])
    .expect("both failures refuse the load")
    .to_string();

    assert!(
        refusal.contains("Employer/dotfiles/config: GitHub: Bad credentials")
            && refusal.contains("personal.dotconfig.json is not valid JSON"),
        "{refusal}"
    );
}

#[tokio::test]
async fn a_run_counts_only_the_configurations_whose_content_could_not_be_read() {
    let refusal = refusal_of(vec![
        SourceFailure::Environment(EnvironmentFailure::GitHubFetch {
            path: "Employer/dotfiles/config".to_owned(),
            failure: refused_as_unauthorised().await,
        }),
        SourceFailure::Unreadable(Unreadable::Malformed(anyhow!(
            "personal.dotconfig.json is not valid JSON"
        ))),
        SourceFailure::Unreadable(Unreadable::Malformed(anyhow!(
            "work.dotconfig.json is not valid JSON"
        ))),
    ])
    .expect("every failure refuses the load")
    .to_string();

    assert!(
        refusal.contains("2 configurations could not be read"),
        "{refusal}"
    );
}

#[test]
fn an_absent_github_cli_is_answered_with_how_to_install_it() {
    let reported = EnvironmentFailure::GitHubCli(cli::Refusal::ToolAbsent).to_string();

    assert!(
        reported.ends_with(
            "Install it from https://cli.github.com, then authenticate with `gh auth login`"
        ),
        "{reported}"
    );
}

#[test]
fn an_account_the_github_cli_does_not_hold_is_answered_with_how_to_sign_in_as_it() {
    let reported = EnvironmentFailure::GitHubCli(cli::Refusal::AccountUnheld {
        account: "Alice".to_owned(),
    })
    .to_string();

    assert!(
        reported.ends_with("Run `gh auth login` and sign in as Alice"),
        "{reported}"
    );
}

#[test]
fn a_github_cli_failure_with_no_act_behind_it_is_answered_with_its_reason_alone() {
    let reported = EnvironmentFailure::GitHubCli(cli::Refusal::Failed {
        account: "Alice".to_owned(),
        reason: "the token it wrote is not valid UTF-8".to_owned(),
    })
    .to_string();

    assert!(
        reported.ends_with("the token it wrote is not valid UTF-8"),
        "{reported}"
    );
}

#[tokio::test]
async fn a_directory_that_does_not_exist_is_a_failure_of_the_environment() {
    let error = load_desired_state(
        &[ConfigurationSource::LocalDirectory(absolute(
            env::temp_dir().join("no").join("such").join("directory"),
        ))],
        MachineClass::Personal,
        Path::new("/repositories"),
        &GitHubAccess::new(),
    )
    .await
    .unwrap_err();

    let LoadFailure::Environment { .. } = error else {
        panic!("expected a failure of the environment, got {error:?}");
    };
}

#[test]
fn a_failure_that_is_not_a_refusal_to_read_sends_this_build_looking_for_nothing() {
    let failure = LoadFailure::NoConfigurationFound(vec![ConfigurationSource::LocalDirectory(
        absolute(env::temp_dir().join("config")),
    )]);

    assert!(!failure.is_answered_by_a_newer_build());
}
#[test]
fn the_cause_of_each_refusal_survives_being_combined_with_the_others() {
    let refusal = Refusal::of(vec![
        Unreadable::Malformed(anyhow!("personal.dotconfig.json is not valid JSON")),
        Unreadable::TooNew {
            source: ConfigurationName::from("everywhere.dotconfig.json"),
            required: BUILD_GENERATION.stepped_by(1),
            available: BUILD_GENERATION,
        },
    ])
    .unwrap();

    let [_, needing_a_newer_build] = refusal.unreadable() else {
        panic!("expected both refusals to be kept");
    };

    let Unreadable::TooNew { required, .. } = needing_a_newer_build else {
        panic!("expected the build to still be named as the fault");
    };
    assert_eq!(*required, BUILD_GENERATION.stepped_by(1));
}

fn temporary_checkout(name: &str) -> PathBuf {
    let checkout = env::temp_dir()
        .join("dotfiles_configuration_source_tests")
        .join(process::id().to_string())
        .join(name);
    let _ = fs::remove_dir_all(&checkout);
    fs::create_dir_all(checkout.join(".git")).unwrap();
    fs::create_dir_all(checkout.join("config")).unwrap();
    checkout
}

fn write_configuration(checkout: &Path, file_name: &str, applies_to: &str, resources: &str) {
    let contents = format!(
        r#"{{
                "version": "{BUILD_GENERATION}",
                "applies_to": "{applies_to}",
                "github_account": "Alice",
                "resources": [{resources}]
            }}"#
    );
    let mut file = File::create(checkout.join("config").join(file_name)).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
}

fn command(argument: &str) -> String {
    format!(r#"{{ "kind": "command", "shell": "bash", "args": ["{argument}"] }}"#)
}

async fn load_from(checkout: &Path, machine: MachineClass) -> Result<DesiredState, LoadFailure> {
    load_desired_state(
        &[ConfigurationSource::LocalDirectory(absolute(
            checkout.join("config"),
        ))],
        machine,
        Path::new("/repositories"),
        &GitHubAccess::new(),
    )
    .await
}

fn rendered_other_than_what_every_change_set_carries(desired_state: &DesiredState) -> Vec<String> {
    desired_state
        .other_than_what_every_change_set_carries()
        .into_iter()
        .map(ToString::to_string)
        .collect()
}

#[tokio::test]
async fn a_directory_that_does_not_exist_is_reported_by_path() {
    let missing = Path::new("no").join("such").join("directory");
    let error = load_desired_state(
        &[ConfigurationSource::LocalDirectory(absolute(
            env::temp_dir().join(&missing),
        ))],
        MachineClass::Personal,
        Path::new("/repositories"),
        &GitHubAccess::new(),
    )
    .await
    .unwrap_err();

    assert!(
        error.to_string().contains(&missing.display().to_string()),
        "{error}"
    );
}

#[cfg(windows)]
fn a_directory_not_spelled_in_utf8() -> PathBuf {
    use std::os::windows::ffi::OsStringExt;

    PathBuf::from(std::ffi::OsString::from_wide(&[u16::from(b'/'), 0xD800]))
}

#[cfg(unix)]
fn a_directory_not_spelled_in_utf8() -> PathBuf {
    use std::os::unix::ffi::OsStringExt;

    PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xFF]))
}

#[tokio::test]
async fn a_manifest_that_cannot_be_written_out_is_refused_at_load_naming_why() {
    let checkout = temporary_checkout("unrenderable_manifest");
    write_configuration(&checkout, "everywhere.dotconfig.json", "everywhere", "");
    write_configuration(&checkout, "personal.dotconfig.json", "personal", "");

    let error = load_desired_state(
        &[ConfigurationSource::LocalDirectory(absolute(
            checkout.join("config"),
        ))],
        MachineClass::Personal,
        &a_directory_not_spelled_in_utf8(),
        &GitHubAccess::new(),
    )
    .await
    .unwrap_err();

    let LoadFailure::Irreconcilable(Irreconcilable::UnrenderableManifest(_)) = &error else {
        panic!("expected the manifest to be refused, got {error:?}");
    };
    assert!(error.to_string().contains("UTF-8"), "{error}");
}

#[tokio::test]
async fn a_relative_repositories_root_is_refused_at_load_naming_the_directory() {
    let checkout = temporary_checkout("relative_repositories_root");
    write_configuration(&checkout, "everywhere.dotconfig.json", "everywhere", "");
    write_configuration(&checkout, "personal.dotconfig.json", "personal", "");

    let error = load_desired_state(
        &[ConfigurationSource::LocalDirectory(absolute(
            checkout.join("config"),
        ))],
        MachineClass::Personal,
        Path::new("Repositories"),
        &GitHubAccess::new(),
    )
    .await
    .unwrap_err();

    let LoadFailure::Irreconcilable(Irreconcilable::RelativeRepositoriesDirectory(refusal)) =
        &error
    else {
        panic!("expected the relative directory to be refused, got {error:?}");
    };
    assert_eq!(
        refusal.0,
        Path::new("Repositories").join(Context::from(MachineClass::Personal).repositories_leaf())
    );
}

#[tokio::test]
async fn configurations_in_a_directory_are_read_in_a_stable_order() {
    let checkout = temporary_checkout("stable_order");
    write_configuration(
        &checkout,
        "b_second.dotconfig.json",
        "everywhere",
        &command("second"),
    );
    write_configuration(
        &checkout,
        "a_first.dotconfig.json",
        "personal",
        &command("first"),
    );

    let desired_state = load_from(&checkout, MachineClass::Personal).await.unwrap();

    assert_eq!(
        rendered_other_than_what_every_change_set_carries(&desired_state),
        vec!["command first".to_owned(), "command second".to_owned()]
    );
}

#[tokio::test]
async fn a_directory_holding_a_configuration_for_another_machine_contributes_none_of_it() {
    let checkout = temporary_checkout("another_machine");
    write_configuration(
        &checkout,
        "everywhere.dotconfig.json",
        "everywhere",
        &command("for every machine"),
    );
    write_configuration(
        &checkout,
        "personal.dotconfig.json",
        "personal",
        &command("for a personal machine"),
    );

    let desired_state = load_from(&checkout, MachineClass::Personal).await.unwrap();

    assert!(
        !rendered_other_than_what_every_change_set_carries(&desired_state)
            .contains(&"command for a work machine".to_owned()),
        "{:?}",
        rendered_other_than_what_every_change_set_carries(&desired_state)
    );
}

#[tokio::test]
async fn a_file_without_the_configuration_suffix_is_not_read_at_all() {
    let checkout = temporary_checkout("unsuffixed_file");
    write_configuration(
        &checkout,
        "everywhere.dotconfig.json",
        "everywhere",
        &command("declared"),
    );
    write_configuration(
        &checkout,
        "personal.dotconfig.json",
        "personal",
        &command("also declared"),
    );
    let mut readme = File::create(checkout.join("config").join("README.md")).unwrap();
    readme.write_all(b"These are the configurations.").unwrap();

    let desired_state = load_from(&checkout, MachineClass::Personal).await.unwrap();

    assert_eq!(
        rendered_other_than_what_every_change_set_carries(&desired_state),
        vec![
            "command declared".to_owned(),
            "command also declared".to_owned()
        ]
    );
}

#[tokio::test]
async fn a_source_holding_configurations_of_two_trees_is_refused_by_the_trees_it_names() {
    let checkout = temporary_checkout("two_trees");
    write_configuration(&checkout, "everywhere.dotconfig.json", "everywhere", "");
    write_configuration(&checkout, "work.dotconfig.json", "work", "");

    let error = load_from(&checkout, MachineClass::Work).await.unwrap_err();

    assert!(
        error.to_string().contains("Personal") && error.to_string().contains("Work"),
        "{error}"
    );
}

#[tokio::test]
async fn a_configuration_in_the_superseded_format_is_rejected_by_file_name() {
    let checkout = temporary_checkout("superseded_format");
    let mut file = File::create(checkout.join("config").join("everywhere.dotconfig.json")).unwrap();
    file.write_all(br#"{ "version": "0.1.0", "clone_config": {}, "items": [] }"#)
        .unwrap();

    let error = load_from(&checkout, MachineClass::Personal)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("everywhere.dotconfig.json"));
}

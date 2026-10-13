use super::*;

fn configuration_json(version: &str, body: &str) -> String {
    format!(
        r#"{{ "version": "{version}", "applies_to": "everywhere",
               "github_account": "Alice", {body} }}"#
    )
}

fn parse(body: &str) -> Result<Configuration, Unreadable> {
    parse_configuration(
        &configuration_json(&BUILD_GENERATION.to_string(), body),
        &ConfigurationName::from("the test configuration"),
    )
    .map(|reading| reading.configuration)
}

#[test]
fn a_configuration_whose_version_is_not_a_generation_is_rejected_by_name() {
    let superseded = configuration_json("0.1.0", r#""resources": []"#);

    let error = parse_configuration(
        &superseded,
        &ConfigurationName::from("everywhere.dotconfig.json"),
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(
        message.contains("0.1.0") && message.contains(&BUILD_GENERATION.to_string()),
        "expected the message to name the version and this build's generation, got: {message}"
    );
}

#[test]
fn a_configuration_further_back_than_one_generation_is_a_document_this_build_has_outgrown() {
    let outgrown = configuration_json(
        &OLDEST_READABLE_GENERATION.stepped_by(-1).to_string(),
        r#""resources": []"#,
    );

    let error = parse_configuration(
        &outgrown,
        &ConfigurationName::from("everywhere.dotconfig.json"),
    )
    .unwrap_err();

    let Unreadable::TooOld { stated, .. } = error else {
        panic!("expected the refusal to name the document as outgrown, got: {error}");
    };
    assert_eq!(stated, OLDEST_READABLE_GENERATION.stepped_by(-1));
}

#[test]
fn a_configuration_stating_a_generation_beyond_this_build_is_a_fault_in_the_build() {
    let newer = configuration_json(
        &BUILD_GENERATION.stepped_by(1).to_string(),
        r#""resources": []"#,
    );

    let error = parse_configuration(
        &newer,
        &ConfigurationName::from("everywhere.dotconfig.json"),
    )
    .unwrap_err();

    let Unreadable::TooNew { required, .. } = error else {
        panic!("expected the refusal to name the build as the fault, got: {error}");
    };
    assert_eq!(required, BUILD_GENERATION.stepped_by(1));
}

#[test]
fn a_version_written_as_a_bare_number_is_refused_as_unquoted_rather_than_absent() {
    let unquoted = format!(
        r#"{{ "version": {BUILD_GENERATION}, "applies_to": "everywhere",
               "github_account": "Alice", "resources": [] }}"#
    );

    let error = parse_configuration(
        &unquoted,
        &ConfigurationName::from("everywhere.dotconfig.json"),
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(
        message.contains("quoted") && !message.contains("declares no version"),
        "expected the message to say the version must be quoted, got: {message}"
    );
}

#[test]
fn a_configuration_declaring_no_version_at_all_is_rejected() {
    let versionless =
        r#"{ "applies_to": "everywhere", "github_account": "Alice", "resources": [] }"#;

    let error = parse_configuration(
        versionless,
        &ConfigurationName::from("everywhere.dotconfig.json"),
    )
    .unwrap_err();

    assert!(error.to_string().contains("declares no version"));
}

#[test]
fn a_configuration_naming_no_account_to_act_as_is_refused_by_field() {
    let accountless = format!(
        r#"{{ "version": "{BUILD_GENERATION}", "applies_to": "everywhere",
               "resources": [] }}"#
    );

    let error = parse_configuration(
        &accountless,
        &ConfigurationName::from("personal.dotconfig.json"),
    )
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(
        message.contains("github_account"),
        "expected the message to name the missing field, got: {message}"
    );
}

#[test]
fn a_fault_inside_a_resource_is_reported_by_its_position_in_the_list() {
    let with_an_unknown_shell = configuration_json(
        &BUILD_GENERATION.to_string(),
        r#""resources": [
                { "kind": "command", "shell": "bash", "args": ["first"] },
                { "kind": "command", "shell": "nonesuch", "args": ["second"] }
            ]"#,
    );

    let error = parse_configuration(
        &with_an_unknown_shell,
        &ConfigurationName::from("personal.dotconfig.json"),
    )
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(
        message.contains("resources[1]"),
        "expected the message to name the offending resource, got: {message}"
    );
}

#[test]
fn resources_and_notices_both_default_to_empty_when_absent() {
    let configuration = parse(r#""workspaces": []"#).unwrap();

    assert!(configuration.resources.is_empty());
    assert!(configuration.notices.is_empty());
}

#[test]
fn a_notice_is_written_as_a_bare_string() {
    let configuration = parse(r#""notices": ["Sync the settings repository"]"#).unwrap();

    assert_eq!(
        configuration.notices,
        vec![DeclaredNotice::from("Sync the settings repository")]
    );
}

#[test]
fn a_configuration_cannot_pin_a_crate_to_a_git_revision_by_hand() {
    let pinned = r#""resources": [{
            "kind": "package", "manager": "cargo", "crate_name": "stop-gate",
            "source": {
                "source": "git",
                "url": "https://github.com/Alice/dotfiles",
                "revision": "426d343cd4dbf07fef70dcdfe4b65aedd9a07898"
            }
        }]"#;

    assert!(parse(pinned).is_err());
}

#[test]
fn a_configuration_cannot_claim_a_crate_comes_from_a_workspace_by_hand() {
    let claimed = r#""resources": [{
            "kind": "package", "manager": "cargo", "crate_name": "stop-gate",
            "source": {
                "source": "workspace",
                "repository": { "owner": "Alice", "repository": "dotfiles" }
            }
        }]"#;

    assert!(parse(claimed).is_err());
}

#[test]
fn a_configuration_cannot_claim_the_directory_this_program_installs_binaries_into() {
    let claimed = r#""resources": [{
            "kind": "environment_variable", "shape": "search_path_entry",
            "directory": { "in": "tool_binaries" }
        }]"#;

    assert!(parse(claimed).is_err());
}

#[test]
fn a_search_path_entry_in_a_repository_is_read_as_a_path_under_that_repositorys_clone() {
    let configuration = parse(
        r#""resources": [{
                "kind": "environment_variable", "shape": "search_path_entry",
                "directory": {
                    "in": "repository",
                    "repository": { "owner": "flutter", "repository": "flutter" },
                    "path": "bin"
                }
            }]"#,
    )
    .unwrap();

    let Resource::EnvironmentVariable(EnvironmentVariable::SearchPathEntry(entry)) =
        &configuration.resources[0]
    else {
        panic!("the configuration declared no search path entry");
    };
    assert_eq!(
        entry.directory,
        SearchPathDirectory::Repository {
            repository: GitHubRepository {
                owner: RepositoryOwner::from("flutter"),
                repository: RepositoryName::from("flutter"),
            },
            path: std::path::PathBuf::from("bin"),
        }
    );
}

#[test]
fn a_variable_is_read_as_the_whole_value_it_declares() {
    let configuration = parse(
        r#""resources": [
                { "kind": "environment_variable", "shape": "variable",
                  "name": "EDITOR", "value": "nvim" }
            ]"#,
    )
    .unwrap();

    let Resource::EnvironmentVariable(EnvironmentVariable::Variable(variable)) =
        &configuration.resources[0]
    else {
        panic!("the configuration declared no variable");
    };
    assert_eq!(variable.name, VariableName::try_from("EDITOR").unwrap());
    assert_eq!(variable.value, VariableValue::from("nvim"));
}

#[test]
fn a_variable_claiming_the_search_path_is_refused_naming_the_shape_that_owns_it() {
    let claimed = r#""resources": [
            { "kind": "environment_variable", "shape": "variable",
              "name": "Path", "value": "C:\\only\\this" }
        ]"#;

    let error = parse(claimed).unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("search path entry"), "{message}");
}

#[test]
fn a_variable_claiming_the_search_path_in_another_case_is_refused_just_the_same() {
    let claimed = r#""resources": [
            { "kind": "environment_variable", "shape": "variable",
              "name": "PATH", "value": "C:\\only\\this" }
        ]"#;

    assert!(parse(claimed).is_err());
}

#[test]
fn a_released_binary_naming_no_version_arguments_is_asked_the_usual_way() {
    let configuration = parse(
        r#""resources": [{
                "kind": "application", "shape": "released_binary",
                "repository": { "owner": "BurntSushi", "repository": "ripgrep" },
                "asset": { "match": "ends_with", "value": ".zip" },
                "entry": "rg.exe",
                "version_word": 2
            }]"#,
    )
    .unwrap();

    let Resource::Application(Application::ReleasedBinary(binary)) = &configuration.resources[0]
    else {
        panic!("the configuration declared no released binary");
    };
    assert_eq!(binary.version_arguments, vec!["--version".to_owned()]);
}

#[test]
fn a_uv_tool_naming_no_interpreter_leaves_the_choice_of_one_to_uv() {
    let configuration = parse(
        r#""resources": [
                { "kind": "package", "manager": "uv_tool", "name": "serena-agent" }
            ]"#,
    )
    .unwrap();

    assert_eq!(
        configuration.resources[0],
        Resource::Package(Package::UvTool(UvToolPackage {
            name: UvToolName::from("serena-agent"),
            python: None,
            version: PackageCurrency::Latest,
        }))
    );
}

#[test]
fn the_only_setting_a_configuration_still_declares_is_the_account_it_acts_as() {
    let configuration = parse(r#""resources": []"#).unwrap();

    assert_eq!(configuration.github_account, GitHubAccount::from("Alice"));
    assert_eq!(configuration.applies_to, Context::Everywhere);
}

#[test]
fn kinds_converge_in_the_order_that_keeps_applications_ahead_of_symlinks() {
    let mut kinds = vec![
        ResourceKind::Command,
        ResourceKind::Symlink,
        ResourceKind::Application,
        ResourceKind::Repository,
        ResourceKind::Registration,
        ResourceKind::EnvironmentVariable,
        ResourceKind::Package,
    ];

    kinds.sort();

    assert_eq!(
        kinds,
        vec![
            ResourceKind::Repository,
            ResourceKind::Application,
            ResourceKind::Package,
            ResourceKind::EnvironmentVariable,
            ResourceKind::Symlink,
            ResourceKind::Registration,
            ResourceKind::Command,
        ]
    );
}

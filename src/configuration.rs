use {
    anyhow::{Context as _, anyhow},
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::fmt::Display,
};

// The build script includes this file to generate the configuration schema, and resolves a
// `#[path]`-loaded module's children against its own directory rather than a directory named
// after it. Naming each child outright resolves the same way from both roots.
#[path = "configuration/context.rs"]
pub mod context;
#[path = "configuration/generation.rs"]
pub mod generation;
#[path = "configuration/identity.rs"]
pub mod identity;
#[path = "configuration/migration.rs"]
pub mod migration;
#[path = "configuration/names.rs"]
pub mod names;
#[path = "configuration/presence_check.rs"]
pub mod presence_check;
#[path = "configuration/resource.rs"]
pub mod resource;
#[path = "configuration/unreadable.rs"]
pub mod unreadable;
#[path = "configuration/workspace.rs"]
pub mod workspace;

pub use {
    context::{Context, MachineClass},
    generation::{
        BENEATH_OLDEST_READABLE_GENERATION, BEYOND_BUILD_GENERATION, BUILD_GENERATION, Generation,
        OLDEST_READABLE_GENERATION,
    },
    identity::Identity,
    migration::{Migration, announcement},
    names::{
        ApplicationName, BinaryName, CrateName, GitHubAccount, McpServerName, RepositoryName,
        RepositoryOwner, VariableName, VariableValue, WingetPackageId,
    },
    presence_check::PresenceCheck,
    resource::{
        Application, ApplicationSource, ArchiveEntry, AssetPattern, CargoPackage, CargoSource,
        ClaudeMcpServer, Command, EnvironmentVariable, GitHubRepository, Installer, McpScope,
        Package, Registration, ReleasedBinary, Resource, ResourceKind, SearchPathDirectory,
        SearchPathEntry, Shell, Symlink, Variable, VersionWord, WingetPackage,
    },
    unreadable::Unreadable,
    workspace::CargoWorkspace,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(
    description = "A configuration as it is written down: the desired state of a machine, \
                          as a set of resources and the notices that accompany them, and which \
                          machines it is for."
)]
pub struct Configuration {
    #[schemars(
        description = "The lowest generation of dotfiles_configurator that can read this \
                       configuration, written as a whole number. A build reads any configuration \
                       at or below its own generation and refuses one above."
    )]
    pub version: Generation,
    #[schemars(
        description = "Which machines this configuration is for. One declaring \"everywhere\" \
                       applies to every machine; any other applies only to a machine an \
                       invocation names as being that."
    )]
    pub applies_to: Context,
    #[schemars(
        description = "The GitHub account every resource this configuration declares acts as. \
                       Not a repository's owner — a public repository is read by any account at \
                       all."
    )]
    pub github_account: GitHubAccount,
    #[serde(default)]
    pub workspaces: Vec<CargoWorkspace>,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub notices: Vec<Notice>,
}

/// A message for a person about something the tool cannot do. Not a resource — it never
/// converges, is reported by every change set, and is never applied.
#[derive(
    Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Notice(String);

impl From<String> for Notice {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Notice {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl Display for Notice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A configuration as this build reads it, and the generation it was written as where that is not
/// the one this build reads.
#[derive(Debug)]
pub struct Reading {
    pub configuration: Configuration,
    pub migrated_from: Option<Generation>,
}

pub fn parse_configuration(contents: &str, source: &str) -> Result<Reading, Unreadable> {
    let document: serde_json::Value =
        serde_json::from_str(contents).with_context(|| format!("{source} is not valid JSON"))?;

    let stated = match document.get("version") {
        None => Err(anyhow!(
            "{source} declares no version. A configuration's version is the lowest generation of \
             dotfiles_configurator that can read it, written as a quoted whole number, and this \
             build is generation {BUILD_GENERATION}."
        )),
        Some(unquoted) => unquoted.as_str().ok_or_else(|| {
            anyhow!(
                "{source} declares the version {unquoted}, which a configuration writes as a \
                 quoted whole number. This build is generation {BUILD_GENERATION}."
            )
        }),
    }?;

    let required = Generation::try_from(stated).map_err(|fault| {
        anyhow!(
            "{source}: {fault}. This build reads generation {BUILD_GENERATION}, so rewrite the \
             configuration as a generation {BUILD_GENERATION} document."
        )
    })?;

    if !required.is_met_by(BUILD_GENERATION) {
        return Err(Unreadable::TooNew {
            source: source.to_owned(),
            required,
            available: BUILD_GENERATION,
        });
    }

    if required.is_outgrown_by(OLDEST_READABLE_GENERATION) {
        return Err(Unreadable::TooOld {
            source: source.to_owned(),
            stated: required,
            oldest_readable: OLDEST_READABLE_GENERATION,
        });
    }

    if required == OLDEST_READABLE_GENERATION {
        let outgoing: migration::OutgoingConfiguration = serde_path_to_error::deserialize(document)
            .with_context(|| format!("{source} is not a valid generation {required} configuration"))
            .map_err(Unreadable::Malformed)?;

        return Ok(Reading {
            migrated_from: Some(outgoing.stated_generation()),
            configuration: outgoing.into(),
        });
    }

    serde_path_to_error::deserialize(document)
        .with_context(|| {
            format!("{source} is not a valid generation {BUILD_GENERATION} configuration")
        })
        .map_err(Unreadable::Malformed)
        .map(|configuration| Reading {
            configuration,
            migrated_from: None,
        })
}

#[cfg(test)]
mod tests {
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
            "the test configuration",
        )
        .map(|reading| reading.configuration)
    }

    #[test]
    fn a_configuration_whose_version_is_not_a_generation_is_rejected_by_name() {
        let superseded = configuration_json("0.1.0", r#""resources": []"#);

        let error = parse_configuration(&superseded, "everywhere.dotconfig.json").unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("0.1.0") && message.contains(&BUILD_GENERATION.to_string()),
            "expected the message to name the version and this build's generation, got: {message}"
        );
    }

    #[test]
    fn a_configuration_further_back_than_one_generation_is_a_document_this_build_has_outgrown() {
        let outgrown = configuration_json(
            &BENEATH_OLDEST_READABLE_GENERATION.to_string(),
            r#""resources": []"#,
        );

        let error = parse_configuration(&outgrown, "everywhere.dotconfig.json").unwrap_err();

        let Unreadable::TooOld { stated, .. } = error else {
            panic!("expected the refusal to name the document as outgrown, got: {error}");
        };
        assert_eq!(stated, BENEATH_OLDEST_READABLE_GENERATION);
    }

    #[test]
    fn a_configuration_stating_a_generation_beyond_this_build_is_a_fault_in_the_build() {
        let newer = configuration_json(&BEYOND_BUILD_GENERATION.to_string(), r#""resources": []"#);

        let error = parse_configuration(&newer, "everywhere.dotconfig.json").unwrap_err();

        let Unreadable::TooNew { required, .. } = error else {
            panic!("expected the refusal to name the build as the fault, got: {error}");
        };
        assert_eq!(required, BEYOND_BUILD_GENERATION);
    }

    #[test]
    fn a_version_written_as_a_bare_number_is_refused_as_unquoted_rather_than_absent() {
        let unquoted = format!(
            r#"{{ "version": {BUILD_GENERATION}, "applies_to": "everywhere",
               "github_account": "Alice", "resources": [] }}"#
        );

        let error = parse_configuration(&unquoted, "everywhere.dotconfig.json").unwrap_err();

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

        let error = parse_configuration(versionless, "everywhere.dotconfig.json").unwrap_err();

        assert!(error.to_string().contains("declares no version"));
    }

    #[test]
    fn a_configuration_naming_no_account_to_act_as_is_refused_by_field() {
        let accountless = format!(
            r#"{{ "version": "{BUILD_GENERATION}", "applies_to": "everywhere",
               "resources": [] }}"#
        );

        let error = parse_configuration(&accountless, "personal.dotconfig.json").unwrap_err();

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

        let error =
            parse_configuration(&with_an_unknown_shell, "personal.dotconfig.json").unwrap_err();

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
            vec![Notice::from("Sync the settings repository")]
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

        let Resource::Application(Application::ReleasedBinary(binary)) =
            &configuration.resources[0]
        else {
            panic!("the configuration declared no released binary");
        };
        assert_eq!(binary.version_arguments, vec!["--version".to_owned()]);
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
}

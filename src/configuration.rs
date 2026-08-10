use {
    anyhow::{Context as _, Result, anyhow, bail},
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{collections::BTreeMap, fmt::Display, path::PathBuf},
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
    context::Context,
    generation::{
        BENEATH_OLDEST_READABLE_GENERATION, BEYOND_BUILD_GENERATION, BUILD_GENERATION, Generation,
        OLDEST_READABLE_GENERATION,
    },
    identity::Identity,
    names::{
        ApplicationName, BinaryName, CrateName, GitHubAccount, McpServerName, RepositoryName,
        RepositoryOwner, WingetPackageId,
    },
    presence_check::PresenceCheck,
    resource::{
        Application, ApplicationSource, ArchiveEntry, AssetPattern, CargoPackage, CargoSource,
        ClaudeMcpServer, Command, GitHubRepository, Installer, McpScope, Package, Registration,
        ReleasedBinary, Resource, ResourceKind, Shell, Symlink, VersionWord, WingetPackage,
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
    pub machine: MachineSettings,
    #[serde(default)]
    pub workspaces: Vec<CargoWorkspace>,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub notices: Vec<Notice>,
}

/// The settings every resource in a configuration is read against.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MachineSettings {
    /// Absolute path of the directory repositories are cloned into.
    pub repositories_directory_path: PathBuf,
    /// The username used when authenticating against GitHub.
    pub github_username: GitHubAccount,
    /// The repository holding the dotfiles themselves, cloned before any symlink can converge.
    pub dotfiles_repository: GitHubRepository,
}

impl MachineSettings {
    pub fn dotfiles_repository_path(&self) -> PathBuf {
        self.repositories_directory_path
            .join(self.dotfiles_repository.repository.as_ref())
    }
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

/// Several configurations merged into the one desired state a machine is converged against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredState {
    pub machine: MachineSettings,
    pub workspaces: Vec<CargoWorkspace>,
    pub resources: Vec<Resource>,
    pub notices: Vec<Notice>,
}

impl DesiredState {
    /// The dotfiles repository is declared as a setting because symlinks resolve against it, and
    /// it is also an ordinary repository resource so that it converges through the same
    /// machinery as any other clone.
    pub fn with_dotfiles_repository(mut self) -> Self {
        let dotfiles_repository = Resource::Repository(self.machine.dotfiles_repository.clone());
        if !self.resources.contains(&dotfiles_repository) {
            self.resources.insert(0, dotfiles_repository);
        }
        self
    }
}

pub fn parse_configuration(contents: &str, source: &str) -> Result<Configuration, Unreadable> {
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

    serde_path_to_error::deserialize(document)
        .with_context(|| {
            format!("{source} is not a valid generation {BUILD_GENERATION} configuration")
        })
        .map_err(Unreadable::Malformed)
}

/// Merges loaded configurations into one desired state, collapsing identical claims and rejecting
/// conflicting ones.
pub fn merge_configurations(configurations: Vec<(String, Configuration)>) -> Result<DesiredState> {
    let mut configurations = configurations.into_iter();
    let (first_source, first) = configurations
        .next()
        .ok_or_else(|| anyhow!("No configurations were loaded"))?;

    let machine = first.machine.clone();
    let mut workspaces: Vec<CargoWorkspace> = Vec::new();
    let mut resources: Vec<Resource> = Vec::new();
    let mut notices: Vec<Notice> = Vec::new();
    let mut claimed: BTreeMap<Identity, (String, Resource)> = BTreeMap::new();

    for (source, configuration) in
        std::iter::once((first_source.clone(), first)).chain(configurations)
    {
        if configuration.machine != machine {
            bail!(
                "{source} declares different machine settings from {first_source}. Every \
                 configuration loaded together must agree on them."
            );
        }

        for resource in configuration.resources {
            match resource.identity() {
                None => resources.push(resource),
                Some(identity) => match claimed.get(&identity) {
                    None => {
                        claimed.insert(identity, (source.clone(), resource.clone()));
                        resources.push(resource);
                    }
                    Some((_, existing)) if *existing == resource => {}
                    Some((existing_source, existing)) => bail!(
                        "{source} and {existing_source} make conflicting claims on {identity}. \
                         No machine could satisfy both:\n  {existing}\n  {resource}"
                    ),
                },
            }
        }

        for workspace in configuration.workspaces {
            if !workspaces.contains(&workspace) {
                workspaces.push(workspace);
            }
        }

        notices.extend(configuration.notices);
    }

    Ok(DesiredState {
        machine,
        workspaces,
        resources,
        notices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine_settings_json() -> &'static str {
        r#""machine": {
            "repositories_directory_path": "C:\\Repositories",
            "github_username": "Alice",
            "dotfiles_repository": { "owner": "Alice", "repository": "dotfiles" }
        }"#
    }

    fn configuration_json(version: &str, body: &str) -> String {
        format!(
            r#"{{ "version": "{version}", "applies_to": "everywhere", {}, {body} }}"#,
            machine_settings_json()
        )
    }

    fn parse(body: &str) -> Result<Configuration, Unreadable> {
        parse_configuration(
            &configuration_json(&BUILD_GENERATION.to_string(), body),
            "the test configuration",
        )
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
    fn a_configuration_stating_the_generation_below_this_build_is_read() {
        let earlier = configuration_json(
            &OLDEST_READABLE_GENERATION.to_string(),
            r#""resources": []"#,
        );

        let configuration = parse_configuration(&earlier, "everywhere.dotconfig.json").unwrap();

        assert_eq!(configuration.version, OLDEST_READABLE_GENERATION);
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
            r#"{{ "version": 3, "applies_to": "everywhere", {}, "resources": [] }}"#,
            machine_settings_json()
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
        let versionless = format!(
            r#"{{ "applies_to": "everywhere", {}, "resources": [] }}"#,
            machine_settings_json()
        );

        let error = parse_configuration(&versionless, "everywhere.dotconfig.json").unwrap_err();

        assert!(error.to_string().contains("declares no version"));
    }

    #[test]
    fn a_missing_machine_setting_is_reported_by_the_object_it_belongs_to() {
        let without_a_repositories_directory_path = r#"{
            "version": "4",
            "applies_to": "everywhere",
            "machine": {
                "github_username": "Alice",
                "dotfiles_repository": { "owner": "Alice", "repository": "dotfiles" }
            },
            "resources": []
        }"#;

        let error = parse_configuration(
            without_a_repositories_directory_path,
            "personal.dotconfig.json",
        )
        .unwrap_err();

        let message = format!("{error:#}");
        assert!(
            message.contains("machine") && message.contains("repositories_directory_path"),
            "expected the message to name the object holding the field, got: {message}"
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
        let configuration = parse_configuration(
            &configuration_json(&BUILD_GENERATION.to_string(), r#""notices": []"#),
            "test",
        )
        .unwrap();

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
    fn a_workspace_declared_by_two_configurations_collapses_to_one() {
        let workspace = r#""workspaces": [
            { "repository": { "owner": "Alice", "repository": "dotfiles" } }
        ]"#;
        let loaded = vec![
            ("everywhere".to_owned(), parse(workspace).unwrap()),
            ("personal".to_owned(), parse(workspace).unwrap()),
        ];

        let desired_state = merge_configurations(loaded).unwrap();

        assert_eq!(desired_state.workspaces.len(), 1);
    }

    #[test]
    fn a_command_declares_no_identity() {
        let configuration = parse(
            r#""resources": [{ "kind": "command", "shell": "command_prompt", "args": ["wsl"] }]"#,
        )
        .unwrap();

        assert_eq!(configuration.resources[0].identity(), None);
    }

    #[test]
    fn two_identical_claims_on_one_fact_collapse_to_a_single_resource() {
        let symlink = r#""resources": [{
            "kind": "symlink", "source_path": "gitconfig/.gitconfig", "link_path": ".gitconfig"
        }]"#;
        let loaded = vec![
            ("everywhere".to_owned(), parse(symlink).unwrap()),
            ("personal".to_owned(), parse(symlink).unwrap()),
        ];

        let desired_state = merge_configurations(loaded).unwrap();

        assert_eq!(desired_state.resources.len(), 1);
    }

    #[test]
    fn two_conflicting_claims_on_one_fact_are_rejected_naming_both_sources() {
        let everywhere = parse(
            r#""resources": [{
                "kind": "symlink", "source_path": "gitconfig/.gitconfig", "link_path": ".gitconfig"
            }]"#,
        )
        .unwrap();
        let personal = parse(
            r#""resources": [{
                "kind": "symlink", "source_path": "other/.gitconfig", "link_path": ".gitconfig"
            }]"#,
        )
        .unwrap();

        let error = merge_configurations(vec![
            ("everywhere".to_owned(), everywhere),
            ("personal".to_owned(), personal),
        ])
        .unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("everywhere") && message.contains("personal"),
            "expected the message to name both sources, got: {message}"
        );
    }

    #[test]
    fn commands_never_conflict_with_one_another_because_they_claim_nothing() {
        let command = r#""resources": [
            { "kind": "command", "shell": "command_prompt", "args": ["wsl"] }
        ]"#;
        let loaded = vec![
            ("everywhere".to_owned(), parse(command).unwrap()),
            ("personal".to_owned(), parse(command).unwrap()),
        ];

        let desired_state = merge_configurations(loaded).unwrap();

        assert_eq!(desired_state.resources.len(), 2);
    }

    #[test]
    fn configurations_disagreeing_on_machine_settings_are_rejected() {
        let everywhere = parse(r#""resources": []"#).unwrap();
        let mut personal = parse(r#""resources": []"#).unwrap();
        personal.machine.github_username = "Bob".into();

        let error = merge_configurations(vec![
            ("everywhere".to_owned(), everywhere),
            ("personal".to_owned(), personal),
        ])
        .unwrap_err();

        assert!(error.to_string().contains("machine settings"));
    }

    #[test]
    fn the_dotfiles_repository_becomes_a_repository_resource_of_its_own() {
        let desired_state = merge_configurations(vec![(
            "everywhere".to_owned(),
            parse(r#""resources": []"#).unwrap(),
        )])
        .unwrap()
        .with_dotfiles_repository();

        assert_eq!(
            desired_state.resources,
            vec![Resource::Repository(GitHubRepository {
                owner: RepositoryOwner::from("Alice"),
                repository: RepositoryName::from("dotfiles"),
            })]
        );
    }

    fn released_binary(entry: &str, owner: &str) -> String {
        format!(
            r#""resources": [{{
                "kind": "application", "shape": "released_binary",
                "repository": {{ "owner": "{owner}", "repository": "ripgrep" }},
                "asset": {{ "match": "ends_with", "value": ".zip" }},
                "entry": "{entry}",
                "version_word": 2
            }}]"#
        )
    }

    #[test]
    fn a_released_binary_naming_no_version_arguments_is_asked_the_usual_way() {
        let configuration = parse(&released_binary("rg.exe", "BurntSushi")).unwrap();

        let Resource::Application(Application::ReleasedBinary(binary)) =
            &configuration.resources[0]
        else {
            panic!("the configuration declared no released binary");
        };
        assert_eq!(binary.version_arguments, vec!["--version".to_owned()]);
    }

    #[test]
    fn a_released_binary_claims_the_name_its_entry_carries_rather_than_the_whole_path() {
        let configuration = parse(&released_binary("bin/rg.exe", "BurntSushi")).unwrap();

        assert_eq!(
            configuration.resources[0].identity(),
            Some(Identity::InstalledBinary(BinaryName::from("rg.exe")))
        );
    }

    #[test]
    fn two_released_binaries_installing_under_one_name_are_refused() {
        let loaded = vec![
            (
                "everywhere".to_owned(),
                parse(&released_binary("rg.exe", "BurntSushi")).unwrap(),
            ),
            (
                "personal".to_owned(),
                parse(&released_binary("rg.exe", "Someone")).unwrap(),
            ),
        ];

        let error = merge_configurations(loaded).unwrap_err();

        assert!(error.to_string().contains("conflicting claims"));
    }

    #[test]
    fn an_installer_and_a_released_binary_of_one_name_claim_different_facts() {
        let installer = parse(
            r#""resources": [{
                "kind": "application", "shape": "installer", "name": "rg.exe",
                "source": { "source": "uri", "uri": "https://example.invalid/rg.exe",
                            "installer_file_name": "rg.exe" },
                "presence_check": { "check": "command_on_path", "command": "rg" }
            }]"#,
        )
        .unwrap();
        let loaded = vec![
            ("everywhere".to_owned(), installer),
            (
                "personal".to_owned(),
                parse(&released_binary("rg.exe", "BurntSushi")).unwrap(),
            ),
        ];

        assert_eq!(merge_configurations(loaded).unwrap().resources.len(), 2);
    }

    #[test]
    fn kinds_converge_in_the_order_that_keeps_applications_ahead_of_symlinks() {
        let mut kinds = vec![
            ResourceKind::Command,
            ResourceKind::Symlink,
            ResourceKind::Application,
            ResourceKind::Repository,
            ResourceKind::Registration,
            ResourceKind::Package,
        ];

        kinds.sort();

        assert_eq!(
            kinds,
            vec![
                ResourceKind::Repository,
                ResourceKind::Application,
                ResourceKind::Package,
                ResourceKind::Symlink,
                ResourceKind::Registration,
                ResourceKind::Command,
            ]
        );
    }
}

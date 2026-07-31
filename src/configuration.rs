use {
    anyhow::{Context, Result, anyhow, bail},
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{collections::BTreeMap, fmt::Display, path::PathBuf},
};

// The build script includes this file to generate the configuration schema, and resolves a
// `#[path]`-loaded module's children against its own directory rather than a directory named
// after it. Naming each child outright resolves the same way from both roots.
#[path = "configuration/identity.rs"]
pub mod identity;
#[path = "configuration/presence_check.rs"]
pub mod presence_check;
#[path = "configuration/resource.rs"]
pub mod resource;

pub use {
    identity::Identity,
    presence_check::PresenceCheck,
    resource::{
        Application, ApplicationName, ApplicationSource, AssetPattern, CargoPackage, CargoSource,
        ClaudeMcpServer, Command, McpScope, Package, Registration, RepositoryName, Resource,
        ResourceKind, Shell, Symlink, WingetPackage,
    },
};

/// The only configuration format version this build understands. `Configuration.version` was
/// parsed and discarded until the model took a breaking revision; it is now the gate that rejects
/// the old shape with a real message.
pub const SUPPORTED_VERSION: &str = "2";

/// A configuration as it is written down: the desired state of a machine, as a set of resources
/// and the notices that accompany them.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Configuration {
    /// The configuration format version. Must be "2".
    pub version: String,
    pub machine: MachineSettings,
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
    pub github_username: String,
    /// The repository holding the dotfiles themselves, cloned before any symlink can converge.
    pub dotfiles_repository: RepositoryName,
}

impl MachineSettings {
    pub fn dotfiles_repository_path(&self) -> PathBuf {
        self.repositories_directory_path
            .join(&self.dotfiles_repository.repo)
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

/// Reads a configuration from JSON, rejecting any version this build does not understand before
/// attempting to interpret the shape.
pub fn parse_configuration(contents: &str, source: &str) -> Result<Configuration> {
    let document: serde_json::Value =
        serde_json::from_str(contents).with_context(|| format!("{source} is not valid JSON"))?;

    let version = document
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("{source} declares no version; expected \"{SUPPORTED_VERSION}\""))?;

    if version != SUPPORTED_VERSION {
        bail!(
            "{source} declares configuration version \"{version}\", but this build understands \
             only \"{SUPPORTED_VERSION}\". Migrate the configuration to the declarative resource \
             format before running it."
        );
    }

    serde_json::from_value(document).with_context(|| {
        format!("{source} is not a valid version {SUPPORTED_VERSION} configuration")
    })
}

/// Merges loaded configurations into one desired state, collapsing identical claims and rejecting
/// conflicting ones.
pub fn merge_configurations(configurations: Vec<(String, Configuration)>) -> Result<DesiredState> {
    let mut configurations = configurations.into_iter();
    let (first_source, first) = configurations
        .next()
        .ok_or_else(|| anyhow!("No configurations were loaded"))?;

    let machine = first.machine.clone();
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

        notices.extend(configuration.notices);
    }

    Ok(DesiredState {
        machine,
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
            "dotfiles_repository": { "owner": "Alice", "repo": "dotfiles" }
        }"#
    }

    fn configuration_json(version: &str, body: &str) -> String {
        format!(
            r#"{{ "version": "{version}", {}, {body} }}"#,
            machine_settings_json()
        )
    }

    fn parse(body: &str) -> Result<Configuration> {
        parse_configuration(
            &configuration_json(SUPPORTED_VERSION, body),
            "the test configuration",
        )
    }

    #[test]
    fn a_configuration_declaring_an_older_format_version_is_rejected_by_name() {
        let older = configuration_json("0.1.0", r#""resources": []"#);

        let error = parse_configuration(&older, "everywhere.dotconfig.json").unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("0.1.0") && message.contains(SUPPORTED_VERSION),
            "expected the message to name both versions, got: {message}"
        );
    }

    #[test]
    fn a_configuration_declaring_no_version_at_all_is_rejected() {
        let versionless = format!(r#"{{ {}, "resources": [] }}"#, machine_settings_json());

        let error = parse_configuration(&versionless, "everywhere.dotconfig.json").unwrap_err();

        assert!(error.to_string().contains("declares no version"));
    }

    #[test]
    fn resources_and_notices_both_default_to_empty_when_absent() {
        let configuration = parse_configuration(
            &configuration_json(SUPPORTED_VERSION, r#""notices": []"#),
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
        personal.machine.github_username = "Bob".to_owned();

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
            vec![Resource::Repository(RepositoryName {
                owner: "Alice".to_owned(),
                repo: "dotfiles".to_owned(),
            })]
        );
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

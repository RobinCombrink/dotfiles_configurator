use {
    crate::{
        configuration::{
            CargoWorkspace, Configuration, Context, GitHubAccount, GitHubRepository, Identity,
            Migration, Notice, Resource, ResourceKind,
        },
        convergence::Requirement,
    },
    anyhow::{Result, anyhow, bail},
    std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    },
};

// ADR 0025
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLocation {
    Repository(GitHubRepository),
    Checkout(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FilesRoot {
    Clone {
        repository: GitHubRepository,
        directory: PathBuf,
    },
    Checkout(PathBuf),
}

impl FilesRoot {
    fn path(&self) -> &Path {
        match self {
            FilesRoot::Clone { directory, .. } | FilesRoot::Checkout(directory) => directory,
        }
    }

    fn requirement(&self) -> Option<Requirement> {
        match self {
            FilesRoot::Clone { repository, .. } => {
                Some(Requirement::DotfilesRepository(repository.clone()))
            }
            FilesRoot::Checkout(_) => None,
        }
    }

    fn repository(&self) -> Option<GitHubRepository> {
        match self {
            FilesRoot::Clone { repository, .. } => Some(repository.clone()),
            FilesRoot::Checkout(_) => None,
        }
    }
}

// ADR 0027
#[derive(Debug, Clone, PartialEq, Eq)]
struct Origin {
    account: GitHubAccount,
    files_root: FilesRoot,
    repositories_directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved<T> {
    value: T,
    origin: Origin,
}

pub type ResolvedResource = Resolved<Resource>;
pub type ResolvedWorkspace = Resolved<CargoWorkspace>;
pub type ResolvedNotice = Resolved<Notice>;

impl<T> Resolved<T> {
    pub fn declared(&self) -> &T {
        &self.value
    }

    pub fn files_root(&self) -> &Path {
        self.origin.files_root.path()
    }

    pub fn clone_directory(&self, repository: &GitHubRepository) -> PathBuf {
        self.origin
            .repositories_directory
            .join(repository.repository.as_ref())
    }

    /// Pairs a resource resolved out of this one's source with the same origin, which is the only
    /// origin it could have come from.
    pub fn alongside<U>(&self, value: U) -> Resolved<U> {
        Resolved {
            value,
            origin: self.origin.clone(),
        }
    }
}

impl ResolvedResource {
    pub fn kind(&self) -> ResourceKind {
        self.value.kind()
    }

    pub fn identity(&self) -> Option<Identity> {
        self.value
            .identity_within(&self.origin.repositories_directory)
    }

    pub fn requirements(&self) -> Vec<Requirement> {
        let mut requirements = self.value.tool_requirements();
        requirements.extend(self.files_requirement());
        requirements.sort();
        requirements.dedup();
        requirements
    }

    fn files_requirement(&self) -> Option<Requirement> {
        match &self.value {
            Resource::Symlink(_) => self.origin.files_root.requirement(),
            Resource::Repository(_)
            | Resource::Application(_)
            | Resource::Package(_)
            | Resource::Registration(_)
            | Resource::Command(_) => None,
        }
    }
}

impl<T: std::fmt::Display> std::fmt::Display for Resolved<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.value, formatter)
    }
}

pub struct ResolvedConfiguration {
    origin: Origin,
    context: Context,
    workspaces: Vec<CargoWorkspace>,
    resources: Vec<Resource>,
    notices: Vec<Notice>,
}

impl ResolvedConfiguration {
    pub fn read(
        configuration: Configuration,
        location: SourceLocation,
        repositories_root: &Path,
    ) -> Self {
        let repositories_directory =
            repositories_root.join(configuration.applies_to.repositories_leaf());
        let files_root = match location {
            SourceLocation::Repository(repository) => FilesRoot::Clone {
                directory: repositories_directory.join(repository.repository.as_ref()),
                repository,
            },
            SourceLocation::Checkout(directory) => FilesRoot::Checkout(directory),
        };

        Self {
            origin: Origin {
                account: configuration.github_account,
                files_root,
                repositories_directory,
            },
            context: configuration.applies_to,
            workspaces: configuration.workspaces,
            resources: configuration.resources,
            notices: configuration.notices,
        }
    }

    pub fn context(&self) -> Context {
        self.context
    }

    fn pair<T>(&self, value: T) -> Resolved<T> {
        Resolved {
            value,
            origin: self.origin.clone(),
        }
    }

    fn resources(&self) -> Vec<ResolvedResource> {
        let files_come_from = self
            .origin
            .files_root
            .repository()
            .map(|repository| self.pair(Resource::Repository(repository)));

        files_come_from
            .into_iter()
            .chain(
                self.resources
                    .iter()
                    .cloned()
                    .map(|resource| self.pair(resource)),
            )
            .collect()
    }

    fn workspaces(&self) -> Vec<ResolvedWorkspace> {
        self.workspaces
            .iter()
            .cloned()
            .map(|workspace| self.pair(workspace))
            .collect()
    }

    fn notices(&self) -> Vec<ResolvedNotice> {
        self.notices
            .iter()
            .cloned()
            .map(|notice| self.pair(notice))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredState {
    pub resources: Vec<ResolvedResource>,
    pub workspaces: Vec<ResolvedWorkspace>,
    pub notices: Vec<ResolvedNotice>,
    pub migrations: Vec<Migration>,
    pub announcements: Vec<Notice>,
    account: GitHubAccount,
}

impl DesiredState {
    pub fn account(&self) -> &GitHubAccount {
        &self.account
    }

    /// The documents an apply rewrites, and what it can only announce about the ones it cannot.
    pub fn also_reporting(
        mut self,
        migrations: Vec<Migration>,
        announcements: Vec<Notice>,
    ) -> Self {
        self.migrations = migrations;
        self.announcements = announcements;
        self
    }

    pub fn of(configurations: Vec<(String, ResolvedConfiguration)>) -> Result<Self> {
        let account = account_of_the_configuration_for_every_machine(&configurations)?;
        refuse_a_set_holding_nothing_for_this_class(&configurations)?;

        let mut resources: Vec<ResolvedResource> = Vec::new();
        let mut workspaces: Vec<ResolvedWorkspace> = Vec::new();
        let mut notices: Vec<ResolvedNotice> = Vec::new();
        let mut claimed: BTreeMap<Identity, (String, ResolvedResource)> = BTreeMap::new();

        for (source, configuration) in &configurations {
            for resource in configuration.resources() {
                match resource.identity() {
                    None => resources.push(resource),
                    Some(identity) => match claimed.get(&identity) {
                        None => {
                            claimed.insert(identity, (source.clone(), resource.clone()));
                            resources.push(resource);
                        }
                        Some((_, existing)) if *existing == resource => {}
                        Some((existing_source, existing)) => bail!(
                            "{source} and {existing_source} make conflicting claims on \
                             {identity}. No machine could satisfy both:\n  {existing}\n  \
                             {resource}"
                        ),
                    },
                }
            }

            for workspace in configuration.workspaces() {
                if !workspaces.contains(&workspace) {
                    workspaces.push(workspace);
                }
            }

            notices.extend(configuration.notices());
        }

        Ok(Self {
            resources,
            workspaces,
            notices,
            migrations: Vec::new(),
            announcements: Vec::new(),
            account,
        })
    }
}

fn account_of_the_configuration_for_every_machine(
    configurations: &[(String, ResolvedConfiguration)],
) -> Result<GitHubAccount> {
    configurations
        .iter()
        .find(|(_, configuration)| configuration.context() == Context::Everywhere)
        .map(|(_, configuration)| configuration.origin.account.clone())
        .ok_or_else(|| {
            anyhow!(
                "No configuration for every machine was loaded. A run reads one configuration for \
                 every machine and exactly one for this machine's class."
            )
        })
}

fn refuse_a_set_holding_nothing_for_this_class(
    configurations: &[(String, ResolvedConfiguration)],
) -> Result<()> {
    match configurations
        .iter()
        .any(|(_, configuration)| configuration.context() != Context::Everywhere)
    {
        true => Ok(()),
        false => bail!(
            "No configuration for this machine's class was loaded. A run reads one configuration \
             for every machine and exactly one for this machine's class."
        ),
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::configuration::{BUILD_GENERATION, RepositoryName, RepositoryOwner, Unreadable},
    };

    const REPOSITORIES_ROOT: &str = "/repositories";

    fn dotfiles() -> GitHubRepository {
        GitHubRepository {
            owner: RepositoryOwner::from("Alice"),
            repository: RepositoryName::from("dotfiles"),
        }
    }

    fn document(applies_to: &str, body: &str) -> Configuration {
        let written = format!(
            r#"{{ "version": "{BUILD_GENERATION}", "applies_to": "{applies_to}",
               "github_account": "Alice", {body} }}"#
        );
        crate::configuration::parse_configuration(&written, "the test configuration")
            .unwrap_or_else(|refusal: Unreadable| panic!("{refusal}"))
            .configuration
    }

    fn read_from_the_dotfiles_repository(applies_to: &str, body: &str) -> ResolvedConfiguration {
        ResolvedConfiguration::read(
            document(applies_to, body),
            SourceLocation::Repository(dotfiles()),
            Path::new(REPOSITORIES_ROOT),
        )
    }

    fn read_from_a_checkout(applies_to: &str, body: &str) -> ResolvedConfiguration {
        ResolvedConfiguration::read(
            document(applies_to, body),
            SourceLocation::Checkout(PathBuf::from("/checkout")),
            Path::new(REPOSITORIES_ROOT),
        )
    }

    fn merged(configurations: Vec<(&str, ResolvedConfiguration)>) -> Result<DesiredState> {
        DesiredState::of(
            configurations
                .into_iter()
                .map(|(source, configuration)| (source.to_owned(), configuration))
                .collect(),
        )
    }

    const SYMLINK: &str = r#""resources": [{
        "kind": "symlink", "source_path": "gitconfig/.gitconfig", "link_path": ".gitconfig"
    }]"#;

    const EMPTY: &str = r#""resources": []"#;

    fn a_readable_set(personal: &str) -> Result<DesiredState> {
        merged(vec![
            (
                "everywhere",
                read_from_the_dotfiles_repository("everywhere", EMPTY),
            ),
            (
                "personal",
                read_from_the_dotfiles_repository("personal", personal),
            ),
        ])
    }

    #[test]
    fn a_configuration_read_from_a_repository_contributes_that_repository_as_a_clone() {
        let desired_state = a_readable_set(EMPTY).unwrap();

        assert_eq!(
            desired_state
                .resources
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["repository Alice/dotfiles".to_owned()]
        );
    }

    #[test]
    fn a_configuration_read_out_of_a_checkout_waits_for_no_clone() {
        let desired_state = merged(vec![
            ("everywhere", read_from_a_checkout("everywhere", EMPTY)),
            ("personal", read_from_a_checkout("personal", SYMLINK)),
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
            document("work", EMPTY),
            SourceLocation::Repository(dotfiles()),
            Path::new(REPOSITORIES_ROOT),
        );

        let desired_state = merged(vec![
            (
                "everywhere",
                read_from_the_dotfiles_repository("everywhere", EMPTY),
            ),
            ("work", work),
        ])
        .unwrap();

        let claimed: Vec<Identity> = desired_state
            .resources
            .iter()
            .filter_map(ResolvedResource::identity)
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
        let error = merged(vec![(
            "personal",
            read_from_the_dotfiles_repository("personal", EMPTY),
        )])
        .unwrap_err();

        assert!(error.to_string().contains("every machine"), "{error}");
    }

    #[test]
    fn a_set_holding_no_configuration_for_this_machines_class_is_refused() {
        let error = merged(vec![(
            "everywhere",
            read_from_the_dotfiles_repository("everywhere", EMPTY),
        )])
        .unwrap_err();

        assert!(error.to_string().contains("machine's class"), "{error}");
    }

    #[test]
    fn two_identical_claims_on_one_fact_collapse_to_a_single_resource() {
        let desired_state = merged(vec![
            (
                "everywhere",
                read_from_the_dotfiles_repository("everywhere", SYMLINK),
            ),
            (
                "personal",
                read_from_the_dotfiles_repository("personal", SYMLINK),
            ),
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
            (
                "everywhere",
                read_from_the_dotfiles_repository("everywhere", SYMLINK),
            ),
            (
                "personal",
                read_from_the_dotfiles_repository("personal", elsewhere),
            ),
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
            (
                "everywhere",
                read_from_the_dotfiles_repository("everywhere", command),
            ),
            (
                "personal",
                read_from_the_dotfiles_repository("personal", command),
            ),
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
            (
                "everywhere",
                read_from_the_dotfiles_repository("everywhere", workspace),
            ),
            (
                "personal",
                read_from_the_dotfiles_repository("personal", workspace),
            ),
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
        assert_eq!(command.identity(), None);
    }

    #[test]
    fn a_released_binary_claims_the_name_its_entry_carries_rather_than_the_whole_path() {
        let desired_state = a_readable_set(
            r#""resources": [{
                "kind": "application", "shape": "released_binary",
                "repository": { "owner": "BurntSushi", "repository": "ripgrep" },
                "asset": { "match": "ends_with", "value": ".zip" },
                "entry": "bin/rg.exe",
                "version_word": 2
            }]"#,
        )
        .unwrap();

        let binary = desired_state
            .resources
            .iter()
            .find(|resource| resource.kind() == ResourceKind::Application)
            .expect("the personal configuration declared a released binary");
        assert_eq!(
            binary.identity(),
            Some(Identity::InstalledBinary(
                crate::configuration::BinaryName::from("rg.exe")
            ))
        );
    }

    #[test]
    fn the_account_a_run_acts_as_is_the_one_the_configuration_for_every_machine_declares() {
        let desired_state = a_readable_set(EMPTY).unwrap();

        assert_eq!(*desired_state.account(), GitHubAccount::from("Alice"));
    }
}

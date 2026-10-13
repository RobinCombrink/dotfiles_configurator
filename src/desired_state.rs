use {
    crate::{
        configuration::{
            Application, CargoWorkspace, Configuration, ConfigurationName, Context, DeclaredNotice,
            EnvironmentVariable, GitHubAccount, GitHubRepository, Identity, Migration, Notice,
            Registration, RenderedManifest, Requirement, Resource, ResourceKind,
            SearchPathDirectory, SearchPathEntry,
        },
        configuration_source::AbsoluteDirectory,
        currency,
    },
    std::{
        collections::BTreeMap,
        fmt::{Display, Formatter},
        path::{Path, PathBuf},
    },
};

// ADR 0025
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLocation {
    Repository(GitHubRepository),
    Checkout(AbsoluteDirectory),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FilesRoot {
    Clone {
        repository: GitHubRepository,
        directory: PathBuf,
    },
    Checkout(AbsoluteDirectory),
}

impl FilesRoot {
    fn path(&self) -> &Path {
        match self {
            FilesRoot::Clone { directory, .. } => directory,
            FilesRoot::Checkout(directory) => directory.as_ref(),
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
pub type ResolvedNotice = Resolved<DeclaredNotice>;

impl<T> Resolved<T> {
    pub fn declared(&self) -> &T {
        &self.value
    }

    pub fn files_root(&self) -> &Path {
        self.origin.files_root.path()
    }

    pub fn account(&self) -> &GitHubAccount {
        &self.origin.account
    }

    pub fn clone_directory(&self, repository: &GitHubRepository) -> PathBuf {
        self.origin
            .repositories_directory
            .join(repository.repository.as_ref())
    }

    pub fn alongside<U>(&self, value: U) -> Resolved<U> {
        Resolved {
            value,
            origin: self.origin.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precedence {
    TheRunningBuild,
    Declared(ResourceKind),
}

impl ResolvedResource {
    pub fn kind(&self) -> ResourceKind {
        self.value.kind()
    }

    pub fn identity(&self, home_directory: &Path) -> Option<Identity> {
        self.value
            .identity_within(&self.origin.repositories_directory, home_directory)
    }

    // ADR 0019
    pub fn replaces_the_running_build(&self) -> bool {
        match &self.value {
            Resource::Application(Application::ReleasedBinary(binary)) => {
                *binary == currency::own_currency()
            }
            Resource::Application(Application::Installer(_))
            | Resource::Repository(_)
            | Resource::Package(_)
            | Resource::EnvironmentVariable(_)
            | Resource::Symlink(_)
            | Resource::Registration(_)
            | Resource::Command(_) => false,
        }
    }

    // ADR 0004, ADR 0035
    pub fn precedence(&self) -> Precedence {
        match self.replaces_the_running_build() {
            true => Precedence::TheRunningBuild,
            false => Precedence::Declared(self.kind()),
        }
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
            | Resource::EnvironmentVariable(_)
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
    name: ConfigurationName,
    origin: Origin,
    context: Context,
    workspaces: Vec<CargoWorkspace>,
    resources: Vec<Resource>,
    notices: Vec<DeclaredNotice>,
}

impl ResolvedConfiguration {
    pub fn read(
        name: ConfigurationName,
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
            name,
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

    pub fn name(&self) -> &ConfigurationName {
        &self.name
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
            .map(|repository| self.pair(Resource::Repository(repository.into())));

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
    pub undeclared: Vec<Identity>,
}

#[cfg(test)]
pub(crate) const HOME_DIRECTORY_IN_TESTS: &str = "/home/Alice";

#[cfg(test)]
impl DesiredState {
    pub(crate) fn other_than_what_every_change_set_carries(&self) -> Vec<&ResolvedResource> {
        self.resources
            .iter()
            .filter(|resource| {
                resource
                    .identity(Path::new(HOME_DIRECTORY_IN_TESTS))
                    .is_none_or(|identity| !self.undeclared.contains(&identity))
            })
            .collect()
    }
}

impl DesiredState {
    pub fn also_reporting(
        mut self,
        migrations: Vec<Migration>,
        announcements: Vec<Notice>,
    ) -> Self {
        self.migrations = migrations;
        self.announcements = announcements;
        self
    }

    pub fn of(
        configurations: Vec<ResolvedConfiguration>,
        machine_manifest: RenderedManifest,
        home_directory: &Path,
    ) -> Result<Self, Irreconcilable> {
        let for_every_machine = the_configuration_for_every_machine(&configurations)?;
        refuse_a_set_holding_nothing_for_this_class(&configurations)?;

        let mut workspaces: Vec<ResolvedWorkspace> = Vec::new();
        let mut notices: Vec<ResolvedNotice> = Vec::new();
        let mut claimed: BTreeMap<Identity, Claim> = BTreeMap::new();

        // ADR 0019
        let own_currency = for_every_machine.pair(Resource::Application(
            Application::ReleasedBinary(currency::own_currency()),
        ));
        // ADR 0017
        let own_binaries_are_reachable = for_every_machine.pair(Resource::EnvironmentVariable(
            EnvironmentVariable::SearchPathEntry(SearchPathEntry {
                directory: SearchPathDirectory::ToolBinaries,
            }),
        ));

        // ADR 0030
        let machine_manifest = for_every_machine.pair(Resource::Registration(
            Registration::MachineManifest(machine_manifest),
        ));

        let mut resources: Vec<ResolvedResource> = Vec::new();
        let mut undeclared: Vec<Identity> = Vec::new();
        for carried in [own_currency, own_binaries_are_reachable, machine_manifest] {
            if let Some(identity) = carried.identity(home_directory) {
                claimed.insert(
                    identity.clone(),
                    Claim {
                        by: ConfigurationName::from(THIS_BUILD),
                        resource: carried.clone(),
                    },
                );
                undeclared.push(identity);
            }
            resources.push(carried);
        }

        for configuration in &configurations {
            let source = configuration.name();
            for resource in configuration.resources() {
                match resource.identity(home_directory) {
                    None => resources.push(resource),
                    Some(identity) => match claimed.get(&identity) {
                        None => {
                            claimed.insert(
                                identity,
                                Claim {
                                    by: source.clone(),
                                    resource: resource.clone(),
                                },
                            );
                            resources.push(resource);
                        }
                        Some(claim) if claim.resource == resource => {}
                        Some(claim) => {
                            return Err(Irreconcilable::ConflictingClaims {
                                identity,
                                first: Box::new(claim.clone()),
                                second: Box::new(Claim {
                                    by: source.clone(),
                                    resource,
                                }),
                            });
                        }
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
            undeclared,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    by: ConfigurationName,
    resource: ResolvedResource,
}

#[derive(Debug)]
pub enum Irreconcilable {
    NoConfigurationForEveryMachine,
    NoConfigurationForThisClass,
    ConflictingClaims {
        identity: Identity,
        first: Box<Claim>,
        second: Box<Claim>,
    },
    UnrenderableManifest(serde_json::Error),
}

const ONE_FOR_EACH: &str =
    "A run reads one configuration for every machine and exactly one for this machine's class.";

impl Display for Irreconcilable {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Irreconcilable::NoConfigurationForEveryMachine => write!(
                formatter,
                "No configuration for every machine was loaded. {ONE_FOR_EACH}"
            ),
            Irreconcilable::NoConfigurationForThisClass => write!(
                formatter,
                "No configuration for this machine's class was loaded. {ONE_FOR_EACH}"
            ),
            Irreconcilable::ConflictingClaims {
                identity,
                first,
                second,
            } => write!(
                formatter,
                "{} and {} make conflicting claims on {identity}. No machine could satisfy \
                 both:\n  {}\n  {}",
                second.by, first.by, first.resource, second.resource
            ),
            Irreconcilable::UnrenderableManifest(failure) => write!(
                formatter,
                "The machine manifest could not be written out: {failure}"
            ),
        }
    }
}

impl std::error::Error for Irreconcilable {}

const THIS_BUILD: &str = "this build";

fn the_configuration_for_every_machine(
    configurations: &[ResolvedConfiguration],
) -> Result<&ResolvedConfiguration, Irreconcilable> {
    configurations
        .iter()
        .find(|configuration| configuration.context() == Context::Everywhere)
        .ok_or(Irreconcilable::NoConfigurationForEveryMachine)
}

fn refuse_a_set_holding_nothing_for_this_class(
    configurations: &[ResolvedConfiguration],
) -> Result<(), Irreconcilable> {
    match configurations
        .iter()
        .any(|configuration| configuration.context() != Context::Everywhere)
    {
        true => Ok(()),
        false => Err(Irreconcilable::NoConfigurationForThisClass),
    }
}

#[cfg(test)]
mod tests;

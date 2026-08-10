use {
    crate::configuration::{
        names::{
            ApplicationName, BinaryName, CrateName, McpServerName, VariableName, WingetPackageId,
        },
        resource::{
            Application, ClaudeMcpServer, EnvironmentVariable, GitHubRepository, Package,
            Registration, Resource, SearchPathDirectory, SearchPathEntry, Symlink, Variable,
        },
    },
    std::{
        fmt::Display,
        path::{Path, PathBuf},
    },
};

/// The machine fact a resource claims, by which two declarations are recognised as the same
/// resource. Identical claims collapse to one resource; conflicting claims on one fact are
/// rejected when the configuration is loaded, because no machine could satisfy both.
///
/// A command claims no fact and so has no identity, which is part of what makes it the last
/// resort.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Identity {
    ClonedRepository(PathBuf),
    Application(ApplicationName),
    InstalledBinary(BinaryName),
    WingetPackage(WingetPackageId),
    CargoCrate(CrateName),
    EnvironmentVariable(VariableName),
    /// The directory the entry names, as declared.
    SearchPathEntry(SearchPathDirectory),
    /// The path of the link itself, as declared.
    Symlink(PathBuf),
    ClaudeMcpServer(McpServerName),
}

impl Display for Identity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Identity::ClonedRepository(directory) => {
                write!(formatter, "the clone directory {}", directory.display())
            }
            Identity::Application(name) => write!(formatter, "the application {name}"),
            Identity::InstalledBinary(name) => {
                write!(formatter, "the binary {name} in the tool directory")
            }
            Identity::WingetPackage(id) => write!(formatter, "the winget package {id}"),
            Identity::CargoCrate(name) => write!(formatter, "the cargo crate {name}"),
            Identity::EnvironmentVariable(name) => {
                write!(formatter, "the environment variable {name}")
            }
            Identity::SearchPathEntry(directory) => {
                write!(formatter, "the search path entry {directory}")
            }
            Identity::Symlink(path) => write!(formatter, "the link at {}", path.display()),
            Identity::ClaudeMcpServer(name) => {
                write!(formatter, "the claude mcp server {name}")
            }
        }
    }
}

impl Resource {
    pub(crate) fn identity_within(&self, repositories_directory: &Path) -> Option<Identity> {
        match self {
            Resource::Repository(GitHubRepository { repository, .. }) => Some(
                Identity::ClonedRepository(repositories_directory.join(repository.as_ref())),
            ),
            Resource::Application(Application::Installer(installer)) => {
                Some(Identity::Application(installer.name.clone()))
            }
            Resource::Application(Application::ReleasedBinary(binary)) => {
                Some(Identity::InstalledBinary(binary.installed_name()))
            }
            Resource::Package(Package::Winget(package)) => {
                Some(Identity::WingetPackage(package.id.clone()))
            }
            Resource::Package(Package::Cargo(package)) => {
                Some(Identity::CargoCrate(package.crate_name.clone()))
            }
            Resource::EnvironmentVariable(EnvironmentVariable::Variable(Variable {
                name, ..
            })) => Some(Identity::EnvironmentVariable(name.clone())),
            Resource::EnvironmentVariable(EnvironmentVariable::SearchPathEntry(
                SearchPathEntry { directory },
            )) => Some(Identity::SearchPathEntry(directory.clone())),
            Resource::Symlink(Symlink { link_path, .. }) => {
                Some(Identity::Symlink(link_path.clone()))
            }
            Resource::Registration(Registration::ClaudeMcpServer(ClaudeMcpServer {
                name, ..
            })) => Some(Identity::ClaudeMcpServer(name.clone())),
            Resource::Command(_) => None,
        }
    }
}

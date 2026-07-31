use {
    crate::configuration::{
        names::{ApplicationName, CrateName, McpServerName, RepositoryName, WingetPackageId},
        resource::{
            Application, ClaudeMcpServer, GitHubRepository, Package, Registration, Resource,
            Symlink,
        },
    },
    std::{fmt::Display, path::PathBuf},
};

/// The machine fact a resource claims, by which two declarations are recognised as the same
/// resource. Identical claims collapse to one resource; conflicting claims on one fact are
/// rejected when the configuration is loaded, because no machine could satisfy both.
///
/// A command claims no fact and so has no identity, which is part of what makes it the last
/// resort.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Identity {
    /// The directory the repository is cloned into, which is named by the repository alone
    /// because every configuration shares one repositories directory.
    ClonedRepository(RepositoryName),
    Application(ApplicationName),
    WingetPackage(WingetPackageId),
    CargoCrate(CrateName),
    /// The path of the link itself, as declared.
    Symlink(PathBuf),
    ClaudeMcpServer(McpServerName),
}

impl Display for Identity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Identity::ClonedRepository(repository) => {
                write!(formatter, "the clone directory for {repository}")
            }
            Identity::Application(name) => write!(formatter, "the application {name}"),
            Identity::WingetPackage(id) => write!(formatter, "the winget package {id}"),
            Identity::CargoCrate(name) => write!(formatter, "the cargo crate {name}"),
            Identity::Symlink(path) => write!(formatter, "the link at {}", path.display()),
            Identity::ClaudeMcpServer(name) => {
                write!(formatter, "the claude mcp server {name}")
            }
        }
    }
}

impl Resource {
    /// The fact this resource claims, or `None` for a command, which claims none.
    pub fn identity(&self) -> Option<Identity> {
        match self {
            Resource::Repository(GitHubRepository { repository, .. }) => {
                Some(Identity::ClonedRepository(repository.clone()))
            }
            Resource::Application(Application { name, .. }) => {
                Some(Identity::Application(name.clone()))
            }
            Resource::Package(Package::Winget(package)) => {
                Some(Identity::WingetPackage(package.id.clone()))
            }
            Resource::Package(Package::Cargo(package)) => {
                Some(Identity::CargoCrate(package.crate_name.clone()))
            }
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

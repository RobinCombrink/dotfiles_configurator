use {
    crate::configuration::{
        names::{
            ApplicationName, BinaryName, CrateName, McpServerName, UvToolName, VariableName,
            WingetPackageId,
        },
        path_folding,
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct LinkPath(String);

impl LinkPath {
    pub fn under_home(home_directory: &Path, declared: &Path) -> Self {
        LinkPath(path_folding::comparable(&path_folding::home_relative_path(
            home_directory,
            declared,
        )))
    }
}

impl Display for LinkPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Identity {
    ClonedRepository(PathBuf),
    Application(ApplicationName),
    InstalledBinary(BinaryName),
    WingetPackage(WingetPackageId),
    CargoCrate(CrateName),
    UvTool(UvToolName),
    EnvironmentVariable(VariableName),
    SearchPathEntry(SearchPathDirectory),
    Symlink(LinkPath),
    ClaudeMcpServer(McpServerName),
    MachineManifest,
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
            Identity::UvTool(name) => write!(formatter, "the uv tool {name}"),
            Identity::EnvironmentVariable(name) => {
                write!(formatter, "the environment variable {name}")
            }
            Identity::SearchPathEntry(directory) => {
                write!(formatter, "the search path entry {directory}")
            }
            Identity::Symlink(link_path) => write!(formatter, "the link at {link_path}"),
            Identity::ClaudeMcpServer(name) => {
                write!(formatter, "the claude mcp server {name}")
            }
            Identity::MachineManifest => formatter.write_str("the machine manifest"),
        }
    }
}

impl Resource {
    pub(crate) fn identity_within(
        &self,
        repositories_directory: &Path,
        home_directory: &Path,
    ) -> Option<Identity> {
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
            Resource::Package(Package::UvTool(package)) => {
                Some(Identity::UvTool(package.name.clone()))
            }
            Resource::EnvironmentVariable(EnvironmentVariable::Variable(Variable {
                name, ..
            })) => Some(Identity::EnvironmentVariable(name.clone())),
            Resource::EnvironmentVariable(EnvironmentVariable::SearchPathEntry(
                SearchPathEntry { directory },
            )) => Some(Identity::SearchPathEntry(directory.clone())),
            Resource::Symlink(Symlink { link_path, .. }) => Some(Identity::Symlink(
                LinkPath::under_home(home_directory, link_path),
            )),
            Resource::Registration(Registration::ClaudeMcpServer(ClaudeMcpServer {
                name, ..
            })) => Some(Identity::ClaudeMcpServer(name.clone())),
            Resource::Registration(Registration::MachineManifest(_)) => {
                Some(Identity::MachineManifest)
            }
            Resource::Command(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_link_paths_spelled_in_different_case_resolve_to_the_same_identity() {
        let home_directory = Path::new("C:\\Users\\Alice");

        assert_eq!(
            LinkPath::under_home(home_directory, Path::new(".gitconfig")),
            LinkPath::under_home(home_directory, Path::new(".GITCONFIG"))
        );
    }

    #[test]
    fn two_link_paths_spelled_with_different_separators_resolve_to_the_same_identity() {
        let home_directory = Path::new("C:\\Users\\Alice");

        assert_eq!(
            LinkPath::under_home(home_directory, Path::new("config/git/config")),
            LinkPath::under_home(home_directory, Path::new("config\\git\\config"))
        );
    }

    #[test]
    fn two_link_paths_naming_different_files_resolve_to_different_identities() {
        let home_directory = Path::new("C:\\Users\\Alice");

        assert_ne!(
            LinkPath::under_home(home_directory, Path::new(".gitconfig")),
            LinkPath::under_home(home_directory, Path::new(".npmrc"))
        );
    }

    #[test]
    fn an_absolute_link_path_is_resolved_without_the_home_directory() {
        let absolute = if cfg!(windows) {
            "C:\\elsewhere\\.gitconfig"
        } else {
            "/elsewhere/.gitconfig"
        };
        let declared = Path::new(absolute);

        assert_eq!(
            LinkPath::under_home(Path::new("C:\\Users\\Alice"), declared),
            LinkPath::under_home(Path::new("C:\\Users\\Bob"), declared)
        );
    }
}

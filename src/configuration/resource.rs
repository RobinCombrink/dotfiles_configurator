use {
    crate::configuration::presence_check::PresenceCheck,
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{collections::BTreeMap, fmt::Display, path::PathBuf},
    url::Url,
};

/// One declared fact about a machine, whose actual state can be read and whose drift can be
/// closed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resource {
    Repository(RepositoryName),
    Application(Application),
    Package(Package),
    Symlink(Symlink),
    Registration(Registration),
    Command(Command),
}

/// The category a resource belongs to, which determines how its actual state is read.
///
/// The declaration order below is the order kinds are converged in, and it is load-bearing for
/// safety rather than presentation: a program initialising its configuration for the first time
/// writes through a symlink into the dotfiles repository, so applications must be installed
/// before anything links into their configuration directories. See ADR 0004.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResourceKind {
    Repository,
    Application,
    Package,
    Symlink,
    Registration,
    Command,
}

impl Display for ResourceKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ResourceKind::Repository => "repository",
            ResourceKind::Application => "application",
            ResourceKind::Package => "package",
            ResourceKind::Symlink => "symlink",
            ResourceKind::Registration => "registration",
            ResourceKind::Command => "command",
        };
        formatter.write_str(name)
    }
}

impl Resource {
    pub fn kind(&self) -> ResourceKind {
        match self {
            Resource::Repository(_) => ResourceKind::Repository,
            Resource::Application(_) => ResourceKind::Application,
            Resource::Package(_) => ResourceKind::Package,
            Resource::Symlink(_) => ResourceKind::Symlink,
            Resource::Registration(_) => ResourceKind::Registration,
            Resource::Command(_) => ResourceKind::Command,
        }
    }
}

impl Display for Resource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Resource::Repository(name) => write!(formatter, "repository {name}"),
            Resource::Application(application) => {
                write!(formatter, "application {}", application.name)
            }
            Resource::Package(package) => write!(formatter, "package {package}"),
            Resource::Symlink(symlink) => write!(
                formatter,
                "symlink {} -> {}",
                symlink.link_path.display(),
                symlink.source_path.display()
            ),
            Resource::Registration(registration) => {
                write!(formatter, "registration {registration}")
            }
            Resource::Command(command) => write!(formatter, "command {}", command.rendered()),
        }
    }
}

/// A repository on GitHub, named by the owner and repository it is cloned from.
#[derive(
    Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct RepositoryName {
    pub owner: String,
    pub repo: String,
}

impl Display for RepositoryName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}/{}", self.owner, self.repo)
    }
}

/// The name an application is known by, and the name its downloaded installer is written under.
#[derive(
    Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct ApplicationName(String);

impl From<String> for ApplicationName {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ApplicationName {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl Display for ApplicationName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A program installed by downloading and running an installer, whose presence the machine cannot
/// be asked about directly and which therefore declares a presence check.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Application {
    pub name: ApplicationName,
    pub source: ApplicationSource,
    pub presence_check: PresenceCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ApplicationSource {
    /// An installer downloaded straight from a URL.
    Uri {
        uri: Url,
        installer_file_name: String,
    },
    /// An installer downloaded from the latest release of a GitHub repository.
    GitHubRelease {
        owner: String,
        repo: String,
        asset: AssetPattern,
    },
}

/// How the wanted asset is picked out of a release's assets.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "match", content = "value", rename_all = "snake_case")]
pub enum AssetPattern {
    Exact(String),
    Contains(String),
    EndsWith(String),
}

impl AssetPattern {
    pub fn matches(&self, asset_name: &str) -> bool {
        match self {
            AssetPattern::Exact(exact) => asset_name == exact,
            AssetPattern::Contains(fragment) => asset_name.contains(fragment),
            AssetPattern::EndsWith(suffix) => asset_name.ends_with(suffix),
        }
    }
}

/// A resource whose installation is owned by a package manager, which is consequently also what
/// can be asked whether it is installed. Which manager owns it is part of what the resource is,
/// so it is the variant rather than a field.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "manager", rename_all = "snake_case")]
pub enum Package {
    Winget(WingetPackage),
    Cargo(CargoPackage),
}

impl Display for Package {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Package::Winget(package) => write!(formatter, "winget {}", package.id),
            Package::Cargo(package) => write!(formatter, "cargo {}", package.crate_name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetPackage {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CargoPackage {
    pub crate_name: String,
    pub source: CargoSource,
}

/// Where Cargo installs a crate from. Cargo reports this back in `cargo install --list`, which is
/// what makes a cargo package readable: a registry install is bare, a path install carries its
/// path, and a git install carries the revision it was asked for and the commit that resolved to.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum CargoSource {
    Registry,
    Path { path: PathBuf },
    Git { url: Url, revision: String },
}

/// A configuration file or directory owned by the dotfiles repository and linked into place on the
/// machine, rather than copied.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Symlink {
    /// Path within the dotfiles repository that the link points at.
    pub source_path: PathBuf,
    /// Path of the link itself, resolved against the home directory unless it is absolute.
    pub link_path: PathBuf,
}

/// A record this machine holds in another tool's configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "registry", rename_all = "snake_case")]
pub enum Registration {
    ClaudeMcpServer(ClaudeMcpServer),
}

impl Display for Registration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Registration::ClaudeMcpServer(server) => {
                write!(formatter, "claude mcp server {}", server.name)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ClaudeMcpServer {
    pub name: String,
    pub scope: McpScope,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpScope {
    User,
    Project,
    Local,
}

impl McpScope {
    pub fn as_argument(&self) -> &'static str {
        match self {
            McpScope::User => "user",
            McpScope::Project => "project",
            McpScope::Local => "local",
        }
    }
}

/// An arbitrary shell invocation, and the resource kind of last resort. A command claims no
/// machine fact and so has no identity; one without a presence check has drift on every run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Command {
    pub shell: Shell,
    pub args: Vec<String>,
    #[serde(default)]
    pub presence_check: Option<PresenceCheck>,
}

impl Command {
    pub fn rendered(&self) -> String {
        self.args.join(" ")
    }
}

// `PowerShell` shares the enum's ending because that is the shell's name; shortening it to
// satisfy the lint would name the variant after nothing anyone runs.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    Bash,
    CommandPrompt,
    PowerShell,
    Wsl,
}

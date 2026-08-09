use {
    crate::{
        configuration::{
            names::{
                ApplicationName, BinaryName, CrateName, McpServerName, RepositoryName,
                RepositoryOwner, WingetPackageId,
            },
            presence_check::PresenceCheck,
        },
        version::Version,
    },
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{collections::BTreeMap, fmt::Display, num::NonZeroUsize, path::PathBuf},
    url::Url,
};

/// One declared fact about a machine, whose actual state can be read and whose drift can be
/// closed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resource {
    Repository(GitHubRepository),
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
            Resource::Application(application) => write!(formatter, "application {application}"),
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

/// A repository on GitHub, named by the owner it belongs to and its own name.
#[derive(
    Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct GitHubRepository {
    pub owner: RepositoryOwner,
    pub repository: RepositoryName,
}

impl GitHubRepository {
    pub fn clone_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repository)
    }
}

impl Display for GitHubRepository {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}/{}", self.owner, self.repository)
    }
}

// ADR 0016
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Application {
    Installer(Installer),
    ReleasedBinary(ReleasedBinary),
}

impl Display for Application {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Application::Installer(installer) => Display::fmt(&installer.name, formatter),
            Application::ReleasedBinary(binary) => {
                Display::fmt(&binary.entry.installed_name(), formatter)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Installer {
    pub name: ApplicationName,
    pub source: ApplicationSource,
    pub presence_check: PresenceCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ReleasedBinary {
    pub repository: GitHubRepository,
    pub asset: AssetPattern,
    pub entry: ArchiveEntry,
    #[serde(default = "asking_for_a_version")]
    pub version_arguments: Vec<String>,
    pub version_word: VersionWord,
}

fn asking_for_a_version() -> Vec<String> {
    vec!["--version".to_owned()]
}

impl ReleasedBinary {
    pub fn installed_name(&self) -> BinaryName {
        self.entry.installed_name()
    }

    pub fn rendered_version_invocation(&self) -> String {
        format!(
            "{} {}",
            self.installed_name(),
            self.version_arguments.join(" ")
        )
        .trim_end()
        .to_owned()
    }

    pub fn reported_version(&self, output: &str) -> Result<Version, String> {
        let word = self.version_word.select(output).ok_or_else(|| {
            format!(
                "`{}` printed no {} word to read a version from",
                self.rendered_version_invocation(),
                self.version_word
            )
        })?;

        Version::try_from(word).map_err(|fault| {
            format!(
                "the {} word `{}` printed is {fault}",
                self.version_word,
                self.rendered_version_invocation()
            )
        })
    }
}

/// ```
/// use dotfiles_configurator::configuration::ArchiveEntry;
///
/// let nested = ArchiveEntry::try_from("bin/rg.exe".to_owned()).unwrap();
///
/// assert_eq!(nested.installed_name().to_string(), "rg.exe");
/// assert!(ArchiveEntry::try_from("bin/".to_owned()).is_err());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(try_from = "String", into = "String")]
pub struct ArchiveEntry(String);

impl ArchiveEntry {
    pub fn installed_name(&self) -> BinaryName {
        BinaryName::from(self.0.rsplit('/').next().unwrap_or(&self.0))
    }
}

impl TryFrom<String> for ArchiveEntry {
    type Error = String;

    fn try_from(path: String) -> Result<Self, Self::Error> {
        match path.rsplit('/').next().unwrap_or_default().is_empty() {
            true => Err(format!(
                "{path:?} names no file inside the archive to install"
            )),
            false => Ok(Self(path)),
        }
    }
}

impl From<ArchiveEntry> for String {
    fn from(entry: ArchiveEntry) -> Self {
        entry.0
    }
}

impl Display for ArchiveEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(transparent)]
#[repr(transparent)]
pub struct VersionWord(NonZeroUsize);

impl VersionWord {
    /// ```
    /// use {dotfiles_configurator::configuration::VersionWord, std::num::NonZeroUsize};
    ///
    /// let third = VersionWord::from(NonZeroUsize::new(3).unwrap());
    ///
    /// assert_eq!(third.select("gh version 2.80.0 (2025-09-23)"), Some("2.80.0"));
    /// assert_eq!(third.select("ripgrep 15.1.0"), None);
    /// ```
    pub fn select<'output>(&self, output: &'output str) -> Option<&'output str> {
        output.split_whitespace().nth(self.0.get() - 1)
    }
}

impl From<NonZeroUsize> for VersionWord {
    fn from(position: NonZeroUsize) -> Self {
        Self(position)
    }
}

impl Display for VersionWord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let suffix = match (self.0.get() % 10, self.0.get() % 100) {
            (_, 11..=13) => "th",
            (1, _) => "st",
            (2, _) => "nd",
            (3, _) => "rd",
            _ => "th",
        };
        write!(formatter, "{}{suffix}", self.0)
    }
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
        owner: RepositoryOwner,
        repository: RepositoryName,
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

impl Display for AssetPattern {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetPattern::Exact(exact) => write!(formatter, "the asset named {exact:?}"),
            AssetPattern::Contains(fragment) => {
                write!(formatter, "an asset holding {fragment:?}")
            }
            AssetPattern::EndsWith(suffix) => write!(formatter, "an asset ending in {suffix:?}"),
        }
    }
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
    pub id: WingetPackageId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CargoPackage {
    pub crate_name: CrateName,
    pub source: CargoSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum CargoSource {
    Registry,
    Path {
        path: PathBuf,
    },
    #[serde(skip_deserializing)]
    #[schemars(skip)]
    Workspace {
        repository: GitHubRepository,
    },
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
    pub name: McpServerName,
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

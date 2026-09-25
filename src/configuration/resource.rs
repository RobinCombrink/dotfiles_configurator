use {
    crate::{
        configuration::{
            estate::Estates,
            names::{
                ApplicationName, BinaryName, CrateName, GitHubAccount, McpServerName,
                PythonInterpreter, RepositoryName, RepositoryOwner, UvToolName, VariableName,
                VariableValue, WingetPackageId,
            },
            presence_check::PresenceCheck,
            requirement::{Requirement, Tool},
        },
        version::Version,
    },
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{
        collections::BTreeMap,
        fmt::Display,
        num::NonZeroUsize,
        path::{Path, PathBuf},
    },
    strum::EnumDiscriminants,
    url::Url,
};

// ADR 0004
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, EnumDiscriminants)]
#[schemars(
    description = "One declared fact about a machine, whose actual state can be read and whose \
                   drift can be\n\
                   closed."
)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum_discriminants(name(ResourceKind), derive(PartialOrd, Ord, Hash))]
pub enum Resource {
    Repository(GitHubRepository),
    Application(Application),
    Package(Package),
    EnvironmentVariable(EnvironmentVariable),
    Symlink(Symlink),
    Registration(Registration),
    Command(Command),
}

impl Display for ResourceKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ResourceKind::Repository => "repository",
            ResourceKind::Application => "application",
            ResourceKind::Package => "package",
            ResourceKind::EnvironmentVariable => "environment variable",
            ResourceKind::Symlink => "symlink",
            ResourceKind::Registration => "registration",
            ResourceKind::Command => "command",
        };
        formatter.write_str(name)
    }
}

impl Resource {
    pub fn kind(&self) -> ResourceKind {
        ResourceKind::from(self)
    }

    pub fn can_be_read_back(&self) -> bool {
        match self {
            Resource::Command(command) => command.presence_check.is_some(),
            Resource::Repository(_)
            | Resource::Application(_)
            | Resource::Package(_)
            | Resource::EnvironmentVariable(_)
            | Resource::Symlink(_)
            | Resource::Registration(_) => true,
        }
    }

    pub(crate) fn tool_requirements(&self) -> Vec<Requirement> {
        match self {
            Resource::Repository(_) => Vec::new(),
            Resource::Application(Application::Installer(installer)) => {
                check_requirements(Some(&installer.presence_check))
            }
            Resource::Application(Application::ReleasedBinary(_)) => Vec::new(),
            Resource::Package(Package::Winget(_)) => vec![Requirement::Tool(Tool::Winget)],
            Resource::Package(Package::UvTool(_)) => vec![Requirement::Tool(Tool::Uv)],
            Resource::Package(Package::Cargo(package)) => match package.source {
                CargoSource::Workspace { .. } => {
                    vec![Requirement::Tool(Tool::Cargo), Requirement::Tool(Tool::Git)]
                }
                CargoSource::Registry | CargoSource::Path { .. } => {
                    vec![Requirement::Tool(Tool::Cargo)]
                }
            },
            Resource::EnvironmentVariable(_) | Resource::Symlink(_) => Vec::new(),
            Resource::Registration(_) => vec![Requirement::Tool(Tool::Claude)],
            Resource::Command(command) => {
                let mut requirements = check_requirements(command.presence_check.as_ref());
                requirements.extend(shell_requirement(command.shell));
                requirements
            }
        }
    }
}

fn check_requirements(check: Option<&PresenceCheck>) -> Vec<Requirement> {
    match check {
        Some(PresenceCheck::CommandOutputContains { shell, .. }) => {
            shell_requirement(*shell).into_iter().collect()
        }
        Some(_) | None => Vec::new(),
    }
}

fn shell_requirement(shell: Shell) -> Option<Requirement> {
    match shell {
        Shell::Wsl => Some(Requirement::Tool(Tool::Wsl)),
        Shell::Bash | Shell::CommandPrompt | Shell::PowerShell => None,
    }
}

impl Display for Resource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Resource::Repository(name) => write!(formatter, "repository {name}"),
            Resource::Application(application) => write!(formatter, "application {application}"),
            Resource::Package(package) => write!(formatter, "package {package}"),
            Resource::EnvironmentVariable(variable) => write!(formatter, "{variable}"),
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

#[derive(
    Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[schemars(
    description = "A repository on GitHub, named by the owner it belongs to and its own name."
)]
pub struct GitHubRepository {
    pub owner: RepositoryOwner,
    pub repository: RepositoryName,
}

impl GitHubRepository {
    /// The URL a git fetch is made against, naming the account to fetch as so that a credential
    /// helper holding several can pick the right one. It carries no credential itself.
    ///
    /// ```
    /// # use dotfiles_configurator::configuration::{GitHubAccount, GitHubRepository,
    /// #     RepositoryName, RepositoryOwner};
    /// let repository = GitHubRepository {
    ///     owner: RepositoryOwner::from("Employer"),
    ///     repository: RepositoryName::from("dotfiles"),
    /// };
    ///
    /// assert_eq!(
    ///     repository.fetch_url_as(&GitHubAccount::from("Alice")),
    ///     "https://Alice@github.com/Employer/dotfiles"
    /// );
    /// ```
    pub fn fetch_url_as(&self, account: &GitHubAccount) -> String {
        format!(
            "https://{account}@github.com/{}/{}",
            self.owner, self.repository
        )
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
                Display::fmt(&binary.installed_name(), formatter)
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
    #[serde(default = "default_version_arguments")]
    pub version_arguments: Vec<String>,
    pub version_word: VersionWord,
}

fn default_version_arguments() -> Vec<String> {
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
/// assert_eq!(nested.installed_name().file_name(), "rg.exe");
/// assert!(ArchiveEntry::try_from("bin/".to_owned()).is_err());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(try_from = "String", into = "String")]
pub struct ArchiveEntry(String);

impl ArchiveEntry {
    pub fn installed_name(&self) -> BinaryName {
        let own_name = own_name(&self.0);

        BinaryName::from(
            own_name
                .strip_suffix(std::env::consts::EXE_SUFFIX)
                .unwrap_or(own_name),
        )
    }
}

fn own_name(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((_, own_name)) => own_name,
        None => path,
    }
}

impl TryFrom<String> for ArchiveEntry {
    type Error = String;

    fn try_from(path: String) -> Result<Self, Self::Error> {
        match own_name(&path).is_empty() {
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
    /// assert_eq!(third.select("build source=binaryRelease, version=0.64.0,"), Some("0.64.0"));
    /// ```
    pub fn select<'output>(&self, output: &'output str) -> Option<&'output str> {
        let word = output.split_whitespace().nth(self.0.get() - 1)?;
        let undecorated = word.trim_end_matches(',');

        match undecorated.split_once('=') {
            Some((_, value)) => Some(value),
            None => Some(undecorated),
        }
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
    #[schemars(description = "An installer downloaded straight from a URL.")]
    Uri {
        uri: Url,
        installer_file_name: String,
    },
    #[schemars(
        description = "An installer downloaded from the latest release of a GitHub repository."
    )]
    GitHubRelease {
        owner: RepositoryOwner,
        repository: RepositoryName,
        asset: AssetPattern,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(description = "How the wanted asset is picked out of a release's assets.")]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(
    description = "A resource whose installation is owned by a package manager, which is \
                   consequently also what\n\
                   can be asked whether it is installed. Which manager owns it is part of what \
                   the resource is,\n\
                   so it is the variant rather than a field."
)]
#[serde(tag = "manager", rename_all = "snake_case")]
pub enum Package {
    Winget(WingetPackage),
    Cargo(CargoPackage),
    UvTool(UvToolPackage),
}

impl Display for Package {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Package::Winget(package) => write!(formatter, "winget {}", package.id),
            Package::Cargo(package) => write!(formatter, "cargo {}", package.crate_name),
            Package::UvTool(package) => write!(formatter, "uv tool {}", package.name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetPackage {
    pub id: WingetPackageId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(
    description = "A Python package uv installs as a tool in an environment of its own, kept at \
                   the newest\n\
                   version that resolves."
)]
pub struct UvToolPackage {
    pub name: UvToolName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<PythonInterpreter>,
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

// ADR 0017
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum EnvironmentVariable {
    Variable(Variable),
    SearchPathEntry(SearchPathEntry),
}

impl Display for EnvironmentVariable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvironmentVariable::Variable(variable) => {
                write!(formatter, "environment variable {variable}")
            }
            EnvironmentVariable::SearchPathEntry(entry) => {
                write!(formatter, "search path entry {}", entry.directory)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Variable {
    pub name: VariableName,
    pub value: VariableValue,
}

impl Display for Variable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}={}", self.name, self.value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SearchPathEntry {
    pub directory: SearchPathDirectory,
}

// ADR 0025
#[derive(
    Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(tag = "in", rename_all = "snake_case")]
pub enum SearchPathDirectory {
    // ADR 0019
    #[serde(skip_deserializing)]
    #[schemars(skip)]
    ToolBinaries,
    Repository {
        repository: GitHubRepository,
        path: PathBuf,
    },
    Home {
        path: PathBuf,
    },
}

impl Display for SearchPathDirectory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchPathDirectory::ToolBinaries => {
                formatter.write_str("the directory this program installs binaries into")
            }
            SearchPathDirectory::Repository { repository, path } => {
                write!(formatter, "{} in {repository}", path.display())
            }
            SearchPathDirectory::Home { path } => write!(formatter, "{}", path.display()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(
    description = "A configuration file or directory owned by the dotfiles repository and linked \
                   into place on the\n\
                   machine, rather than copied."
)]
pub struct Symlink {
    #[schemars(description = "Path within the dotfiles repository that the link points at.")]
    pub source_path: PathBuf,
    #[schemars(
        description = "Path of the link itself, resolved against the home directory unless it is \
                       absolute."
    )]
    pub link_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(description = "A record this machine holds in another tool's configuration.")]
#[serde(tag = "registry", rename_all = "snake_case")]
pub enum Registration {
    ClaudeMcpServer(ClaudeMcpServer),
    // ADR 0030
    #[serde(skip_deserializing)]
    #[schemars(skip)]
    MachineManifest(MachineManifest),
}

impl Display for Registration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Registration::ClaudeMcpServer(server) => {
                write!(formatter, "claude mcp server {}", server.name)
            }
            Registration::MachineManifest(manifest) => write!(
                formatter,
                "machine manifest naming {}",
                manifest.repositories_directory_path.display()
            ),
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MachineManifest {
    pub repositories_directory_path: PathBuf,
    pub estates: Estates,
}

const MANIFEST_DIRECTORY: &str = ".dotconfig";
const MANIFEST_FILE: &str = "machine.json";

impl MachineManifest {
    pub fn path_within(home_directory: &Path) -> PathBuf {
        home_directory.join(MANIFEST_DIRECTORY).join(MANIFEST_FILE)
    }
}

impl TryFrom<&MachineManifest> for String {
    type Error = serde_json::Error;

    fn try_from(manifest: &MachineManifest) -> Result<Self, Self::Error> {
        #[derive(Serialize)]
        struct Document<'manifest> {
            machine: &'manifest MachineManifest,
        }

        serde_json::to_string_pretty(&Document { machine: manifest })
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(
    description = "An arbitrary shell invocation, and the resource kind of last resort. A command \
                   claims no\n\
                   machine fact and so has no identity; one without a presence check has drift on \
                   every run."
)]
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::configuration::estate::{EstateName, EstateOwner},
        std::collections::BTreeSet,
    };

    #[test]
    fn an_archive_entry_is_known_by_a_name_carrying_no_platform_executable_suffix() {
        let entry =
            ArchiveEntry::try_from(format!("bin/rg{}", std::env::consts::EXE_SUFFIX)).unwrap();

        assert_eq!(entry.installed_name(), BinaryName::from("rg"));
    }

    #[test]
    fn the_manifest_declares_the_repositories_directory_under_a_machine_of_its_own() {
        let document = String::try_from(&MachineManifest {
            repositories_directory_path: PathBuf::from("/repositories/Personal"),
            estates: Estates::new(),
        })
        .expect("a manifest that serialises");

        assert_eq!(
            document,
            "{\n  \"machine\": {\n    \"repositories_directory_path\": \"/repositories/Personal\",\n    \
             \"estates\": {}\n  }\n}"
        );
    }

    #[test]
    fn the_manifest_names_each_estate_with_its_owners_as_they_were_spelled() {
        let estates = Estates::from([
            (
                EstateName::try_from("personal").unwrap(),
                BTreeSet::from([EstateOwner::from("RobinCombrink")]),
            ),
            (
                EstateName::try_from("work").unwrap(),
                BTreeSet::from([
                    EstateOwner::from("skynamo"),
                    EstateOwner::from("Robin-Combrink"),
                ]),
            ),
        ]);

        let document = String::try_from(&MachineManifest {
            repositories_directory_path: PathBuf::from("/repositories/Work"),
            estates,
        })
        .expect("a manifest that serialises");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&document).unwrap()["machine"]["estates"],
            serde_json::json!({
                "personal": ["RobinCombrink"],
                "work": ["Robin-Combrink", "skynamo"]
            })
        );
    }

    #[test]
    fn the_manifest_is_written_into_the_configuration_directory_under_the_home_directory() {
        assert_eq!(
            MachineManifest::path_within(Path::new("/home/alice")),
            Path::new("/home/alice/.dotconfig/machine.json")
        );
    }
}

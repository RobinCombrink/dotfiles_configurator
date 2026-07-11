use std::{fmt::Display, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::EnumIter;
use url::Url;

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigurationItem {
    Download(Download),
    Dotfile(DetailsType),
    GitClone(GitClone),
    ShellCommand(ShellCommand),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Download {
    Application(ApplicationDetails),
    GitHubAsset(RepositoryDetails),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
pub struct Configuration {
    pub version: String,
    pub clone_config: GitCloneConfig,
    pub items: Vec<ConfigurationItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, PartialEq, Eq, Hash)]
pub struct GitCloneConfig {
    /// Absolute path of directory to clone into
    pub repositories_directory_path: PathBuf,
    /// The username used when making requests to github
    pub github_username: String,
    pub dotfiles_repository: GitClone,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Downloads {
    pub applications: Vec<ApplicationDetails>,
    pub github_releases: Vec<RepositoryDetails>,
}

impl Default for Downloads {
    fn default() -> Self {
        Self {
            applications: vec![ApplicationDetails::default()],
            github_releases: vec![RepositoryDetails::default()],
        }
    }
}

#[derive(
    Debug, Serialize, Deserialize, Clone, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct ApplicationDetails {
    pub name: String,
    pub uri: Url,
    pub dotfiles: Option<Vec<DetailsType>>,
}

impl Default for ApplicationDetails {
    fn default() -> Self {
        Self {
            name: String::new(),
            uri: Url::parse("http://localhost").unwrap(),
            dotfiles: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged, rename_all = "snake_case")]
pub enum AssetFind {
    AssetExact { asset_exact: String },
    AssetContains { asset_contains: String },
    AssetEndsWith { asset_ends_with: String },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepositoryDetails {
    pub owner: String,
    pub repo: String,
    #[serde(flatten)]
    pub asset_find: Option<AssetFind>,
    pub shell_commands: Option<Vec<ShellCommand>>,
    pub dotfiles: Option<Vec<DetailsType>>,
}

impl Default for RepositoryDetails {
    fn default() -> Self {
        Self {
            owner: String::new(),
            repo: String::new(),
            asset_find: None,
            shell_commands: None,
            dotfiles: None,
        }
    }
}

#[derive(
    Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct CliCommand {
    pub args: Vec<String>,
    #[serde(default)]
    pub require_output: bool,
    pub interactive: bool,
}

#[derive(
    Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(tag = "shell")]
pub enum ShellCommand {
    Bash(CliCommand),
    CommandPrompt(CliCommand),
    Powershell(CliCommand),
    WSL(CliCommand),
}

impl ShellCommand {
    #[cfg(target_family = "windows")]
    pub fn new(args: Vec<String>, require_output: bool, interactive: bool) -> Self {
        Self::CommandPrompt(CliCommand {
            args,
            require_output,
            interactive,
        })
    }
    #[cfg(target_family = "unix")]
    pub fn new(args: Vec<String>, require_output: bool, interactive: bool) -> Self {
        Self::Bash(CliCommand {
            args,
            require_output,
            interactive,
        })
    }
}

#[cfg(target_family = "windows")]
impl Default for ShellCommand {
    fn default() -> Self {
        Self::CommandPrompt(CliCommand {
            interactive: false,
            require_output: false,
            args: vec![],
        })
    }
}

#[cfg(target_family = "unix")]
impl Default for ShellCommand {
    fn default() -> Self {
        Self::Bash(CliCommand {
            interactive: false,
            args: vec![],
        })
    }
}

#[derive(
    Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct GitClone {
    pub owner: String,
    pub repo: String,
    pub shell_commands: Option<Vec<ShellCommand>>,
}

impl Display for GitClone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("{}/{}", self.owner, self.repo))
    }
}

#[derive(
    Serialize, Deserialize, Debug, Clone, EnumIter, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum DetailsType {
    File(FileDetails),
    Directory(DirectoryDetails),
}

#[derive(
    Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct FileDetails {
    pub file_name: PathBuf,
    pub original_path: PathBuf,
    pub link_path: Option<PathBuf>,
}

impl Default for FileDetails {
    fn default() -> Self {
        Self {
            file_name: PathBuf::new(),
            original_path: PathBuf::new(),
            link_path: Some(PathBuf::new()),
        }
    }
}

#[derive(
    Serialize, Deserialize, Debug, Clone, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct DirectoryDetails {
    pub original_path: PathBuf,
    pub link_path: PathBuf,
}

impl Default for DirectoryDetails {
    fn default() -> Self {
        Self {
            original_path: PathBuf::new(),
            link_path: PathBuf::new(),
        }
    }
}

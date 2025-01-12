use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use strum::EnumIter;
use url::Url;
use schemars::JsonSchema;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Configuration {
    pub version: String,
    pub clone_config: GitCloneConfig,
    pub dotfiles_repository: GitClone,
    pub downloads: Downloads,
    #[serde(rename = "git_clones")]
    pub to_clones: Vec<GitClone>,
    pub dotfiles: Option<Vec<DetailsType>>,
    pub cli_commands: Option<Vec<CliCommand>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct GitCloneConfig {
    /// Absolute path of directory to clone into
    pub repositories_directory_path: PathBuf,
    /// The username used when making requests to github
    pub github_username: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct Downloads {
    pub applications: Vec<ApplicationDetails>,
    pub github_releases: Vec<RepositoryDetails>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct ApplicationDetails {
    pub name: String,
    pub uri: Url,
    pub dotfiles: Option<Vec<DetailsType>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[serde(untagged, rename_all = "snake_case")]
pub enum AssetFind {
    AssetExact { asset_exact: String },
    AssetContains { asset_contains: String },
    AssetEndsWith { asset_ends_with: String },
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct RepositoryDetails {
    pub owner: String,
    pub repo: String,
    #[serde(flatten)]
    pub asset_find: Option<AssetFind>,
    pub commands: Option<Vec<CliCommand>>,
    pub dotfiles: Option<Vec<DetailsType>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct CliCommand {
    pub(crate) interactive: bool,
    pub(crate) args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct GitClone {
    pub owner: String,
    pub repo: String,
    pub cli_commands: Option<Vec<CliCommand>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, EnumIter, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DetailsType {
    File(FileDetails),
    Directory(DirectoryDetails),
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
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

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
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

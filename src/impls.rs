use crate::config::{DownloadType, ExecutionPlan, ExecutionPlanItem};
use crate::dotfiles::DotfilesDetails;
use crate::progress_bar::{
    create_download_application_progress_bar, create_download_asset_progress_bar,
    create_progress_bar,
};
use crate::shell_command;
use crate::{download::Downloader, github};
use anyhow::{anyhow, Context, Result};
use common::configuration::{
    ApplicationDetails, AssetFind, Configuration, ConfigurationItem, Download, GitClone,
    GitCloneConfig, RepositoryDetails,
};
use futures::future::join_all;
use git2::build::RepoBuilder;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use log::{error, trace};
use reqwest::Client;
use secrecy::SecretString;
use serde::Serialize;
use std::borrow::Cow;
use std::time::Duration;
use std::{fs, path::PathBuf};
use tokio::task::JoinSet;

pub trait Executor {
    async fn execute(&self) -> Result<()>;
}

pub trait AssetDownloaderExecutor {}

pub trait FileDownloaderExecutor {}

pub trait DotfileExecutor {}

pub trait CloneExecutor {}

pub trait ShellCommandExecutor {}

pub trait ItemProgress {
    fn create_progress_bar(&self, path: PathBuf) -> ProgressBar;
}

impl ItemProgress for DownloadType {
    fn create_progress_bar(&self, path: PathBuf) -> ProgressBar {
        match self {
            DownloadType::Application(_) => create_download_application_progress_bar(),
            DownloadType::GitHubAsset(repository_details) => create_download_asset_progress_bar(
                &repository_details.owner,
                &repository_details.repo,
                path,
            ),
        }
    }
}

impl ItemProgress for GitClone {
    fn create_progress_bar(&self, path: PathBuf) -> ProgressBar {
        create_download_asset_progress_bar(&self.owner, &self.repo, path.join(&self.repo))
    }
}

pub trait ExecutorSync {
    fn execute_sync(&self) -> Result<String>;
}

#[derive(Serialize, Clone)]
pub struct Config {
    pub configuration: Configuration,
    download_directory: PathBuf,
    home_dir: PathBuf,
}

impl Config {
    pub async fn execute(self) -> Result<()> {
        let repositories_path = self
            .configuration
            .clone_config
            .repositories_directory_path
            .clone();
        let repo = self.configuration.clone_config.dotfiles_repository;
        let name = &repo.repo;
        // let dotfiles_repo_result =
        //     GitCloneArgs::from_gitclone(repo.clone(), repositories_path.clone(), None)
        //         .git_clone(Self::create_download_asset_progress_bar(
        //             &repo.owner,
        //             name,
        //             repositories_path.clone(),
        //         ))
        //         .await;

        Ok(())
    }
}

impl Downloader for ApplicationDetails {
    async fn download_self(
        &self,
        client: Client,
        download_directory: PathBuf,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        Self::download(
            client,
            &self.uri,
            &download_directory.join(&self.name),
            progress_bar,
        )
        .await
    }
}

impl Downloader for RepositoryDetails {
    async fn download_self(
        &self,
        client: Client,
        download_directory: PathBuf,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        let release = octocrab::instance()
            .repos(&self.owner, &self.repo)
            .releases()
            .get_latest()
            .await?;

        trace!("RepositoryDetails: {:#?}", self);

        let asset = &release
            .assets
            .iter()
            .find(|asset| {
                trace!("Asset name: {:#?}", asset.name);
                if let Some(asset_find) = &self.asset_find {
                    trace!("{:#?}", asset_find);
                    match asset_find {
                        AssetFind::AssetContains { asset_contains } => {
                            asset.name.contains(asset_contains)
                        }
                        AssetFind::AssetExact { asset_exact } => {
                            asset.name == *asset_exact
                                || asset
                                    .label
                                    .to_owned()
                                    .is_some_and(|label| label == *asset_exact)
                        }
                        AssetFind::AssetEndsWith { asset_ends_with } => {
                            asset.name.ends_with(asset_ends_with)
                        }
                    }
                } else {
                    false
                }
            })
            .unwrap();

        Self::download(
            client,
            &asset.browser_download_url.to_owned(),
            &download_directory.join(&asset.name),
            progress_bar,
        )
        .await
    }
}

pub struct GitCloneArgs {
    git_clone: GitClone,
    directory_path: PathBuf,
    token: Option<secrecy::SecretBox<str>>,
}

impl GitCloneArgs {
    pub fn from_gitclone(
        git_clone: GitClone,
        directory_path: PathBuf,
        token: Option<SecretString>,
    ) -> GitCloneArgs {
        GitCloneArgs {
            git_clone,
            directory_path,
            token,
        }
    }
    pub async fn clone_and_execute(&self, progress_bar: ProgressBar) -> Result<()> {
        self.git_clone(progress_bar).await?;
        shell_command::execute_all(&self.git_clone.shell_commands).await;
        Ok(())
    }
    async fn git_clone(&self, progress_bar: ProgressBar) -> Result<()> {
        let token = match &self.token {
            Some(token) => token,
            None => {
                github::initialise_octocrab(&self.git_clone.owner)?;
                &github::get_github_token()
            }
        };

        let repo = octocrab::instance()
            .repos(&self.git_clone.owner, &self.git_clone.repo)
            .get()
            .await
            .expect("Invalid repo");

        println!("clone dir: {:#?}", &self.directory_path);
        fs::create_dir_all(&self.directory_path)
            .with_context(|| format!("Could not create directory: {:#?}", &self.directory_path))?;

        let directory_path = self.directory_path.join(self.git_clone.repo.clone());

        let local_repo = git2::Repository::open(&directory_path);
        match local_repo {
            Ok(local_repo) => local_repo
                .find_remote("origin")
                .expect("Imagine not using origin as your remote name")
                .fetch(&["main"], None, None)
                .with_context(|| {
                    format!(
                        "Could not fetch origin main for local repository: {}",
                        repo.name
                    )
                }),
            Err(_) => {
                let url = &repo.html_url.ok_or(anyhow!(
                    "{} does not have an html url",
                    repo.full_name.unwrap_or_else(|| repo.name.clone())
                ))?;

                let fetch_options = github::create_repository_fetch_options(
                    &token,
                    &self.git_clone.owner,
                    progress_bar,
                );

                match RepoBuilder::new()
                    .fetch_options(fetch_options)
                    .clone(url.as_str(), &directory_path)
                    .with_context(|| {
                        format!("Failed to clone repo:\n{url}\n into {:?}", directory_path)
                    }) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let _ = fs::remove_dir_all(&directory_path);
                        Err(e)
                    }
                }
            }
        }
    }
}

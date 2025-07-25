use std::{borrow::Cow, collections::HashMap, env::home_dir, fs, path::PathBuf};

use crate::{
    dotfiles::DotfilesDetails,
    download::Downloader,
    github,
    impls::{Config, Executor, ExecutorSync, GitCloneArgs, ItemProgress},
    progress_bar::{
        create_application_download_coordinator_progress_bar, create_download_asset_progress_bar,
        create_progress_bar, create_repositories_clone_coordinator_progress_bar,
    },
    Command, RemoteConfigArguments,
};
use anyhow::{anyhow, Context, Result};
use common::configuration::{
    ApplicationDetails, Configuration, ConfigurationItem, DetailsType, Download, GitClone,
    GitCloneConfig, RepositoryDetails, ShellCommand,
};
use dirs::download_dir;
use futures::future::{join, join_all};
use indicatif::{MultiProgress, ProgressFinish, ProgressStyle};
use log::error;
use reqwest::Client;
use serde::Serialize;
use tokio::task::JoinSet;

pub struct ConfigurationLoader {
    args: Command,
    download_directory: PathBuf,
    home_directory: PathBuf,
}

impl ConfigurationLoader {
    pub fn new(args: Command) -> Self {
        let download_directory = dirs::download_dir().expect("Failed to find download directory");
        let home_directory: PathBuf = dirs::home_dir().expect("Failed to find home directory");
        Self {
            args,
            download_directory,
            home_directory,
        }
    }
    pub async fn load_all_configurations(self) -> Result<ExecutionPlan> {
        let mut configs = vec![];

        match &self.args {
            Command::Local(args) => configs.append(
                &mut self
                    .load_local_configurations(&args.directory_path)
                    .await
                    .with_context(|| format!("Could not load local configuration. Path:{:#?}", &args.directory_path))?,
            ),
            Command::Remote(remote) => configs.append(
                &mut self
                    .load_external_configurations(&remote)
                    .await
                    .with_context(|| {
                        format!("could not load remote configuration: {:#?}", remote)
                    })?,
            ),
            Command::Remotes(args) => {
                let loaded_external_configurations = args
                    .remotes
                    .iter()
                    .map(|remote| self.load_external_configurations(remote));
                let remotes = join_all(loaded_external_configurations)
                    .await
                    .into_iter()
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten();

                configs.extend(remotes);
            }
            Command::All(args) => {
                let (local_configs, external_configs) = join(
                    self.load_local_configurations(&args.local.directory_path),
                    self.load_external_configurations(&args.remote),
                )
                .await;
                configs.append(&mut local_configs?);
                configs.append(&mut external_configs?);
            }
        };
        Ok(ExecutionPlan {
            download_directory: self.download_directory,
            home_directory: self.home_directory,
            items: configs.into_iter().collect(),
        })
    }

    async fn load_local_configurations(
        &self,
        configuration_directory: &PathBuf,
    ) -> Result<Vec<ExecutionPlanEntry>> {
        let configuration_files = fs::read_dir(configuration_directory)?.filter_map(|file| {
            file.ok().and_then(
                |file| match file.metadata().expect("Not a symlink").is_file() {
                    true => Some(file),
                    false => None,
                },
            )
        });

        let config_files = configuration_files.map(|file| {
            let file_path = &file.path();
            let configuration_file = fs::read_to_string(file_path);
            let file = match configuration_file {
                Ok(configuration_file_content) => {
                    match serde_json::from_str::<Configuration>(&configuration_file_content) {
                        Ok(config) => Ok(config),
                        Err(err) => {
                            error!(
                                "File at path {:#?} did not contain a valid configuration\n{err}",
                                file_path
                            );
                            Err(err.into())
                        }
                    }
                }
                Err(err) => Err(anyhow!("Could not read configuration file: {err}")),
            };
            file
        });

        config_files
            .map(|configuration| match configuration {
                Ok(configuration) => Ok((
                    configuration.clone_config,
                    configuration
                        .items
                        .into_iter()
                        .map(|configuration_item| configuration_item.into())
                        .collect(),
                )),
                Err(err) => Err(err),
            })
            .collect()
    }

    async fn load_external_configurations(
        &self,
        remote: &RemoteConfigArguments,
    ) -> Result<Vec<ExecutionPlanEntry>> {
        let owner = &remote.owner;
        let repo = &remote.repo;
        let file_paths = &remote.config_file_paths;

        github::initialise_octocrab(owner)?;
        let mut external_configs: Vec<Result<ExecutionPlanEntry>> = vec![];

        for path in file_paths {
            let configurations = github::get_configs_from_github(owner, repo, path).await?;
            let mut configs: Vec<Result<ExecutionPlanEntry>> = configurations
                .into_iter()
                .map(|config| match config {
                    Ok(config) => Ok((
                        config.clone_config,
                        config
                            .items
                            .into_iter()
                            .map(|configuration_item| configuration_item.into())
                            .collect(),
                    )),
                    Err(err) => Err(err),
                })
                .collect();
            external_configs.append(&mut configs)
        }

        if external_configs.len() == 0 {
            println!("No external configs provided");
            return Err(anyhow!(
                "Zero external configs were loaded from the provided details"
            ));
        }

        external_configs.into_iter().collect()
    }
}

type ExecutionPlanEntry = (GitCloneConfig, Vec<ExecutionPlanItem>);

#[derive(Debug)]
pub struct ExecutionPlan {
    pub download_directory: PathBuf,
    pub home_directory: PathBuf,
    pub items: HashMap<GitCloneConfig, Vec<ExecutionPlanItem>>,
}

impl ExecutionPlan {
    pub(crate) async fn execute_no_install(self, client: Client) -> Vec<Result<()>> {
        vec![]
    }
    pub(crate) async fn execute(self, client: Client) -> Vec<Result<()>> {
        let multi_progress = MultiProgress::new();
        let mut application_download_tasks = JoinSet::new();
        let mut repository_clone_tasks = JoinSet::new();
        let mut dotfiles_tasks = JoinSet::new();

        let token = Some(github::get_github_token());

        for (config, execution_items) in self.items.into_iter() {
            let mut applications_to_download: Vec<ExecutionPlanItem> = vec![];
            let mut dotfiles_to_setup: Vec<ExecutionPlanItem> = vec![];
            let mut repositories_to_clone: Vec<ExecutionPlanItem> = vec![];
            let mut shell_commands_to_execute: Vec<ExecutionPlanItem> = vec![];

            for execution_item in execution_items.clone() {
                match execution_item {
                    ExecutionPlanItem::Download(ref download) => match download {
                        DownloadType::Application(_) => {
                            applications_to_download.push(execution_item)
                        }
                        DownloadType::GitHubAsset(_) => {
                            applications_to_download.push(execution_item)
                        }
                    },
                    ExecutionPlanItem::Dotfile(_) => dotfiles_to_setup.push(execution_item),
                    ExecutionPlanItem::GitClone(_) => repositories_to_clone.push(execution_item),
                    ExecutionPlanItem::ShellCommand(_) => {
                        shell_commands_to_execute.push(execution_item)
                    }
                }
            }

                let applications_to_download_count = applications_to_download.len();
                let application_download_coordinator_progress_bar =
                    create_application_download_coordinator_progress_bar(
                        &multi_progress,
                        applications_to_download_count,
                    );

                execution_items.into_iter().for_each(|item| {
                    let client = client.clone();
                    let download_directory = self.download_directory.clone();
                    match item {
                        ExecutionPlanItem::Download(download) => {
                            let progress_bar =
                                download.create_progress_bar(self.download_directory.clone());
                            match download {
                                DownloadType::Application(application_details) => {
                                    application_download_tasks.spawn(async move {
                                        application_details
                                            .to_owned()
                                            .download_self(client, download_directory, progress_bar)
                                            .await
                                    })
                                }
                                DownloadType::GitHubAsset(repository_details) => {
                                    application_download_tasks.spawn(async move {
                                        repository_details
                                            .download_self(client, download_directory, progress_bar)
                                            .await
                                    })
                                }
                            };
                            ()
                        }
                        ExecutionPlanItem::GitClone(ref git_clone) => {
                            let repositories_clone_coordinator_progress_bar =
                                create_repositories_clone_coordinator_progress_bar(
                                    repositories_to_clone.len(),
                                );

                            let repositories_clone_coordinator_progress_bar =
                                multi_progress.add(repositories_clone_coordinator_progress_bar);
                            repositories_clone_coordinator_progress_bar.set_position(0);
                            print!("\n");

                            let directory_path = config
                                .repositories_directory_path
                                .join(git_clone.repo.clone());
                            let progress_bar = git_clone.create_progress_bar(directory_path);
                            let progress_bar = multi_progress.add(progress_bar);
                            let git_clone_args = GitCloneArgs::from_gitclone(
                                git_clone.to_owned(),
                                config.repositories_directory_path.clone(),
                                token.clone(),
                            );
                            repository_clone_tasks.spawn(async move {
                                git_clone_args.clone_and_execute(progress_bar).await
                            });
                        }
                        ExecutionPlanItem::Dotfile(details_type) => {
                            let dotfiles_repository_path = config
                                .repositories_directory_path
                                .join(&config.dotfiles_repository.repo);
                            let dotfiles_details = DotfilesDetails::from_details(
                                details_type,
                                dotfiles_repository_path,
                                self.home_directory.clone(),
                            );
                            dotfiles_tasks.spawn(async move {
                                dotfiles_details.execute().await;
                            });
                            ()
                        }
                        ExecutionPlanItem::ShellCommand(shell_command) => {
                            shell_command.execute_sync();
                        }
                    }
                })
            });
        // while let Some(res) = tasks.join_next().await {
        //     match res {
        //         Ok(_) => results.push(Ok(())),
        //         Err(err) => results.push(Err(err.into())),
        //     }
        //     coordinator_progress_bar.set_position(results.len().try_into().unwrap());
        // }
        //

        // let mut results = Vec::new();
        // while let Some(res) = repository_clone_tasks.join_next().await {
        //     match res {
        //         Ok(a) => results.push(a),
        //         Err(err) => results.push(Err(err.into())),
        //     }
        //    application_download_coordinator_progress_bar
        //         .set_position(results.len().try_into().unwrap());
        // }
        // results

        vec![]
        // tasks.join_all().await
    }
}

#[derive(Debug, Clone)]
pub enum ExecutionPlanItem {
    Download(DownloadType),
    Dotfile(DetailsType),
    GitClone(GitClone),
    ShellCommand(ShellCommand),
}

impl From<ConfigurationItem> for ExecutionPlanItem {
    fn from(value: ConfigurationItem) -> Self {
        match value {
            ConfigurationItem::Download(download) => ExecutionPlanItem::Download(download.into()),
            ConfigurationItem::Dotfile(details_type) => ExecutionPlanItem::Dotfile(details_type),
            ConfigurationItem::GitClone(git_clone) => ExecutionPlanItem::GitClone(git_clone),
            ConfigurationItem::ShellCommand(shell_command) => {
                ExecutionPlanItem::ShellCommand(shell_command)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum DownloadType {
    Application(ApplicationDetails),
    GitHubAsset(RepositoryDetails),
}

impl From<Download> for DownloadType {
    fn from(value: Download) -> Self {
        match value {
            Download::Application(application_details) => {
                DownloadType::Application(application_details)
            }
            Download::GitHubAsset(repository_details) => {
                DownloadType::GitHubAsset(repository_details)
            }
        }
    }
}

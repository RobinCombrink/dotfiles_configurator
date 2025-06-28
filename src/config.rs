use std::{collections::HashMap, fs, path::PathBuf};

use crate::{github, impls::Config, Command, RemoteConfigArguments};
use anyhow::{anyhow, Context, Result};
use common::configuration::{
    ApplicationDetails, Configuration, DetailsType, GitClone, GitCloneConfig, RepositoryDetails,
    ShellCommand,
};
use futures::future::{join, join_all};
use log::error;
use serde::Serialize;

pub struct ConfigurationLoader {
    args: Command,
    download_directory: PathBuf,
    home_directory: PathBuf,
}

impl ConfigurationLoader {
    pub fn new(args: Command) -> Self {
        let download_directory = dirs::download_dir().expect("Failed to find download directory");
        let home_directory = dirs::home_dir().expect("Failed to find home directory");
        Self {
            args,
            download_directory,
            home_directory,
        }
    }
    pub async fn load_all_configurations(self) -> Result<Vec<Config>> {
        let mut configs = vec![];

        match &self.args {
            Command::Local(args) => configs.append(
                &mut self
                    .load_local_configurations(&args.directory_path)
                    .await
                    .with_context(|| format!("Could not load local configuration"))?,
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
        Ok(configs)
    }

    async fn load_local_configurations(
        &self,
        configuration_directory: &PathBuf,
    ) -> Result<Vec<Config>> {
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

        let configurations = config_files
            .filter_map(|file| file.ok())
            .map(|configuration| {
                Config::from_configuration(
                    configuration,
                    self.download_directory.clone(),
                    self.home_directory.clone(),
                )
            })
            .collect();
        Ok(configurations)
    }

    async fn load_external_configurations(
        &self,
        remote: &RemoteConfigArguments,
    ) -> Result<Vec<Config>> {
        let owner = &remote.owner;
        let repo = &remote.repo;
        let file_paths = &remote.config_file_paths;

        github::initialise_octocrab(owner)?;
        let mut external_configs: Vec<Result<Config>> = vec![];

        for path in file_paths {
            match github::get_configs_from_github(owner, repo, path).await {
                Ok(configurations) => {
                    let mut configs: Vec<Result<Config>> = configurations
                        .into_iter()
                        .map(|config| match config {
                            Ok(configuration) => Ok(Config::from_configuration(
                                configuration,
                                self.download_directory.clone(),
                                self.home_directory.clone(),
                            )),
                            Err(err) => Err(err).into(),
                        })
                        .collect();
                    external_configs.append(&mut configs)
                }
                Err(err) => return Err(err),
            }
        }

        if external_configs.len() == 0 {
            println!("No external configs provided");
            return Ok(vec![]);
        }

        let external_configs = external_configs
            .into_iter()
            .filter_map(|config| config.ok())
            .collect();
        Ok(external_configs)
    }
}

pub async fn apply_all(configs: Vec<Config>) -> Vec<Result<()>> {
    join_all(configs.into_iter().map(|config| config.execute())).await
}

#[derive(Debug, Serialize)]
pub struct ExecutionPlan {
    application_downloads: Vec<ApplicationDetails>,
    github_assets: Vec<RepositoryDetails>,
    repository_clones: HashMap<GitCloneConfig, Vec<GitClone>>,
    dotfiles: HashMap<GitClone, Vec<DetailsType>>,
    shell_commands: Vec<ShellCommand>,
}

pub fn plan_all(configs: Vec<Config>) -> ExecutionPlan {
    let mut application_downloads: Vec<ApplicationDetails> = vec![];
    let mut github_assets: Vec<RepositoryDetails> = vec![];
    let mut repository_clones: HashMap<GitCloneConfig, Vec<GitClone>> = HashMap::new();
    let mut dotfiles: HashMap<GitClone, Vec<DetailsType>> = HashMap::new();
    let mut shell_commands: Vec<ShellCommand> = vec![];

    for config in configs {
        application_downloads.extend(config.configuration.downloads.applications);
        github_assets.extend(config.configuration.downloads.github_releases);

        if let Some(to_clones) = repository_clones.get(&config.configuration.clone_config) {
            let mut new_to_clones = to_clones.to_owned();
            new_to_clones.extend(config.configuration.to_clones);

            repository_clones.insert(config.configuration.clone_config, new_to_clones);
        } else {
            repository_clones.insert(
                config.configuration.clone_config,
                config.configuration.to_clones,
            );
        }

        if let Some(config_dotfiles) = dotfiles.get(&config.configuration.dotfiles_repository) {
            let mut updated_dotfiles = config_dotfiles.to_owned();
            if let Some(new_dotfiles) = config.configuration.dotfiles {
                updated_dotfiles.extend(new_dotfiles);
            }

            dotfiles.insert(config.configuration.dotfiles_repository, updated_dotfiles);
        } else {
            if let Some(config_dotfiles) = config.configuration.dotfiles {
                dotfiles.insert(config.configuration.dotfiles_repository, config_dotfiles);
            }
        }

        if let Some(new_shell_commands) = config.configuration.shell_commands {
            shell_commands.extend(new_shell_commands);
        }
    }

    ExecutionPlan {
        application_downloads,
        github_assets,
        repository_clones,
        dotfiles,
        shell_commands,
    }
}

use std::{fs, path::PathBuf};

use crate::{github, impls::Config, Dotfiles, RemoteConfigArguments};
use anyhow::{anyhow, Result};
use common::configuration::Configuration;
use futures::future::join_all;
use log::error;

pub struct ConfigurationLoader {
    args: Dotfiles,
    download_directory: PathBuf,
    home_directory: PathBuf,
}

impl ConfigurationLoader {
    pub fn new(args: Dotfiles) -> Self {
        let download_directory = dirs::download_dir().expect("Failed to find download directory");
        let home_directory = dirs::home_dir().expect("Failed to find home directory");
        Self {
            args,
            download_directory,
            home_directory,
        }
    }
    pub async fn load_all_configurations(self) -> Vec<Result<Vec<Config>>> {
        let mut configs = vec![Config::from_configuration(
            Configuration::new(),
            self.download_directory.clone(),
            self.home_directory.clone(),
        )];
        let result = match &self.args {
            Dotfiles::Local(args) => {
                match self
                    .load_local_configurations(
                        &args.directory_path,
                        self.download_directory.clone(),
                        self.home_directory.clone(),
                    )
                    .await
                {
                    Ok(configs) => vec![Ok(configs)],
                    Err(e) => vec![Err(
                        anyhow!("Could not load local configuration files").context(e)
                    )],
                }
            }
            Dotfiles::Remote(remote) => {
                match self
                    .load_external_configurations(
                        &remote,
                        self.download_directory.clone(),
                        self.home_directory.clone(),
                    )
                    .await
                {
                    Ok(configs) => vec![Ok(configs)],
                    Err(e) => vec![Err(
                        anyhow!("Could not load remote configuration files").context(e)
                    )],
                }
            }
            Dotfiles::Remotes(args) => {
                let loaded_external_configurations = args.remotes.iter().map(|remote| {
                    self.load_external_configurations(
                        remote,
                        self.download_directory.clone(),
                        self.home_directory.clone(),
                    )
                });
                join_all(loaded_external_configurations).await
            }
            Dotfiles::All(args) => {
                let local = match (&self)
                    .load_local_configurations(
                        &args.local.directory_path,
                        self.download_directory.clone(),
                        self.home_directory.clone(),
                    )
                    .await
                {
                    Ok(mut local_configs) => Ok(configs.append(&mut local_configs)),
                    Err(e) => Err(anyhow!("Could not load local configuration files").context(e)),
                };

                if let Err(err) = local {
                    error!("Could not fetch local configuration: {err}")
                }

                let external = match self
                    .load_external_configurations(
                        &args.remote,
                        self.download_directory.clone(),
                        self.home_directory.clone(),
                    )
                    .await
                {
                    Ok(mut remote_configs) => Ok(configs.append(&mut remote_configs)),
                    Err(e) => Err(anyhow!("Could not load remote configuration files").context(e)),
                };
                if let Err(err) = external {
                    error!("Could not fetch local configuration: {err}")
                }
                vec![Ok(configs)]
            }
        };
        result
    }

    async fn load_local_configurations(
        &self,
        configuration_directory: &PathBuf,
        download_directory: PathBuf,
        home_directory: PathBuf,
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
                    download_directory.clone(),
                    home_directory.clone(),
                )
            })
            .collect();
        Ok(configurations)
    }

    async fn load_external_configurations(
        &self,
        remote: &RemoteConfigArguments,
        download_directory: PathBuf,
        home_dir: PathBuf,
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
                                download_directory.clone(),
                                home_dir.clone(),
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

pub async fn apply_all(configurations: Vec<Result<Vec<Config>>>) -> Vec<Result<Vec<Result<()>>>> {
    join_all(
        configurations
            .into_iter()
            .map(|configuration_file| maybe_execute(configuration_file)),
    )
    .await
}

async fn maybe_execute(configuration_file: Result<Vec<Config>>) -> Result<Vec<Result<()>>> {
    match configuration_file {
        Ok(configs) => Ok(join_all(configs.into_iter().map(|config| config.execute())).await),
        Err(err) => Err(err),
    }
}

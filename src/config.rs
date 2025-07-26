use std::{fs, path::PathBuf};

use {
    crate::{
        execution_plan::{ExecutionPlan, ExecutionPlanEntry, ExecutionPlanEntryConverter},
        github::{self},
        Command, RemoteConfigArguments,
    },
    anyhow::{anyhow, Context, Result},
    common::configuration::Configuration,
    futures::future::{join, join_all},
    log::error,
};

pub struct ConfigurationLoader {
    args: Command,
    download_directory: PathBuf,
    home_directory: PathBuf,
}

impl ConfigurationLoader {
    pub fn new(args: Command) -> Self {
        // let download_directory = dirs::download_dir().expect("Failed to find download directory");
        // let home_directory: PathBuf = dirs::home_dir().expect("Failed to find home directory");
        let download_directory = PathBuf::from("C:\\Test\\Download");
        let home_directory: PathBuf = PathBuf::from("C:\\Test\\Home");
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
                    .with_context(|| {
                        format!(
                            "Could not load local configuration. Path:{:#?}",
                            &args.directory_path
                        )
                    })?,
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
                Ok(configuration) => configuration.try_into_entry(),
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
                    Ok(config) => config.try_into_entry(),
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

use std::{fs, path::PathBuf};

use crate::{common::Configuration, github, Dotfiles};
use anyhow::{anyhow, Result};
use futures::future::join_all;
use log::error;

pub async fn load_all_configurations(args: Dotfiles) -> Vec<Result<Vec<Configuration>>> {
    let mut configurations = vec![Configuration::new()];
    let result = match args {
        Dotfiles::Local(args) => match load_local_configurations(args.directory_path).await {
            Ok(configs) => vec![Ok(configs)],
            Err(e) => vec![Err(
                anyhow!("Could not load local configuration files").context(e)
            )],
        },
        Dotfiles::Remote(args) => {
            match load_external_configurations(&args.owner, &args.repo, args.config_file_paths)
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
                load_external_configurations(
                    &remote.owner,
                    &remote.repo,
                    remote.config_file_paths.clone(),
                )
            });
            join_all(loaded_external_configurations).await
        }
        Dotfiles::All(args) => {
            let local = match load_local_configurations(args.local.directory_path).await {
                Ok(mut configs) => Ok(configurations.append(&mut configs)),
                Err(e) => Err(anyhow!("Could not load local configuration files").context(e)),
            };

            if let Err(err) = local {
                error!("Could not fetch local configuration: {err}")
            }

            let external = match load_external_configurations(
                &args.remote.owner,
                &args.remote.repo,
                args.remote.config_file_paths,
            )
            .await
            {
                Ok(mut configs) => Ok(configurations.append(&mut configs)),
                Err(e) => Err(anyhow!("Could not load remote configuration files").context(e)),
            };
            if let Err(err) = external {
                error!("Could not fetch local configuration: {err}")
            }
            vec![Ok(configurations)]
        }
    };
    result
}

async fn load_local_configurations(
    conifguration_path: impl Into<PathBuf>,
) -> Result<Vec<Configuration>> {
    let configuration_directory = conifguration_path.into();
    let configuration_files = fs::read_dir(&configuration_directory)?.filter_map(|file| {
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
        let file= match configuration_file {
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

    let configurations = config_files.filter_map(|file| file.ok()).collect();
    Ok(configurations)
}

async fn load_external_configurations(
    owner: &str,
    repo: &str,
    file_paths: Vec<impl Into<String>>,
) -> Result<Vec<Configuration>> {
    github::initialise_octocrab(owner)?;
    let mut external_configs: Vec<Result<Configuration>> = vec![];

    for path in file_paths {
        match github::get_configs_from_github(owner, repo, path).await {
            Ok(mut config) => external_configs.append(&mut config),
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

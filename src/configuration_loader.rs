use {
    crate::{
        Arguments, Command, RemoteConfigArguments,
        execution_plan::{
            ExecutionPlan, ExecutionPlanEntry, ExecutionPlanEntryConverter, ExecutionPlanItems,
            Merge,
        },
        github,
    },
    anyhow::{Context, Result, anyhow},
    crate::configuration::{Configuration, GitCloneConfig},
    futures::future::{join, join_all},
    github_authentication::authentication::{Authentication, GitHubCliAuthentication},
    log::error,
    std::{collections::HashMap, env::home_dir, fs, path::PathBuf},
};

pub struct ConfigurationLoader {
    config_source: Command,
    download_directory: PathBuf,
    home_directory: PathBuf,
}

impl ConfigurationLoader {
    pub fn new(args: Arguments) -> Self {
        let (download_directory, home_directory) = if args.debug {
            (
                PathBuf::from("C:\\Test\\Download"),
                PathBuf::from("C:\\Test\\Home"),
            )
        } else {
            (
                dirs::download_dir().expect("Failed to find download directory"),
                home_dir().expect("Failed to find home directory"),
            )
        };
        Self {
            config_source: args.command,
            download_directory,
            home_directory,
        }
    }
    pub async fn load_all_configurations(self) -> Result<ExecutionPlan> {
        let configs = match &self.config_source {
            Command::Local(args) => self
                .load_local_configurations(&args.directory_path)
                .await
                .with_context(|| {
                    format!(
                        "Could not load local configuration. Path:{:#?}",
                        args.directory_path
                    )
                })?,
            Command::Remote(remote) => self
                .load_external_configurations(remote)
                .await
                .with_context(|| format!("could not load remote configuration: {:#?}", remote))?,
            Command::Remotes(args) => {
                let loaded_external_configurations = args
                    .remotes
                    .iter()
                    .map(|remote| self.load_external_configurations(remote));
                join_all(loaded_external_configurations)
                    .await
                    .into_iter()
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect()
            }
            Command::All(args) => {
                let (local_configs, external_configs) = join(
                    self.load_local_configurations(&args.local.directory_path),
                    self.load_external_configurations(&args.remote),
                )
                .await;
                local_configs?
                    .into_iter()
                    .chain(external_configs?)
                    .collect()
            }
        };

        let items = merge_execution_plan_items(configs);

        Ok(ExecutionPlan {
            download_directory: self.download_directory,
            home_directory: self.home_directory,
            items,
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
            match configuration_file {
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
            }
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

        let authentication = GitHubCliAuthentication::new(remote.owner.clone())?;

        let octocrab = github::create_octocrab(authentication.get_token())?;
        let mut external_configs: Vec<Result<ExecutionPlanEntry>> = vec![];

        for path in file_paths {
            let configurations =
                github::get_configs_from_github(owner, repo, path, &octocrab).await?;
            let mut configs: Vec<Result<ExecutionPlanEntry>> = configurations
                .into_iter()
                .map(|config| match config {
                    Ok(config) => config.try_into_entry(),
                    Err(err) => Err(err),
                })
                .collect();
            external_configs.append(&mut configs)
        }

        if external_configs.is_empty() {
            println!("No external configs provided");
            return Err(anyhow!(
                "Zero external configs were loaded from the provided details"
            ));
        }

        external_configs.into_iter().collect()
    }
}

fn merge_execution_plan_items<T: Authentication>(
    configs: Vec<(GitCloneConfig, ExecutionPlanItems<T>)>,
) -> HashMap<GitCloneConfig, ExecutionPlanItems<T>> {
    let mut items: HashMap<_, ExecutionPlanItems<T>> = HashMap::new();
    for (key, value) in configs.into_iter() {
        let execution_plan_item = if let Some(mut execution_plan_item) = items.remove(&key) {
            execution_plan_item.merge(value);
            execution_plan_item
        } else {
            value
        };
        items.insert(key, execution_plan_item);
    }
    items
}

#[cfg(test)]
mod tests {
    use crate::{ExecutionType, LocalConfigArguments, execution_plan::ExecutionPlanItems};
    use crate::configuration::{
        ApplicationDetails, ConfigurationItem, Download, GitClone, GitCloneConfig,
    };
    use github_authentication::authentication::Authentication;
    use url::Url;

    use super::*;
    use std::{env, fs::File, io::Write, str::FromStr};

    fn get_test_data_directory() -> PathBuf {
        let directory: PathBuf = PathBuf::from("tests").join("config/");
        directory
    }

    type TestExecutionPlanEntry = (GitCloneConfig, ExecutionPlanItems<FakeAuthentication>);

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FakeAuthentication;

    impl Authentication for FakeAuthentication {
        fn get_token(&self) -> secrecy::SecretString {
            "123".into()
        }

        fn get_username(&self) -> String {
            "test_username".into()
        }
    }

    fn get_temp_directory(directory_name: impl Into<PathBuf>) -> PathBuf {
        let temp_dir = env::temp_dir();
        let temp_dir = temp_dir
            .join(get_test_data_directory())
            .join(directory_name.into());
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    fn get_test_config_directory<'a>(directory_name: impl Into<&'a str>) -> PathBuf {
        let directory: PathBuf = get_test_data_directory();
        directory.join(directory_name.into())
    }

    fn make_execution_plan_entry(
        gitclone_config: impl Into<GitCloneConfig>,
    ) -> TestExecutionPlanEntry {
        let authentication = FakeAuthentication {};
        let item = make_execution_plan_items(authentication);
        (gitclone_config.into(), item)
    }

    fn make_execution_plan_items<T: Authentication>(authentication: T) -> ExecutionPlanItems<T> {
        ExecutionPlanItems {
            items: vec![
                ConfigurationItem::Download(Download::Application(ApplicationDetails {
                    name: "test_application".into(),
                    uri: Url::from_str("https://test.exe").unwrap(),
                    dotfiles: None,
                }))
                .into(),
            ],
            authentication,
        }
    }

    fn make_git_clone_config(
        github_username: impl Into<String>,
        owner: impl Into<String>,
    ) -> GitCloneConfig {
        GitCloneConfig {
            github_username: github_username.into(),
            repositories_directory_path: "../tests/configuration_loader/repositories".into(),
            dotfiles_repository: GitClone {
                owner: owner.into(),
                repo: "dotfiles_repository".to_owned(),
                shell_commands: None,
            },
        }
    }

    fn make_local_config_args(path: impl Into<PathBuf>) -> LocalConfigArguments {
        LocalConfigArguments {
            directory_path: path.into(),
        }
    }

    #[test]
    fn given_a_single_execution_plan_entry_when_merged_is_equal() {
        let gitclone_config = make_git_clone_config("single_username", "single_owner");
        let execution_plan_entry = make_execution_plan_entry(gitclone_config.clone());
        let input = vec![execution_plan_entry.clone()];

        let result = merge_execution_plan_items(input);

        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&gitclone_config));
        assert_eq!(result[&gitclone_config].items.len(), 1);
        assert_eq!(result[&gitclone_config], execution_plan_entry.1);
    }
    #[test]
    fn given_multiple_unique_execution_plan_entries_when_merged_then_all_entries_present() {
        let gitclone_config_a = make_git_clone_config("user_a", "owner_a");
        let gitclone_config_b = make_git_clone_config("user_b", "owner_b");
        let entry_a = make_execution_plan_entry(gitclone_config_a.clone());
        let entry_b = make_execution_plan_entry(gitclone_config_b.clone());
        let input = vec![entry_a.clone(), entry_b.clone()];

        let result = merge_execution_plan_items(input);

        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&gitclone_config_a));
        assert!(result.contains_key(&gitclone_config_b));
        assert_eq!(result[&gitclone_config_a], entry_a.1);
        assert_eq!(result[&gitclone_config_b], entry_b.1);
    }
    #[test]
    fn given_duplicate_keys_when_merged_then_items_are_merged_into_a_single_entry() {
        let gitclone_config = make_git_clone_config("dup_user", "dup_owner");
        let entry1 = make_execution_plan_entry(gitclone_config.clone());
        let entry2 = make_execution_plan_entry(gitclone_config.clone());
        let input = vec![entry1.clone(), entry2.clone()];

        let result = merge_execution_plan_items(input);

        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&gitclone_config));
        let items = &result[&gitclone_config].items;
        assert_eq!(items.len(), 2);

        assert!(items.contains(&entry1.1.items[0]));
        assert!(items.contains(&entry2.1.items[0]));
    }
    #[test]
    fn given_empty_input_when_merged_then_result_is_empty() {
        let input: Vec<(GitCloneConfig, ExecutionPlanItems<FakeAuthentication>)> = vec![];

        let result = merge_execution_plan_items(input);

        assert!(result.is_empty());
    }
    #[tokio::test]
    async fn given_invalid_local_config_path_when_loading_non_existent_file_then_returns_error() {
        let args = Arguments {
            command: Command::Local(make_local_config_args("/invalid/file_path.json")),
            execution_type: ExecutionType::DryRun,
            debug: false,
        };
        let loader = ConfigurationLoader::new(args);
        let result = loader.load_all_configurations().await;
        assert!(result.is_err());
    }
    #[tokio::test]
    async fn given_malformed_local_config_file_when_loading_then_returns_error() {
        let dir = get_temp_directory("dotfiles");
        let file_path = dir.join("bad.json");
        println!("{:#?}", file_path);
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "{{ this is not valid json }}").unwrap();
        let args = Arguments {
            command: Command::Local(make_local_config_args(file_path)),
            execution_type: crate::ExecutionType::DryRun,
            debug: false,
        };
        let loader = ConfigurationLoader::new(args);
        let result = loader.load_all_configurations().await;
        assert!(result.is_err());
    }
    #[tokio::test]
    async fn given_valid_local_config_file_when_loading_then_execution_plan_items_are_not_empty() {
        let file_path = get_test_config_directory("single");

        let args = Arguments {
            command: Command::Local(make_local_config_args(&file_path)),
            execution_type: crate::ExecutionType::DryRun,
            debug: false,
        };
        let loader = ConfigurationLoader::new(args);

        let result = loader.load_all_configurations().await;
        assert!(result.is_ok());
        let plan = result.unwrap();
        assert!(!plan.items.is_empty());
    }
}

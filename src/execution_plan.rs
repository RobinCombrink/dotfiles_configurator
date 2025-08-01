use {
    crate::{
        dotfiles::DotfilesDetails,
        download::Downloader,
        github::{self},
        impls::{Executor, GitCloneArgs, ItemProgress},
        progress_bar::create_execution_item_coordinator_progress_bar,
    },
    anyhow::{Context, Result},
    common::configuration::{
        ApplicationDetails, AssetFind, Configuration, ConfigurationItem, DetailsType, Download,
        GitClone, GitCloneConfig, RepositoryDetails, ShellCommand,
    },
    github_authentication::authentication::{Authentication, GitHubCliAuthentication},
    indicatif::MultiProgress,
    reqwest::Client,
    std::{collections::HashMap, path::PathBuf, process::exit},
    tokio::task::JoinSet,
};
pub(crate) trait ExecutionPlanEntryConverter {
    fn try_into_entry(self) -> Result<ExecutionPlanEntry>;
}

impl ExecutionPlanEntryConverter for Configuration {
    fn try_into_entry(self) -> Result<ExecutionPlanEntry> {
        let authentication =
            GitHubCliAuthentication::new(self.clone_config.github_username.clone())?;

        let items = self
            .items
            .into_iter()
            .map(|configuration_item| configuration_item.into())
            .collect();

        let item_pairs = ExecutionPlanItems {
            items,
            authentication,
        };
        Ok((self.clone_config, item_pairs))
    }
}

#[derive(Debug)]
pub struct ExecutionPlan {
    pub download_directory: PathBuf,
    pub home_directory: PathBuf,
    pub items: HashMap<GitCloneConfig, ExecutionPlanItems<GitHubCliAuthentication>>,
}

impl ExecutionPlan {
    pub(crate) async fn execute_new(self, client: Client) -> Vec<Result<()>> {
        self.ensure_github_cli_is_installed(client.clone()).await;

        let mut results: Vec<Result<()>> = Vec::new();

        let multi_progress = MultiProgress::new();

        let execution_items_count =
            self.items.values().flat_map(|item| &item.items).count() + self.items.keys().count();

        let execution_items_coordinator_progress_bar = multi_progress.add(
            create_execution_item_coordinator_progress_bar(&multi_progress, execution_items_count),
        );

        for (execution_config, execution_items_pair) in self.items.into_iter() {
            let octocrab =
                match github::create_octocrab(execution_items_pair.authentication.get_token()) {
                    Ok(octocrab) => octocrab,
                    Err(err) => {
                        results.push(Err(err).with_context(|| {
                            format!(
                                "Could not initialize octocrab instance for {}",
                                execution_config.dotfiles_repository
                            )
                        }));
                        continue;
                    }
                };
            let mut tasks = JoinSet::new();

            let dotfiles_repository = execution_config.dotfiles_repository.clone();
            let directory_path = execution_config
                .repositories_directory_path
                .join(&dotfiles_repository.repo);

            let git_clone_args = GitCloneArgs::from_gitclone(
                dotfiles_repository.to_owned(),
                execution_config.repositories_directory_path.clone(),
                execution_items_pair.authentication.clone(),
            );
            let progress_bar = multi_progress.insert_after(
                &execution_items_coordinator_progress_bar,
                dotfiles_repository.create_progress_bar(directory_path.clone()),
            );
            match git_clone_args
                .clone_and_execute(octocrab.clone(), progress_bar)
                .await
            {
                Ok(ok) => results.push(Ok(ok)),
                Err(err) => {
                    results.push(Err(err).with_context(||
                        format!(
                            "Could not clone dotfiles repository: {}/{} as user: {} to directory: {:#?}",
                            dotfiles_repository.owner,
                            dotfiles_repository.repo,
                            execution_items_pair.authentication.get_username(),
                            directory_path,
                        )
                    ));
                    continue;
                }
            };
            for plan_item in execution_items_pair.items.into_iter() {
                let client = client.clone();
                let download_directory = self.download_directory.clone();
                match plan_item {
                    ExecutionItem::Download(download) => {
                        let progress_bar = multi_progress
                            .add(download.create_progress_bar(self.download_directory.clone()));
                        match download {
                            DownloadType::Application(application_details) => {
                                tasks.spawn(async move {
                                    application_details
                                        .download_self(client, download_directory, progress_bar)
                                        .await
                                })
                            }
                            DownloadType::GitHubAsset(repository_details) => {
                                tasks.spawn(async move {
                                    repository_details
                                        .download_self(client, download_directory, progress_bar)
                                        .await
                                })
                            }
                        };
                    }
                    ExecutionItem::GitClone(ref git_clone) => {
                        let directory_path = execution_config
                            .repositories_directory_path
                            .join(&git_clone.repo);
                        let git_clone_args = GitCloneArgs::from_gitclone(
                            git_clone.to_owned(),
                            execution_config.repositories_directory_path.clone(),
                            execution_items_pair.authentication.clone(),
                        );
                        let progress_bar = multi_progress.insert_after(
                            &execution_items_coordinator_progress_bar,
                            git_clone.create_progress_bar(directory_path),
                        );
                        tasks.spawn({
                            let execution_config = execution_config.clone();
                            let octocrab = octocrab.clone();
                            async move {
                                git_clone_args
                                    .clone_and_execute(octocrab, progress_bar)
                                    .await
                                    .with_context(|| {
                                        format!("Execution Plan: {:#?}", execution_config)
                                    })
                            }
                        });
                    }
                    ExecutionItem::Dotfile(details_type) => {
                        let dotfiles_repository_path = execution_config
                            .repositories_directory_path
                            .join(&execution_config.dotfiles_repository.repo);
                        let dotfiles_details = DotfilesDetails::from_details(
                            details_type,
                            dotfiles_repository_path,
                            self.home_directory.clone(),
                        );
                        let progress_bar =
                            multi_progress.add(dotfiles_details.create_progress_bar("/".into()));
                        tasks.spawn(async move {
                            let result = dotfiles_details.execute().await;

                            progress_bar.finish_using_style();
                            result
                        });
                    }
                    ExecutionItem::ShellCommand(shell_command) => {
                        let progress_bar =
                            multi_progress.add(shell_command.create_progress_bar("/".into()));
                        results.push(shell_command.execute().await);
                        progress_bar.finish_using_style();
                        execution_items_coordinator_progress_bar
                            .set_position(results.len().try_into().unwrap());
                    }
                }
            }
            while let Some(res) = tasks.join_next().await {
                match res {
                    Ok(execution_result) => results.push(execution_result),
                    Err(err) => results.push(Err(err.into())),
                }
                execution_items_coordinator_progress_bar
                    .set_position(results.len().try_into().unwrap());
            }
        }

        results
    }

    async fn ensure_github_cli_is_installed(&self, client: Client) {
        let gh_exists = GitHubCliAuthentication::is_github_cli_on_path().is_ok();
        if !gh_exists {
            let github_asset_ends_with = "windows_amd64.msi".to_owned();

            let gh_repository_details = RepositoryDetails {
                owner: "cli".to_owned(),
                repo: "cli".to_owned(),
                asset_find: Some(AssetFind::AssetEndsWith {
                    asset_ends_with: github_asset_ends_with,
                }),
                shell_commands: None,
                dotfiles: None,
            };
            let download_type = DownloadType::GitHubAsset(gh_repository_details.clone());
            let progress_bar = download_type.create_progress_bar(self.download_directory.clone());
            let downloaded_gh_cli = gh_repository_details
                .download_self(client, self.download_directory.clone(), progress_bar)
                .await;

            match downloaded_gh_cli {
                Ok(_) => {
                    println!(
                        "Successfully downloaded the gh cli.\nRerun this application once you have installed it and logged in"
                    );
                    exit(0)
                }
                Err(e) => println!(
                    "Something went wrong installing the github CLI automatically (only supported on Windows):\n{e}\nInstall it manually from https://github.com/cli/cli and add it to your path"
                ),
            }
        }
    }
}

pub(crate) type ExecutionPlanEntry = (GitCloneConfig, ExecutionPlanItems<GitHubCliAuthentication>);

pub trait Merge {
    fn merge(&mut self, other: Self);
}

#[derive(Debug, Clone)]
pub(crate) struct ExecutionPlanItems<T: Authentication> {
    pub(crate) items: Vec<ExecutionItem>,
    pub(crate) authentication: T,
}

impl<T: Authentication> Merge for ExecutionPlanItems<T> {
    fn merge(&mut self, other: Self) {
        self.items.extend(other.items);
    }
}

#[derive(Debug, Clone)]
pub enum ExecutionItem {
    Download(DownloadType),
    Dotfile(DetailsType),
    GitClone(GitClone),
    ShellCommand(ShellCommand),
}

impl From<ConfigurationItem> for ExecutionItem {
    fn from(value: ConfigurationItem) -> Self {
        match value {
            ConfigurationItem::Download(download) => ExecutionItem::Download(download.into()),
            ConfigurationItem::Dotfile(details_type) => ExecutionItem::Dotfile(details_type),
            ConfigurationItem::GitClone(git_clone) => ExecutionItem::GitClone(git_clone),
            ConfigurationItem::ShellCommand(shell_command) => {
                ExecutionItem::ShellCommand(shell_command)
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

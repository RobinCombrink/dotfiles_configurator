use {
    crate::{
        dotfiles::DotfilesDetails,
        download::Downloader,
        github::{self, get_github_token_for_user},
        impls::{Executor, GitCloneArgs, ItemProgress},
        progress_bar::{create_application_download_coordinator_progress_bar, ExecutionProgress},
    },
    anyhow::{Context, Result},
    common::configuration::{
        ApplicationDetails, Configuration, ConfigurationItem, DetailsType, Download, GitClone,
        GitCloneConfig, RepositoryDetails, ShellCommand,
    },
    github_authentication::authentication::{Authentication, GitHubCliAuthentication},
    indicatif::MultiProgress,
    octocrab::Octocrab,
    reqwest::Client,
    secrecy::SecretString,
    std::{collections::HashMap, path::PathBuf, sync::Arc},
    tokio::task::JoinSet,
};
pub(crate) trait ExecutionPlanEntryConverter {
    fn try_into_entry(self) -> Result<ExecutionPlanEntry>;
}

impl ExecutionPlanEntryConverter for Configuration {
    fn try_into_entry(self) -> Result<ExecutionPlanEntry> {
        let authentication =
            GitHubCliAuthentication::new(self.clone_config.github_username.clone())?;
        let octocrab = github::initialise_octocrab(authentication.token.clone())?;

        let items = self
            .items
            .into_iter()
            .map(|configuration_item| ExecutionPlanItem {
                item: configuration_item.into(),
                authentication: authentication.clone(),
                octocrab: octocrab.clone(),
            })
            .collect();
        Ok((self.clone_config, items))
    }
}

#[derive(Debug)]
pub struct ExecutionPlan {
    pub download_directory: PathBuf,
    pub home_directory: PathBuf,
    pub items: HashMap<GitCloneConfig, Vec<ExecutionPlanItem<GitHubCliAuthentication>>>,
}

impl ExecutionPlan {
    pub(crate) async fn execute_new(self, client: Client) -> Vec<Result<()>> {
        let mut results: Vec<Result<()>> = Vec::new();

        let multi_progress = MultiProgress::new();
        let execution_items_coordinator_progress_bar =
            create_application_download_coordinator_progress_bar(&multi_progress, self.items.len());
        let coordinator = MultiProgress::new();

        for (execution_config, execution_items) in self.items.into_iter() {
            let execution_progress = ExecutionProgress::intialize(
                &coordinator,
                execution_config.clone(),
                execution_items.len(),
            );
            let mut tasks = JoinSet::new();

            for plan_item in execution_items.into_iter() {
                let client = client.clone();
                let download_directory = self.download_directory.clone();
                match plan_item.item {
                    ExecutionItem::Download(download) => {
                        let progress_bar =
                            download.create_progress_bar(self.download_directory.clone());
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
                            .join(git_clone.repo.clone());
                        let git_clone_args = GitCloneArgs::from_gitclone(
                            git_clone.to_owned(),
                            execution_config.repositories_directory_path.clone(),
                            plan_item.authentication,
                            plan_item.octocrab,
                        );
                        let progress_bar = git_clone.create_progress_bar(directory_path);
                        tasks.spawn(
                            async move { git_clone_args.clone_and_execute(progress_bar).await },
                        );
                    }
                    ExecutionItem::Dotfile(details_type) => {
                        // let dotfiles_repository_path = execution_config
                        //     .repositories_directory_path
                        //     .join(&execution_config.dotfiles_repository.repo);
                        // let dotfiles_details = DotfilesDetails::from_details(
                        //     details_type,
                        //     dotfiles_repository_path,
                        //     self.home_directory.clone(),
                        // );
                        // tasks.spawn(async move { dotfiles_details.execute().await });
                    }
                    ExecutionItem::ShellCommand(shell_command) => {
                        // shell_command.execute().await;
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
    // pub(crate) async fn execute(self, client: Client) -> Vec<Result<()>> {
    //     let multi_progress = MultiProgress::new();
    //     let mut application_download_tasks = JoinSet::new();
    //     let mut repository_clone_tasks = JoinSet::new();
    //     let mut dotfiles_tasks = JoinSet::new();

    //     let token = Some(github::get_github_token());

    //     for (config, execution_items) in self.items.into_iter() {
    //         let mut applications_to_download: Vec<ExecutionPlanItem> = vec![];
    //         let mut dotfiles_to_setup: Vec<ExecutionPlanItem> = vec![];
    //         let mut repositories_to_clone: Vec<ExecutionPlanItem> = vec![];
    //         let mut shell_commands_to_execute: Vec<ExecutionPlanItem> = vec![];

    //         for execution_item in execution_items.clone() {
    //             match execution_item {
    //                 ExecutionPlanItem::Download(ref download) => match download {
    //                     DownloadType::Application(_) => {
    //                         applications_to_download.push(execution_item)
    //                     }
    //                     DownloadType::GitHubAsset(_) => {
    //                         applications_to_download.push(execution_item)
    //                     }
    //                 },
    //                 ExecutionPlanItem::Dotfile(_) => dotfiles_to_setup.push(execution_item),
    //                 ExecutionPlanItem::GitClone(_) => repositories_to_clone.push(execution_item),
    //                 ExecutionPlanItem::ShellCommand(_) => {
    //                     shell_commands_to_execute.push(execution_item)
    //                 }
    //             }
    //         }

    //         let applications_to_download_count = applications_to_download.len();
    //         let application_download_coordinator_progress_bar =
    //             create_application_download_coordinator_progress_bar(
    //                 &multi_progress,
    //                 applications_to_download_count,
    //             );
    //         let repositories_clone_coordinator_progress_bar =
    //             create_repositories_clone_coordinator_progress_bar(repositories_to_clone.len());

    //         let repositories_clone_coordinator_progress_bar =
    //             multi_progress.add(repositories_clone_coordinator_progress_bar);
    //         repositories_clone_coordinator_progress_bar.set_position(0);

    //         for item in execution_items.into_iter() {
    //             let client = client.clone();
    //             let download_directory = self.download_directory.clone();
    //             match item {
    //                 ExecutionPlanItem::Download(download) => {
    //                     let progress_bar =
    //                         download.create_progress_bar(self.download_directory.clone());
    //                     match download {
    //                         DownloadType::Application(application_details) => {
    //                             application_download_tasks.spawn(async move {
    //                                 application_details
    //                                     .to_owned()
    //                                     .download_self(client, download_directory, progress_bar)
    //                                     .await
    //                             })
    //                         }
    //                         DownloadType::GitHubAsset(repository_details) => {
    //                             application_download_tasks.spawn(async move {
    //                                 repository_details
    //                                     .download_self(client, download_directory, progress_bar)
    //                                     .await
    //                             })
    //                         }
    //                     };
    //                 }
    //                 ExecutionPlanItem::GitClone(ref git_clone) => {
    //                     let directory_path = config
    //                         .repositories_directory_path
    //                         .join(git_clone.repo.clone());
    //                     let progress_bar = git_clone.create_progress_bar(directory_path);
    //                     let progress_bar = multi_progress.add(progress_bar);
    //                     let git_clone_args = GitCloneArgs::from_gitclone(
    //                         git_clone.to_owned(),
    //                         config.repositories_directory_path.clone(),
    //                         token.clone(),
    //                     );
    //                     repository_clone_tasks.spawn(async move {
    //                         git_clone_args.clone_and_execute(progress_bar).await
    //                     });
    //                 }
    //                 ExecutionPlanItem::Dotfile(details_type) => {
    //                     let dotfiles_repository_path = config
    //                         .repositories_directory_path
    //                         .join(&config.dotfiles_repository.repo);
    //                     let dotfiles_details = DotfilesDetails::from_details(
    //                         details_type,
    //                         dotfiles_repository_path,
    //                         self.home_directory.clone(),
    //                     );
    //                     dotfiles_tasks.spawn(async move {
    //                         dotfiles_details.execute().await;
    //                     });
    //                 }
    //                 ExecutionPlanItem::ShellCommand(shell_command) => {
    //                     shell_command.execute().await;
    //                 }
    //             }
    //         }
    //         let mut results: Vec<Result<()>> = Vec::new();
    //         while let Some(res) = repository_clone_tasks.join_next().await {
    //             match res {
    //                 Ok(_) => results.push(Ok(())),
    //                 Err(err) => results.push(Err(err.into())),
    //             }
    //             repositories_clone_coordinator_progress_bar
    //                 .set_position(results.len().try_into().unwrap());
    //         }
    //         let mut results = Vec::new();
    //         while let Some(res) = application_download_tasks.join_next().await {
    //             match res {
    //                 Ok(a) => results.push(a),
    //                 Err(err) => results.push(Err(err.into())),
    //             }
    //             application_download_coordinator_progress_bar
    //                 .set_position(results.len().try_into().unwrap());
    //         }
    //         // results
    //     }

    //     // let mut results = Vec::new();
    //     // while let Some(res) = repository_clone_tasks.join_next().await {
    //     //     match res {
    //     //         Ok(a) => results.push(a),
    //     //         Err(err) => results.push(Err(err.into())),
    //     //     }
    //     //    application_download_coordinator_progress_bar
    //     //         .set_position(results.len().try_into().unwrap());
    //     // }
    //     // results

    //     vec![]
    //     // tasks.join_all().await
    // }
}

pub(crate) type ExecutionPlanEntry = (
    GitCloneConfig,
    Vec<ExecutionPlanItem<GitHubCliAuthentication>>,
);

#[derive(Debug, Clone)]
pub struct ExecutionPlanItem<T: Authentication> {
    pub(crate) item: ExecutionItem,
    pub(crate) authentication: T,
    pub(crate) octocrab: Arc<Octocrab>,
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

use crate::dotfiles::DotfilesDetails;
use crate::{cli_commands, download::Downloader, github};
use anyhow::{anyhow, Context, Result};
use common::configuration::{
    ApplicationDetails, AssetFind, Configuration, GitClone, RepositoryDetails,
};
use futures::future::join_all;
use git2::build::RepoBuilder;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use log::{error, trace};
use reqwest::Client;
use secrecy::SecretString;
use std::borrow::Cow;
use std::future::Future;
use std::time::Duration;
use std::{fs, path::PathBuf};
use tokio::task::JoinSet;

pub trait Executor {
    fn execute(&self) -> impl Future<Output = Result<()>> + Send;
}

pub struct Config {
    configuration: Configuration,
    download_directory: PathBuf,
    home_dir: PathBuf,
}

impl Config {
    fn new(configuration: Configuration, download_directory: PathBuf, home_dir: PathBuf) -> Self {
        Self {
            configuration,
            download_directory,
            home_dir,
        }
    }
    pub fn from_configuration(
        configuration: Configuration,
        download_directory: PathBuf,
        home_dir: PathBuf,
    ) -> Self {
        Config {
            configuration,
            download_directory,
            home_dir,
        }
    }
    pub async fn execute(self) -> Result<()> {
        let repositories_path = self
            .configuration
            .clone_config
            .repositories_directory_path
            .clone();
        let dotfiles_repository_path =
            repositories_path.join(self.configuration.dotfiles_repository.repo.clone());
        let name = &self.configuration.dotfiles_repository.repo;
        let _ = GitCloneArgs::from_gitclone(
            self.configuration.dotfiles_repository.clone(),
            dotfiles_repository_path.clone(),
            None,
        )
        .git_clone(Self::create_download_asset_progress_bar(
            &self.configuration.dotfiles_repository.owner,
            name,
            repositories_path.clone(),
        ))
        .await;
        println!("\n");

        self.download_and_install_all(
            &self.download_directory,
        )
        .await;

        for result in self.clone_repos(repositories_path).await {
            if let Err(err) = result {
                error!("Could not clone or fetch repo: {:?}", err);
            }
        }
        let _ = cli_commands::execute_all(&self.configuration.cli_commands);
        Ok(())
    }
}

impl Config {
    async fn download_and_install_all(&self, download_directory: &PathBuf) {
        let client = Client::default();
        join_all(
            self.configuration
                .downloads
                .github_releases
                .iter()
                .map(|details| {
                    details.download_self(
                        client.clone(),
                        download_directory.to_path_buf(),
                        Self::create_download_asset_progress_bar(
                            &details.owner,
                            &details.repo,
                            download_directory.join(&details.repo),
                        ),
                    )
                }),
        )
        .await;

        self.download_applications(client, download_directory).await;

        let github_releases_dotfiles_details = self
            .configuration
            .downloads
            .github_releases
            .clone()
            .into_iter()
            .filter_map(|release| release.dotfiles)
            .flatten()
            .map(|details| {
                DotfilesDetails::from_details(
                    details,
                    self.configuration
                        .clone_config
                        .repositories_directory_path
                        .join(self.configuration.dotfiles_repository.repo.clone()),
                    self.home_dir.clone(),
                )
            });

        let github_releases_commands = self
            .configuration
            .downloads
            .github_releases
            .clone()
            .into_iter()
            .filter_map(|release| release.commands)
            .flatten();

        for details in github_releases_dotfiles_details {
            let _ = details.execute().await;
        }

        for command in github_releases_commands {
            let _ = command.execute();
        }
    }

    async fn download_applications(&self, client: Client, download_directory: &PathBuf) {
        let multi_progress = MultiProgress::new();

        let to_download = self.configuration.downloads.applications.len();
        let coordinator_progress_bar = multi_progress.add(Self::create_progress_bar(
            to_download,
            format!("Downloading applications"),
            ProgressFinish::WithMessage(Cow::from(format!("{} downloaded", to_download))),
            ProgressStyle::with_template(&format!(
                "[{{elapsed_precise}}] {{bar:{}.cyan/blue}} {{pos:>7}}/{{len:7}} {{msg}}",
                to_download
            ))
            .unwrap()
            .progress_chars("✓▢▢"),
        ));
        coordinator_progress_bar.set_position(0);
        let mut tasks = JoinSet::new();

        let applications = self
            .configuration
            .downloads
            .applications
            .clone()
            .into_iter();

        for details in applications {
            let progress_bar = multi_progress.add(Self::create_download_application_progress_bar());
            let download_directory = download_directory.clone();
            let client = client.clone();
            tasks.spawn(async move {
                let _ = details
                    .download_self(client, download_directory, progress_bar)
                    .await;
            });
        }
        let mut results: Vec<Result<()>> = Vec::new();
        while let Some(res) = tasks.join_next().await {
            match res {
                Ok(_) => results.push(Ok(())),
                Err(err) => results.push(Err(err.into())),
            }
            coordinator_progress_bar.set_position(results.len().try_into().unwrap());
        }
    }

    async fn clone_repos(&self, directory_path: PathBuf) -> Vec<Result<()>> {
        let token = Some(github::get_github_token());
        let git_clones = self
            .configuration
            .to_clones
            .clone()
            .into_iter()
            .map(|git_clone| {
                GitCloneArgs::from_gitclone(git_clone, directory_path.clone(), token.clone())
            });

        let multi_progress = MultiProgress::new();
        let mut tasks = JoinSet::new();

        let coordinator_progress_bar = Self::create_progress_bar(
            self.configuration.to_clones.len(),
            format!(
                "Cloning {} repositories",
                self.configuration.to_clones.len()
            ),
            ProgressFinish::WithMessage(Cow::from(format!(
                "{} repos downloaded",
                self.configuration.to_clones.len()
            ))),
            ProgressStyle::with_template(&format!(
                "[{{elapsed_precise}}] {{bar:{}.cyan/blue}} {{pos:>7}}/{{len:7}} {{msg}}",
                self.configuration.to_clones.len()
            ))
            .unwrap()
            .progress_chars("✓▢▢"),
        );

        let coordinator_progress_bar = multi_progress.add(coordinator_progress_bar);
        coordinator_progress_bar.set_position(0);
        print!("\n");

        for repo_details in git_clones {
            let progress_bar = multi_progress.add(Self::create_download_asset_progress_bar(
                &repo_details.git_clone.owner,
                &repo_details.git_clone.repo,
                directory_path.join(&repo_details.git_clone.repo),
            ));
            tasks.spawn(async move { repo_details.clone_and_execute(progress_bar).await });
        }
        let mut results = Vec::new();
        while let Some(res) = tasks.join_next().await {
            match res {
                Ok(a) => results.push(a),
                Err(err) => results.push(Err(err.into())),
            }
            coordinator_progress_bar.set_position(results.len().try_into().unwrap());
        }
        results
    }

    fn create_progress_bar(
        length: usize,
        message: impl Into<Cow<'static, str>>,
        finish: ProgressFinish,
        style: ProgressStyle,
    ) -> ProgressBar {
        ProgressBar::new(length.try_into().unwrap())
            .with_finish(finish)
            .with_message(message)
            .with_style(style)
            .with_elapsed(Duration::new(0, 0))
            .with_position(0)
    }

    fn create_download_application_progress_bar() -> ProgressBar {
        ProgressBar::new(0).with_style(ProgressStyle::default_bar()
                 .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}")
                 .unwrap()
                 .progress_chars("#>-"))
    }

    fn create_download_asset_progress_bar(
        owner: &String,
        repo: &String,
        repository_path: PathBuf,
    ) -> ProgressBar {
        let finish = indicatif::ProgressFinish::WithMessage(Cow::from(format!(
            "Cloned {owner}/{repo} into {:#?}",
            repository_path
        )));

        let message = format!("Cloning {owner}/{repo} into {:#?}", repository_path);

        let style = ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:100.cyan/blue} {pos:>7}/{len:7} {msg}",
        )
        .unwrap()
        .progress_chars("##-");

        Self::create_progress_bar(100, message, finish, style)
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
        cli_commands::execute_all(&self.git_clone.cli_commands).await;
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

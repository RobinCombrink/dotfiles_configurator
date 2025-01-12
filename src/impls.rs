use crate::{
    cli_commands,
    common::{
        ApplicationDetails, AssetFind, CliCommand, Configuration, DetailsType, Downloads,
        FileDetails, GitClone, GitCloneConfig, RepositoryDetails,
    },
    download::Downloader,
    github,
};
use anyhow::{anyhow, Context, Result};
use futures::future::join_all;
use git2::build::RepoBuilder;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use log::{error, trace};
use reqwest::Client;
use secrecy::SecretString;
use std::borrow::Cow;
use std::time::Duration;
use std::{fs, path::PathBuf};
use tokio::task::JoinSet;
use url::Url;

const CONFIG_DEFAULT_VERSION: &str = "0.1.0";

impl Configuration {
    pub fn new() -> Self {
        Self {
            version: CONFIG_DEFAULT_VERSION.to_owned(),
            clone_config: GitCloneConfig::default(),
            dotfiles_repository: GitClone::default(),
            downloads: Downloads::new(),
            to_clones: vec![GitClone::default()],
            dotfiles: Some(vec![DetailsType::File(FileDetails::default())]),
            cli_commands: Some(vec![CliCommand::default()]),
        }
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

    pub async fn apply(
        &self,
        download_directory: impl Into<&PathBuf>,
        home_dir: impl Into<&PathBuf>,
    ) -> Result<()> {
        let repositories_path = self.clone_config.repositories_directory_path.clone();
        let dotfiles_repository_path =
            repositories_path.join(self.dotfiles_repository.repo.clone());
        let name = &self.dotfiles_repository.repo;
        let _ = &self
            .dotfiles_repository
            .git_clone(
                repositories_path.clone(),
                None,
                Self::create_download_asset_progress_bar(
                    &self.dotfiles_repository.owner,
                    name,
                    repositories_path.clone(),
                ),
            )
            .await;
        println!("\n");

        self.download_and_install_all(
            download_directory.into(),
            home_dir.into(),
            &dotfiles_repository_path,
        )
        .await;

        for result in self.clone_repos(repositories_path).await {
            if let Err(err) = result {
                error!("Could not clone or fetch repo: {:?}", err);
            }
        }
        let _ = cli_commands::execute_all(&self.cli_commands);
        Ok(())
    }

    async fn download_and_install_all(
        &self,
        download_directory: &PathBuf,
        home_dir: &PathBuf,
        dotfiles_repository_path: &PathBuf,
    ) {
        let client = Client::default();
        join_all(self.downloads.github_releases.iter().map(|details| {
            details.download_self(
                client.clone(),
                download_directory.to_path_buf(),
                Self::create_download_asset_progress_bar(
                    &details.owner,
                    &details.repo,
                    download_directory.join(&details.repo),
                ),
            )
        }))
        .await;

        self.download_applications(client, download_directory).await;

        let github_releases_dotfiles_details = self
            .downloads
            .github_releases
            .clone()
            .into_iter()
            .filter_map(|release| release.dotfiles)
            .flatten();

        let github_releases_commands = self
            .downloads
            .github_releases
            .clone()
            .into_iter()
            .filter_map(|release| release.commands)
            .flatten();

        for details in github_releases_dotfiles_details {
            details.setup(home_dir, dotfiles_repository_path)
        }

        for command in github_releases_commands {
            let _ = command.execute();
        }
    }

    async fn download_applications(&self, client: Client, download_directory: &PathBuf) {
        let multi_progress = MultiProgress::new();

        let to_download = self.downloads.applications.len();
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

        let applications = self.downloads.applications.clone().into_iter();

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
        let git_clones = self.to_clones.clone().into_iter();

        let multi_progress = MultiProgress::new();
        let mut tasks = JoinSet::new();

        let coordinator_progress_bar = Self::create_progress_bar(
            self.to_clones.len(),
            format!("Cloning {} repositories", self.to_clones.len()),
            ProgressFinish::WithMessage(Cow::from(format!(
                "{} repos downloaded",
                self.to_clones.len()
            ))),
            ProgressStyle::with_template(&format!(
                "[{{elapsed_precise}}] {{bar:{}.cyan/blue}} {{pos:>7}}/{{len:7}} {{msg}}",
                self.to_clones.len()
            ))
            .unwrap()
            .progress_chars("✓▢▢"),
        );

        let coordinator_progress_bar = multi_progress.add(coordinator_progress_bar);
        coordinator_progress_bar.set_position(0);
        print!("\n");

        for repo_details in git_clones {
            let progress_bar = multi_progress.add(Self::create_download_asset_progress_bar(
                &repo_details.owner,
                &repo_details.repo,
                directory_path.join(&repo_details.repo),
            ));
            let token = token.clone();
            let directory_path = directory_path.clone();
            tasks.spawn(async move {
                repo_details
                    .clone_and_execute(directory_path, token, progress_bar)
                    .await
            });
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
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            version: CONFIG_DEFAULT_VERSION.to_owned(),
            clone_config: GitCloneConfig::default(),
            dotfiles_repository: GitClone::default(),
            downloads: Downloads::default(),
            to_clones: vec![GitClone::default()],
            dotfiles: Some(vec![DetailsType::File(FileDetails::default())]),
            cli_commands: Some(vec![CliCommand::default()]),
        }
    }
}

impl Default for GitCloneConfig {
    fn default() -> Self {
        Self {
            repositories_directory_path: Default::default(),
            github_username: Default::default(),
        }
    }
}

impl Downloads {
    fn new() -> Self {
        Self {
            applications: vec![ApplicationDetails::default()],
            github_releases: vec![RepositoryDetails {
                owner: "cli".to_owned(),
                repo: "cli".to_owned(),
                asset_find: Some(AssetFind::AssetEndsWith {
                    asset_ends_with: "_windows_amd64.msi".to_owned(),
                }),
                commands: Some(vec![CliCommand::new(
                    true,
                    vec!["gh".to_owned(), "auth".to_owned(), "login".to_owned()],
                )]),
                dotfiles: None,
            }],
        }
    }
}

impl Default for Downloads {
    fn default() -> Self {
        Self {
            applications: vec![ApplicationDetails::default()],
            github_releases: vec![RepositoryDetails::default()],
        }
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

impl Default for ApplicationDetails {
    fn default() -> Self {
        Self {
            name: String::new(),
            uri: Url::parse("http://localhost").unwrap(),
            dotfiles: None,
        }
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

impl Default for RepositoryDetails {
    fn default() -> Self {
        Self {
            owner: String::new(),
            repo: String::new(),
            asset_find: None,
            commands: None,
            dotfiles: None,
        }
    }
}

impl GitClone {
    pub async fn clone_and_execute(
        &self,
        directory_path: PathBuf,
        token: Option<secrecy::SecretBox<str>>,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        self.git_clone(directory_path, token, progress_bar).await?;
        cli_commands::execute_all(&self.cli_commands);
        Ok(())
    }
    async fn git_clone(
        &self,
        directory_path: PathBuf,
        token: Option<SecretString>,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        let token = match token {
            Some(token) => token,
            None => {
                github::initialise_octocrab(&self.owner)?;
                github::get_github_token()
            }
        };

        let repo = octocrab::instance()
            .repos(&self.owner, &self.repo)
            .get()
            .await
            .expect("Invalid repo");

        fs::create_dir_all(&directory_path)
            .with_context(|| format!("Could not create directory: {:#?}", &directory_path))?;

        let directory_path = directory_path.join(self.repo.clone());

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

                let fetch_options =
                    github::create_repository_fetch_options(&token, &self.owner, progress_bar);

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

impl Default for GitClone {
    fn default() -> Self {
        Self {
            owner: String::new(),
            repo: String::new(),
            cli_commands: None,
        }
    }
}

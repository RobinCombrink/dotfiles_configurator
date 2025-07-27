use {
    crate::{
        download::Downloader,
        execution_plan::DownloadType,
        github,
        progress_bar::{create_download_application_progress_bar, create_git_clone_progress_bar},
        shell_command,
    },
    anyhow::{anyhow, Context, Result},
    common::configuration::{ApplicationDetails, AssetFind, GitClone, RepositoryDetails},
    git2::build::RepoBuilder,
    github_authentication::authentication::Authentication,
    indicatif::ProgressBar,
    log::{info, trace},
    octocrab::Octocrab,
    reqwest::Client,
    std::{fs, future::Future, path::PathBuf, sync::Arc},
};

pub trait Executor {
    fn execute(&self) -> impl Future<Output = Result<()>> + Send;
}

pub trait AssetDownloaderExecutor {}

pub trait FileDownloaderExecutor {}

pub trait DotfileExecutor {}

pub trait CloneExecutor {}

pub trait ShellCommandExecutor {}

pub(crate) trait ItemProgress {
    fn create_progress_bar(&self, path: PathBuf) -> ProgressBar;
}

impl ItemProgress for DownloadType {
    fn create_progress_bar(&self, path: PathBuf) -> ProgressBar {
        match self {
            DownloadType::Application(_) => create_download_application_progress_bar(),
            DownloadType::GitHubAsset(_) => create_download_application_progress_bar(),
        }
    }
}

impl ItemProgress for GitClone {
    fn create_progress_bar(&self, path: PathBuf) -> ProgressBar {
        create_git_clone_progress_bar(&self.owner, &self.repo, path.join(&self.repo))
    }
}

pub trait ExecutorSync {
    fn execute_sync(&self) -> Result<String>;
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

pub struct GitCloneArgs<T: Authentication> {
    git_clone: GitClone,
    directory_path: PathBuf,
    authentication: T,
    octocrab: Arc<Octocrab>,
}

impl<T: Authentication> GitCloneArgs<T> {
    pub fn from_gitclone(
        git_clone: GitClone,
        directory_path: PathBuf,
        authentication: T,
        octocrab: Arc<Octocrab>,
    ) -> GitCloneArgs<T> {
        GitCloneArgs {
            git_clone,
            directory_path,
            authentication,
            octocrab,
        }
    }
    pub async fn clone_and_execute(&self, progress_bar: ProgressBar) -> Result<()> {
        self.git_clone(progress_bar).await?;
        shell_command::execute_all(&self.git_clone.shell_commands).await;
        Ok(())
    }
    async fn git_clone(&self, progress_bar: ProgressBar) -> Result<()> {
        progress_bar.set_position(0);
        let token = self.authentication.get_token();
        let repo = self
            .octocrab
            .repos(&self.git_clone.owner, &self.git_clone.repo)
            .get()
            .await
            .expect("Invalid repo");

        info!("clone dir: {:#?}", &self.directory_path);
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

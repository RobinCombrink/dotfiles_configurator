use std::{fmt::Display, fs, path::PathBuf, sync::Arc};
use anyhow::{Context, Result, anyhow};
use secrecy::{ExposeSecret, SecretString};
use octocrab::Octocrab;
use indicatif::ProgressBar;
use git2::{Cred, FetchOptions, RemoteCallbacks, build::RepoBuilder};
use crate::configuration::GitClone;
use github_authentication::authentication::Authentication;

pub struct GitCloneArgs<T: Authentication> {
    pub git_clone: GitClone,
    pub directory_path: PathBuf,
    pub authentication: T,
}

impl<T: Authentication> Display for GitCloneArgs<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "Repository: {owner}/{repo}, Directory: {directory}",
            owner = self.git_clone.owner,
            repo = self.git_clone.repo,
            directory = self.directory_path.to_string_lossy()
        ))
    }
}

impl<T: Authentication> GitCloneArgs<T> {
    pub fn from_gitclone(
        git_clone: GitClone,
        directory_path: PathBuf,
        authentication: T,
    ) -> GitCloneArgs<T> {
        GitCloneArgs {
            git_clone,
            directory_path,
            authentication,
        }
    }
    pub async fn git_clone(
        &self,
        octocrab: Arc<Octocrab>,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        progress_bar.set_position(0);
        let token = self.authentication.get_token();

        let repo = octocrab
            .repos(&self.git_clone.owner, &self.git_clone.repo)
            .get()
            .await
            .with_context(|| format!("Could not get repository details: {}", self))?;

        fs::create_dir_all(&self.directory_path)
            .with_context(|| format!("Could not create directory: {:#?}", &self.directory_path))?;

        let directory_path = self.directory_path.join(self.git_clone.repo.clone());

        let branch = repo.default_branch.ok_or_else(|| {
            anyhow!(
                "Repository: {}/{} does not have a default branch",
                self.git_clone.owner,
                self.git_clone.repo
            )
        })?;

        let local_repo = git2::Repository::open(&directory_path);
        let mut fetch_options = self.create_repository_fetch_options(&token, progress_bar);
        match local_repo {
            Ok(local_repo) => local_repo
                .find_remote("origin")
                .expect("Imagine not using origin as your remote name")
                .fetch(&[branch], Some(&mut fetch_options), None)
                .with_context(|| {
                    format!(
                        "Could not fetch origin/main for local repository: {}",
                        repo.full_name.unwrap_or_else(|| repo.name.clone())
                    )
                }),
            Err(_) => {
                let url = &repo.html_url.ok_or(anyhow!(
                    "{} does not have an html url",
                    repo.full_name.unwrap_or_else(|| repo.name.clone())
                ))?;

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
    fn create_repository_fetch_options<'token>(
        &'token self,
        token: &'token SecretString,
        progress_bar: ProgressBar,
    ) -> FetchOptions<'token> {
        let mut callbacks = RemoteCallbacks::new();

        callbacks.transfer_progress(move |progress| {
            let objects_progress_percent = ((progress.received_objects() as f64
                / progress.total_objects() as f64)
                * 100 as f64)
                .floor() as u64;

            let deltas_progress_percent =
                (progress.indexed_deltas() as f64 / progress.total_deltas() as f64 * 100 as f64)
                    .floor() as u64;

            if progress.received_objects() == progress.total_objects() {
                progress_bar.set_message(format!(
                    "Resolving deltas {}/{} ({}%) for {}",
                    progress.indexed_deltas(),
                    progress.total_deltas(),
                    deltas_progress_percent,
                    self.git_clone
                ));
            } else if progress.total_objects() > 0 {
                progress_bar.set_message(format!(
                    "Received {}/{} ({}%) objects ({}) in {} bytes for {}",
                    progress.received_objects(),
                    progress.total_objects(),
                    objects_progress_percent,
                    progress.indexed_objects(),
                    progress.received_bytes(),
                    self.git_clone,
                ));
            }

            let progress_position = (((objects_progress_percent as f64 / 100 as f64)
                + (deltas_progress_percent as f64 / 100 as f64)
                    * progress_bar.length().unwrap_or_else(|| 100) as f64)
                .floor()
                / (2 as f64)) as u64;

            progress_bar.set_position(progress_position);
            if objects_progress_percent >= 100 && deltas_progress_percent >= 100 {
                progress_bar.finish_using_style();
            }
            true
        });

        callbacks.credentials(move |_url, username_from_url, _allowed_types| {
            Cred::userpass_plaintext(
                username_from_url.unwrap_or_else(|| &self.git_clone.owner),
                token.expose_secret(),
            )
        });

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);
        fetch_options.depth(1);

        return fetch_options;
    }
}

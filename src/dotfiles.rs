use anyhow::{Context, Result};
use common::configuration::{DetailsType, DirectoryDetails, FileDetails};
use log::info;
use std::path::PathBuf;

use crate::execution::Executor;

#[derive(Debug, Clone)]
pub struct DotfilesDetails {
    pub(crate) details: DetailsType,
    pub(crate) home_dir: PathBuf,
    pub(crate) dotfiles_repository_path: PathBuf,
}

impl DotfilesDetails {
    pub fn from_details(
        details: DetailsType,
        dotfiles_repository_path: PathBuf,
        home_dir: PathBuf,
    ) -> Self {
        Self {
            details,
            dotfiles_repository_path,
            home_dir,
        }
    }
}

pub(crate) trait PathFinder {
    fn original_path(&self, dotfiles_repository_path: &PathBuf) -> PathBuf;
    fn link_path(&self, home_dir: &PathBuf) -> PathBuf;
}

impl PathFinder for FileDetails {
    fn original_path(&self, dotfiles_repository_path: &PathBuf) -> PathBuf {
        dotfiles_repository_path.join(&self.original_path.join(&self.file_name))
    }

    fn link_path(&self, home_dir: &PathBuf) -> PathBuf {
        let link_path = match &self.link_path {
            Some(path) => path,
            None => &home_dir,
        };
        home_dir.join(link_path).join(&self.file_name)
    }
}

impl PathFinder for DirectoryDetails {
    fn original_path(&self, dotfiles_repository_path: &PathBuf) -> PathBuf {
        dotfiles_repository_path.join(&self.original_path)
    }

    fn link_path(&self, home_dir: &PathBuf) -> PathBuf {
        home_dir.join(&self.link_path)
    }
}

impl Executor for DotfilesDetails {
    async fn execute(&self) -> Result<()> {
        match &self.details {
            DetailsType::File(details) => {
                let original_path = details.original_path(&self.dotfiles_repository_path);
                let link_path = details.link_path(&self.home_dir);

                symlink_file(&original_path, &link_path).with_context(|| {
                    format!(
                        "Could not create file symlink\nLink path: {:#?}\nOriginal Path: {:#?} ",
                        link_path, original_path
                    )
                })
            }
            DetailsType::Directory(details) => {
                let original_path = details.original_path(&self.dotfiles_repository_path);
                let link_path = details.link_path(&self.home_dir);
                symlink_directory(&original_path, &link_path).with_context(||format!("Could not create directory symlink\nLink path: {:#?}\nOriginal Path: {:#?} ", link_path, original_path ))
            }
        }
    }
}

#[cfg(target_family = "windows")]
fn symlink_file(original_path: &PathBuf, link_path: &PathBuf) -> Result<()> {
    log_symlink(&original_path, &link_path);

    std::os::windows::fs::symlink_file(original_path, link_path).map_err(|err| err.into())
}

#[cfg(target_family = "unix")]
fn symlink_file(original_path: &PathBuf, link_path: &PathBuf) -> Result<()> {
    log_symlink(&original_path, &link_path);

    std::os::unix::fs::symlink(original_path, link_path).map_err(|err| err.into())
}

#[cfg(target_family = "windows")]
fn symlink_directory(original_path: &PathBuf, link_path: &PathBuf) -> Result<()> {
    log_symlink(&original_path, &link_path);

    std::os::windows::fs::symlink_dir(original_path, link_path).map_err(|err| err.into())
}

#[cfg(target_family = "unix")]
fn symlink_directory(original_path: &PathBuf, link_path: &PathBuf) -> Result<()> {
    log_symlink(&original_path, &link_path);

    std::os::unix::fs::symlink(original_path, link_path).map_err(|err| err.into())
}

fn log_symlink(original_path: &PathBuf, link_path: &PathBuf) {
    info!(
        " \r\nOriginal: {:#?}\r\nLink: {:#?}",
        original_path, link_path
    );
}

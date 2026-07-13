use anyhow::{Context, Result};
use crate::configuration::{DetailsType, DirectoryDetails, FileDetails};
use log::info;
use std::path::{Path, PathBuf};

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
    fn original_path(&self, dotfiles_repository_path: &Path) -> PathBuf;
    fn link_path(&self, home_dir: &Path) -> PathBuf;
}

impl PathFinder for FileDetails {
    fn original_path(&self, dotfiles_repository_path: &Path) -> PathBuf {
        dotfiles_repository_path.join(self.original_path.join(&self.file_name))
    }

    fn link_path(&self, home_dir: &Path) -> PathBuf {
        match &self.link_path {
            Some(path) => home_dir.join(path).join(&self.file_name),
            None => home_dir.join(&self.file_name),
        }
    }
}

impl PathFinder for DirectoryDetails {
    fn original_path(&self, dotfiles_repository_path: &Path) -> PathBuf {
        dotfiles_repository_path.join(&self.original_path)
    }

    fn link_path(&self, home_dir: &Path) -> PathBuf {
        home_dir.join(&self.link_path)
    }
}

impl Executor for DotfilesDetails {
    async fn execute(&self) -> Result<()> {
        match &self.details {
            DetailsType::File(details) => {
                let original_path = details.original_path(&self.dotfiles_repository_path);
                let link_path = details.link_path(&self.home_dir);

                create_link_parent_directory(&link_path)?;
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
                create_link_parent_directory(&link_path)?;
                symlink_directory(&original_path, &link_path).with_context(||format!("Could not create directory symlink\nLink path: {:#?}\nOriginal Path: {:#?} ", link_path, original_path ))
            }
        }
    }
}

fn create_link_parent_directory(link_path: &Path) -> Result<()> {
    match link_path.parent() {
        Some(parent_directory) => {
            std::fs::create_dir_all(parent_directory).with_context(|| {
                format!(
                    "Could not create parent directory for symlink: {:#?}",
                    parent_directory
                )
            })
        }
        None => Ok(()),
    }
}

#[cfg(target_family = "windows")]
fn symlink_file(original_path: &PathBuf, link_path: &PathBuf) -> Result<()> {
    log_symlink(original_path, link_path);

    std::os::windows::fs::symlink_file(original_path, link_path).map_err(|err| err.into())
}

#[cfg(target_family = "unix")]
fn symlink_file(original_path: &PathBuf, link_path: &PathBuf) -> Result<()> {
    log_symlink(original_path, link_path);

    std::os::unix::fs::symlink(original_path, link_path).map_err(|err| err.into())
}

#[cfg(target_family = "windows")]
fn symlink_directory(original_path: &PathBuf, link_path: &PathBuf) -> Result<()> {
    log_symlink(original_path, link_path);

    std::os::windows::fs::symlink_dir(original_path, link_path).map_err(|err| err.into())
}

#[cfg(target_family = "unix")]
fn symlink_directory(original_path: &PathBuf, link_path: &PathBuf) -> Result<()> {
    log_symlink(original_path, link_path);

    std::os::unix::fs::symlink(original_path, link_path).map_err(|err| err.into())
}

fn log_symlink(original_path: &PathBuf, link_path: &PathBuf) {
    info!(
        " \r\nOriginal: {:#?}\r\nLink: {:#?}",
        original_path, link_path
    );
}

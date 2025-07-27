use anyhow::{Context, Result};
use common::configuration::DetailsType;
use log::info;
use std::path::PathBuf;

use crate::impls::Executor;

#[derive(Debug, Clone)]
pub struct DotfilesDetails {
    details: DetailsType,
    home_dir: PathBuf,
    dotfiles_repository_path: PathBuf,
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

impl Executor for DotfilesDetails {
    async fn execute(&self) -> Result<()> {
        match &self.details {
            DetailsType::File(details) => {
                let link_path = match &details.link_path {
                    Some(path) => path,
                    None => &self.home_dir,
                };
                let original_path = self
                    .dotfiles_repository_path
                    .join(&details.original_path.join(&details.file_name));
                let link_path = self.home_dir.join(link_path).join(&details.file_name);

                symlink_file(&original_path, &link_path).with_context(|| {
                    format!(
                        "Could not create file symlink\nLink path: {:#?}\nOriginal Path: {:#?} ",
                        link_path, original_path
                    )
                })
            }
            DetailsType::Directory(details) => {
                let original_path = self.dotfiles_repository_path.join(&details.original_path);
                let link_path = self.home_dir.join(&details.link_path);

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

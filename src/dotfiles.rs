use anyhow::{anyhow, Result};
use common::configuration::DetailsType;
use log::info;
use std::path::PathBuf;

use crate::impls::Executor;

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

//TODO: Add Unix directory link
impl Executor for DotfilesDetails {
    async fn execute(&self) -> Result<()> {
        let symlink_result = match &self.details {
            DetailsType::File(details) => {
                let link_path = match &details.link_path {
                    Some(path) => path,
                    None => &self.home_dir,
                };
                let original_path = self
                    .dotfiles_repository_path
                    .join(&details.original_path.join(&details.file_name));
                let link_path = self.home_dir.join(link_path).join(&details.file_name);

                log_symlink(&original_path, &link_path);

                #[cfg(target_family = "windows")]
                {
                    std::os::windows::fs::symlink_file(original_path, link_path)
                }
            }
            DetailsType::Directory(details) => {
                let original_path = self.dotfiles_repository_path.join(&details.original_path);
                let link_path = self.home_dir.join(&details.link_path);

                log_symlink(&original_path, &link_path);

                #[cfg(target_family = "windows")]
                {
                    std::os::windows::fs::symlink_dir(original_path, link_path)
                }
            }
        };

        match symlink_result {
            Ok(ok) => Ok(ok),
            Err(e) => Err(anyhow!(format!("Could not create symlink: {e}"))),
        }
    }
}

fn log_symlink(original_path: &PathBuf, link_path: &PathBuf) {
    info!(
        " \r\nOriginal: {:#?}\r\nLink: {:#?}",
        original_path, link_path
    );
}

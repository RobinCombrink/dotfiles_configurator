use log::{error, info};
use std::{fs, io, path::PathBuf};
use walkdir::WalkDir;

use crate::common::DetailsType;


impl DetailsType {
    pub fn setup(&self, home_dir: &PathBuf, dotfiles_repository_path: &PathBuf) {
        let symlink_result = match self {
            DetailsType::File(details) => {
                let link_path = match &details.link_path {
                    Some(path) => path,
                    None => &home_dir,
                };
                let original_path =
                    dotfiles_repository_path.join(&details.original_path.join(&details.file_name));
                let link_path = home_dir.join(link_path).join(&details.file_name);

                log_symlink(&original_path, &link_path);
                fs::hard_link(&original_path, link_path)
            }
            DetailsType::Directory(details) => {
                let original_path = dotfiles_repository_path.join(&details.original_path);
                let link_path = home_dir.join(&details.link_path);

                log_symlink(&original_path, &link_path);
                hard_link_directory(&original_path, &link_path)
            }
        };

        match symlink_result {
            Ok(ok) => ok,
            Err(e) => error!("There was an error creating the symlink: {e}"),
        }
    }
}

fn log_symlink(original_path: &PathBuf, link_path: &PathBuf) {
    info!(
        " \r\nOriginal: {:#?}\r\nLink: {:#?}",
        original_path, link_path
    );
}

fn hard_link_directory(original_path: &PathBuf, link_path: &PathBuf) -> io::Result<()> {
    for entry in WalkDir::new(&original_path) {
        let entry = entry?;
        let path = entry.path();
        let relative_path = path.strip_prefix(&original_path).unwrap();
        let link_directory_path = &link_path.join(relative_path);

        if path.is_dir() {
            fs::create_dir_all(&link_directory_path)?;
        } else {
            fs::hard_link(path, &link_directory_path)?;
        }
    }
    Ok(())
}

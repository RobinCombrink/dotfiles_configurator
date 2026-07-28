use crate::dotfiles::DotfilesDetails;
use crate::execution::shell_command::CommandGetter;
use crate::execution_plan::DownloadType;
use crate::{dotfiles::PathFinder, progress_bar::create_progress_bar};
use crate::configuration::{DetailsType, GitClone, ShellCommand};
use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};
use std::{borrow::Cow, path::PathBuf};

pub(crate) trait ItemProgress {
    fn create_progress_bar(&self, path: PathBuf) -> ProgressBar;
}

impl ItemProgress for DownloadType {
    fn create_progress_bar(&self, _path: PathBuf) -> ProgressBar {
        let progress_bar = ProgressBar::new(0).with_style(ProgressStyle::default_bar()
                 .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}")
                 .unwrap()
                 .progress_chars("#>-"));
        match self {
            DownloadType::Application(_) => progress_bar,
            DownloadType::GitHubAsset(_) => progress_bar,
        }
    }
}

impl ItemProgress for GitClone {
    fn create_progress_bar(&self, path: PathBuf) -> ProgressBar {
        let message = format!("Cloning {}/{} into {:#?}", self.owner, self.repo, path,);

        let finish = ProgressFinish::WithMessage(Cow::from(format!(
            "Cloned {}/{} into {:#?}",
            self.owner, self.repo, path,
        )));

        let style = ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {msg}")
            .unwrap()
            .progress_chars("##-");
        create_progress_bar(40, message, finish, style)
    }
}

impl ItemProgress for ShellCommand {
    fn create_progress_bar(&self, _path: PathBuf) -> ProgressBar {
        let (arguments, shell, _) = self.get_shell_command();
        let command = format!("{shell} {}", arguments.join(" "));
        let message = format!("Executing:  {command}");

        let finish = ProgressFinish::WithMessage(Cow::from(format!("Executed: {command}")));

        let style = ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap()
            .progress_chars("##-");
        create_progress_bar(40, message, finish, style)
    }
}

impl ItemProgress for DotfilesDetails {
    fn create_progress_bar(&self, _path: PathBuf) -> ProgressBar {
        let (dotfiles_type, original_path, link_path) = match &self.details {
            DetailsType::File(file_details) => (
                "file",
                file_details.original_path(&self.dotfiles_repository_path),
                file_details.link_path(&self.home_dir),
            ),
            DetailsType::Directory(directory_details) => (
                "directory",
                directory_details.original_path(&self.dotfiles_repository_path),
                directory_details.link_path(&self.home_dir),
            ),
        };
        let message = format!(
            "Setting up {} dotfile symlink: {:#?} -> {:#?}",
            dotfiles_type, original_path, link_path
        );

        let finish = ProgressFinish::WithMessage(Cow::from(format!(
            "Set up {} dotfile symlink: {:#?} -> {:#?}",
            dotfiles_type, original_path, link_path
        )));

        let style = ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap()
            .progress_chars("##-");
        create_progress_bar(40, message, finish, style)
    }
}

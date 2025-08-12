use {common::configuration::GitCloneConfig, std::path::PathBuf};

pub(crate) trait DotfilesPaths {
    fn dotfiles_repository_path(&self) -> PathBuf;
}

impl DotfilesPaths for GitCloneConfig {
    fn dotfiles_repository_path(&self) -> PathBuf {
        self.repositories_directory_path
            .join(&self.dotfiles_repository.repo)
    }
}

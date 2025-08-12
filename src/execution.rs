use anyhow::Result;
use std::future::Future;

pub mod dotfiles_paths;
pub mod download;
pub mod git_clone_args;
pub mod item_progress;
pub mod shell_command;

pub trait Executor {
    fn execute(&self) -> impl Future<Output = Result<()>> + Send;
}

pub trait ExecutorSync {
    fn execute_sync(&self) -> Result<String>;
}

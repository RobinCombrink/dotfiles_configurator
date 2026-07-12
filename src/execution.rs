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

// Not yet wired into the execution plan: this is the synchronous, output-capturing
// counterpart to `Executor`, intended for `require_output` shell commands whose stdout
// feeds later steps.
#[allow(dead_code)]
pub trait ExecutorSync {
    fn execute_sync(&self) -> Result<String>;
}

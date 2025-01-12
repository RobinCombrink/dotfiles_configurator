use std::process::Command;

use anyhow::Result;
use common::configuration::CliCommand;
use futures::future::join_all;

use crate::impls::Executor;

pub async fn execute_all(cli_commands: &Option<Vec<CliCommand>>) -> Option<Vec<Result<()>>> {
    match cli_commands {
        Some(cli_commands) => {
            Some(join_all(cli_commands.into_iter().map(|command| command.execute())).await)
        }
        None => None,
    }
}

impl Executor for CliCommand {
    async fn execute(&self) -> Result<()> {
        let args = if self.interactive {
            let mut args = vec!["/C".to_owned(), "start".to_owned()];
            args.extend(self.args.iter().cloned());
            args
        } else {
            self.args.clone()
        };
        let result = Command::new("cmd").args(args).spawn();
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

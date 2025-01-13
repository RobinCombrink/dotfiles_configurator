use std::{future::Future, process::Command};

use anyhow::Result;
use common::configuration::ShellCommand;
use futures::future::join_all;
use log::info;

use crate::impls::Executor;

pub async fn execute_all(cli_commands: &Option<Vec<ShellCommand>>) -> Option<Vec<Result<()>>> {
    match cli_commands {
        Some(cli_commands) => {
            Some(join_all(cli_commands.into_iter().map(|command| command.execute())).await)
        }
        None => None,
    }
}

impl Executor for ShellCommand {
    fn execute(&self) -> impl Future<Output = Result<()>> + Send {
        let mut args = vec![];

        let shell_program = match self {
            ShellCommand::Bash(cli_command) => {
                args.extend(cli_command.args.clone());
                "bash"
            }
            ShellCommand::CommandPrompt(cli_command) => {
                if cli_command.interactive {
                    args.extend(vec!["/C".into(), "start".into()]);
                }
                args.extend(cli_command.args.clone());
                "cmd"
            }
            ShellCommand::Powershell(cli_command) => {
                if cli_command.interactive {
                    args.extend(vec![
                        "-Command".into(),
                        format!("\"Start-Process pwsh -ArgumentList").into(),
                    ]);
                    args.push(format!("'{}'\"", cli_command.args.clone().join(" ")));
                } else {
                    args.extend(cli_command.args.clone());
                }
                "pwsh"
            }
            ShellCommand::WSL(cli_command) => {
                if cli_command.interactive {
                    args.extend(vec!["/C".into(), "start".into(), "wsl".into()]);
                }
                args.extend(cli_command.args.clone());
                "cmd"
            }
        };
        info!("In {shell_program}, executing {}", args.join(" "));

        let result = Command::new(shell_program).args(args).spawn();
        async {
            match result {
                Ok(_) => Ok(()),
                Err(e) => Err(e.into()),
            }
        }
    }
}

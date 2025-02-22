use std::{future::Future, process::Command};

use anyhow::{anyhow, Context, Result};
use common::configuration::ShellCommand;
use futures::future::join_all;
use log::info;

use crate::impls::{Executor, ExecutorSync};

pub async fn execute_all(cli_commands: &Option<Vec<ShellCommand>>) -> Option<Vec<Result<()>>> {
    match cli_commands {
        Some(cli_commands) => {
            Some(join_all(cli_commands.into_iter().map(|command| command.execute())).await)
        }
        None => None,
    }
}

fn get_shell_command(shell_command: &ShellCommand) -> (Vec<String>, &str, bool) {
    match shell_command {
        ShellCommand::Bash(cli_command) => {
            (cli_command.args.clone(), "bash", cli_command.interactive)
        }
        ShellCommand::CommandPrompt(cli_command) => {
            let mut args = vec![];
            if cli_command.require_output {
                args.extend(vec!["/C".into()]);
            } else if cli_command.interactive {
                args.extend(vec!["/C".into(), "start".into()]);
            }

            args.extend(cli_command.args.clone());
            (args, "cmd", cli_command.interactive)
        }
        ShellCommand::Powershell(cli_command) => {
            let mut args = vec![];
            if cli_command.interactive {
                args.extend(vec![
                    "-Command".into(),
                    format!("\"Start-Process pwsh -ArgumentList").into(),
                ]);
                args.push(format!("'{}'\"", cli_command.args.clone().join(" ")));
            } else {
                args.extend(cli_command.args.clone());
            }
            (args, "pwsh", cli_command.interactive)
        }
        ShellCommand::WSL(cli_command) => {
            let mut args = vec![];
            if cli_command.interactive {
                args.extend(vec!["/C".into(), "start".into(), "wsl".into()]);
            }
            args.extend(cli_command.args.clone());
            (args, "cmd", cli_command.interactive)
        }
    }
}

impl Executor for ShellCommand {
    fn execute(&self) -> impl Future<Output = Result<()>> + Send {
        let (args, shell_program, interactive) = get_shell_command(self);
        info!("In {shell_program}, executing {}", args.join(" "));

        let result = Command::new(shell_program).args(args).spawn();
        async move {
            match result {
                Ok(child) => {
                    if !interactive.clone() {
                        match child.wait_with_output() {
                            Ok(output) => match output.status.success() {
                                true => Ok(()),
                                false => {
                                    Err(anyhow!("Something went wrong executing the command",))
                                }
                            },
                            Err(e) => Err(anyhow!("Something went wrong executing the command"))
                                .with_context(|| e),
                        }
                    } else {
                        Ok(())
                    }
                }
                Err(e) => {
                    Err(anyhow!("Something went wrong executing the command")).with_context(|| e)
                }
            }
        }
    }
}

impl ExecutorSync for ShellCommand {
    fn execute_sync(&self) -> Result<String> {
        let (args, shell_program, _) = get_shell_command(self);
        info!("In {shell_program}, executing {}", args.join(" "));

        let result = Command::new(shell_program)
            .args(&args)
            .output()
            .with_context(|| {
                format!(
                    "Something went wrong executing the command: {:#?} in the program {}",
                    args, shell_program
                )
            });

        match result {
            Ok(output) => match output.status.success() {
                true => Ok(String::from_utf8(output.stdout).expect("Uf8 only for standard out")),
                false => {
                    Err(anyhow!("Command exited with non zero exit status")).with_context(|| {
                        format!(
                            "status: {:#?}\nstdout: {}\nstderr: {},",
                            output.status.code(),
                            String::from_utf8(output.stdout).expect("Uf8 only for standard out"),
                            String::from_utf8(output.stderr).expect("Uf8 only for standard err"),
                        )
                    })
                }
            },
            Err(e) => Err(anyhow!("Something went wrong executing the command")).with_context(|| e),
        }
    }
}

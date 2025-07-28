use {
    crate::impls::{Executor, ExecutorSync},
    anyhow::{Context, Error, Result, anyhow},
    common::configuration::ShellCommand,
    futures::future::join_all,
    log::info,
    std::{
        future::Future,
        process::{Command, Stdio},
    },
};

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
    fn execute(&self) -> impl Future<Output = Result<(), Error>> {
        let (args, shell_program, interactive) = get_shell_command(self);
        let command_str = format!("{} {}", shell_program, args.join(" "));
        info!("Executing: {command_str}");

        let result = Command::new(shell_program)
            .args(&args)
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn();
        async move {
            let res = match result {
                Ok(child) => {
                    if !interactive {
                        match child.wait_with_output() {
                            Ok(output) => {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                if output.status.success() {
                                    Ok(())
                                } else {
                                    let err_msg = format!(
                                        "Non success output status\nstatus: {:?}\nstdout: {}\nstderr: {}",
                                        output.status.code(),
                                        stdout,
                                        stderr
                                    );
                                    Err(anyhow!(err_msg))
                                }
                            }
                            Err(e) => Err(Into::<Error>::into(e))
                                .with_context(|| "Failed to wait on child process"),
                        }
                    } else {
                        Ok(())
                    }
                }
                Err(e) => Err(e).with_context(|| format!("Failed to spawn process")),
            };
            res.with_context(|| format!("Error executing asynchronously: {command_str}"))
        }
    }
}

impl ExecutorSync for ShellCommand {
    fn execute_sync(&self) -> Result<String> {
        let (args, shell_program, _) = get_shell_command(self);
        let command_str = format!("{} {}", shell_program, args.join(" "));
        info!("Executing: {command_str}");

        let result = Command::new(shell_program)
            .args(&args)
            .output()
            .with_context(|| format!("Error executing synchronously: {command_str}"));

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                if output.status.success() {
                    Ok(stdout)
                } else {
                    let err_msg = format!(
                        "Non success output status\nstatus: {:?}\nstdout: {}\nstderr: {}",
                        output.status.code(),
                        stdout,
                        stderr
                    );
                    Err(anyhow!(err_msg))
                }
            }
            Err(e) => Err(e),
        }
    }
}

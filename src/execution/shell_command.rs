use {
    crate::execution::{Executor, ExecutorSync},
    anyhow::{Context, Error, Result, anyhow},
    crate::configuration::ShellCommand,
    log::info,
    std::{
        future::Future,
        process::{Command, Stdio},
    },
};

pub trait CommandGetter {
    fn get_shell_command(&self) -> (Vec<String>, &str, bool);
}

impl CommandGetter for ShellCommand {
    fn get_shell_command(&self) -> (Vec<String>, &str, bool) {
        match self {
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
}

impl Executor for ShellCommand {
    fn execute(&self) -> impl Future<Output = Result<(), Error>> {
        let (args, shell_program, interactive) = self.get_shell_command();
        let command = format!("{} {}", shell_program, args.join(" "));
        info!("Executing: {command}");

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
            res.with_context(|| format!("Error executing asynchronously: {command}"))
        }
    }
}

impl ExecutorSync for ShellCommand {
    fn execute_sync(&self) -> Result<String> {
        let (args, shell_program, _) = self.get_shell_command();
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

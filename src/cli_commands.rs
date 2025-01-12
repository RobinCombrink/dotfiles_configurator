use std::process::Command;

use anyhow::Result;

use crate::common::CliCommand;

pub fn execute_all(cli_commands: &Option<Vec<CliCommand>>) -> Option<Vec<Result<()>>> {
    match cli_commands {
        Some(cli_commands) => Some(
            cli_commands
                .into_iter()
                .map(|command| command.execute())
                .collect(),
        ),
        None => None,
    }
}

impl CliCommand {
    pub fn new(interactive: bool, args: Vec<String>) -> Self {
        Self { interactive, args }
    }
    pub fn execute(&self) -> Result<()> {
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

impl Default for CliCommand {
    fn default() -> Self {
        Self {
            interactive: false,
            args: vec![String::default()],
        }
    }
}

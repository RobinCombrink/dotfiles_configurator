use crate::{
    configuration::{ClaudeMcpServer, McpScope},
    machine::Tool,
};

/// The closed set of invocations this crate defines for reading state.
///
/// Typing what may be run, rather than only who may run it, is what keeps plan's guarantee real:
/// a single general "run a process" capability would have handed plan the ability to run an
/// installer. See ADR 0006.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadInvocation {
    /// Whether winget reports a package as installed. Exits 0 when it is and
    /// `APPINSTALLER_CLI_ERROR_NO_APPLICATIONS_FOUND` when it is not.
    WingetPackage { id: String },
    /// Every crate Cargo has installed, one `name vX.Y.Z[ (source)]:` line each.
    CargoInstalledCrates,
    /// The details Claude Code holds for one MCP server. Exits non-zero when there is no such
    /// server.
    ClaudeMcpServer { name: String },
    /// The installed WSL distributions. Emits UTF-16LE when its output is redirected.
    WslDistributions,
}

impl ReadInvocation {
    pub fn tool(&self) -> Tool {
        match self {
            ReadInvocation::WingetPackage { .. } => Tool::Winget,
            ReadInvocation::CargoInstalledCrates => Tool::Cargo,
            ReadInvocation::ClaudeMcpServer { .. } => Tool::Claude,
            ReadInvocation::WslDistributions => Tool::Wsl,
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        match self {
            ReadInvocation::WingetPackage { id } => vec![
                "list".to_owned(),
                "--exact".to_owned(),
                "--id".to_owned(),
                id.clone(),
                "--accept-source-agreements".to_owned(),
                "--disable-interactivity".to_owned(),
            ],
            ReadInvocation::CargoInstalledCrates => {
                vec!["install".to_owned(), "--list".to_owned()]
            }
            ReadInvocation::ClaudeMcpServer { name } => {
                vec!["mcp".to_owned(), "get".to_owned(), name.clone()]
            }
            ReadInvocation::WslDistributions => {
                vec!["--list".to_owned(), "--quiet".to_owned()]
            }
        }
    }

    /// `wsl.exe` emits UTF-16LE whenever its output is redirected, which decodes as valid UTF-8
    /// with a NUL between every character — so a naive match against it silently never succeeds.
    pub fn output_is_utf16(&self) -> bool {
        match self {
            ReadInvocation::WslDistributions => true,
            ReadInvocation::WingetPackage { .. }
            | ReadInvocation::CargoInstalledCrates
            | ReadInvocation::ClaudeMcpServer { .. } => false,
        }
    }
}

/// The closed set of invocations this crate defines for changing state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteInvocation {
    InstallWingetPackage { id: String },
    InstallCargoCrate { arguments: Vec<String> },
    RemoveClaudeMcpServer { name: String, scope: McpScope },
    AddClaudeMcpServer { server: Box<ClaudeMcpServer> },
}

impl WriteInvocation {
    pub fn tool(&self) -> Tool {
        match self {
            WriteInvocation::InstallWingetPackage { .. } => Tool::Winget,
            WriteInvocation::InstallCargoCrate { .. } => Tool::Cargo,
            WriteInvocation::RemoveClaudeMcpServer { .. }
            | WriteInvocation::AddClaudeMcpServer { .. } => Tool::Claude,
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        match self {
            WriteInvocation::InstallWingetPackage { id } => vec![
                "install".to_owned(),
                "--exact".to_owned(),
                "--id".to_owned(),
                id.clone(),
                "--accept-package-agreements".to_owned(),
                "--accept-source-agreements".to_owned(),
                "--disable-interactivity".to_owned(),
            ],
            WriteInvocation::InstallCargoCrate { arguments } => arguments.clone(),
            WriteInvocation::RemoveClaudeMcpServer { name, scope } => vec![
                "mcp".to_owned(),
                "remove".to_owned(),
                "--scope".to_owned(),
                scope.as_argument().to_owned(),
                name.clone(),
            ],
            WriteInvocation::AddClaudeMcpServer { server } => {
                let mut arguments = vec![
                    "mcp".to_owned(),
                    "add".to_owned(),
                    "--scope".to_owned(),
                    server.scope.as_argument().to_owned(),
                    server.name.clone(),
                ];
                for (key, value) in &server.environment {
                    arguments.push("--env".to_owned());
                    arguments.push(format!("{key}={value}"));
                }
                arguments.push("--".to_owned());
                arguments.push(server.command.clone());
                arguments.extend(server.args.iter().cloned());
                arguments
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn adding_an_mcp_server_passes_its_environment_before_the_command() {
        let mut environment = BTreeMap::new();
        environment.insert("SERENA_HOME".to_owned(), "C:\\dotfiles\\serena".to_owned());
        let server = ClaudeMcpServer {
            name: "serena".to_owned(),
            scope: McpScope::User,
            command: "serena".to_owned(),
            args: vec!["start-mcp-server".to_owned()],
            environment,
        };

        let arguments = WriteInvocation::AddClaudeMcpServer {
            server: Box::new(server),
        }
        .arguments();

        assert_eq!(
            arguments,
            vec![
                "mcp",
                "add",
                "--scope",
                "user",
                "serena",
                "--env",
                "SERENA_HOME=C:\\dotfiles\\serena",
                "--",
                "serena",
                "start-mcp-server",
            ]
        );
    }

    #[test]
    fn only_the_wsl_read_is_marked_as_reporting_utf16() {
        assert!(ReadInvocation::WslDistributions.output_is_utf16());
        assert!(!ReadInvocation::CargoInstalledCrates.output_is_utf16());
    }
}

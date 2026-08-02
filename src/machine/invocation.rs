use crate::{
    configuration::{ClaudeMcpServer, McpScope, McpServerName, WingetPackageId},
    machine::Tool,
};

/// The closed set of invocations this crate defines for reading state.
///
/// Typing what may be run, rather than only who may run it, is what keeps plan's guarantee real:
/// a single general "run a process" capability would have handed plan the ability to run an
/// installer. See ADR 0006.
///
/// A variant naming no resource is read once for a whole change set; one naming a resource is
/// read per resource. See ADR 0010.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReadInvocation {
    /// Every package winget reports as installed, as a table whose columns are located from its
    /// header row.
    WingetInstalledPackages,
    /// Every crate Cargo has installed, one `name vX.Y.Z[ (source)]:` line each.
    CargoInstalledCrates,
    /// The details Claude Code holds for one MCP server. Exits non-zero when there is no such
    /// server. Read per resource because `claude mcp list` health-checks every server it
    /// reports, which costs more than asking about each one and reaches the network.
    ClaudeMcpServer { name: McpServerName },
}

impl ReadInvocation {
    pub fn tool(&self) -> Tool {
        match self {
            ReadInvocation::WingetInstalledPackages => Tool::Winget,
            ReadInvocation::CargoInstalledCrates => Tool::Cargo,
            ReadInvocation::ClaudeMcpServer { .. } => Tool::Claude,
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        match self {
            // 2026-08-02: winget sizes this table's columns to the data, not to the console,
            // whenever its output is redirected — measured at an 80-column console, where the
            // output was still 196 characters wide with nothing truncated.
            ReadInvocation::WingetInstalledPackages => vec![
                "list".to_owned(),
                "--accept-source-agreements".to_owned(),
                "--disable-interactivity".to_owned(),
            ],
            ReadInvocation::CargoInstalledCrates => {
                vec!["install".to_owned(), "--list".to_owned()]
            }
            ReadInvocation::ClaudeMcpServer { name } => {
                vec!["mcp".to_owned(), "get".to_owned(), name.to_string()]
            }
        }
    }
}

/// The closed set of invocations this crate defines for changing state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteInvocation {
    InstallWingetPackage {
        id: WingetPackageId,
    },
    InstallCargoCrate {
        arguments: Vec<String>,
    },
    RemoveClaudeMcpServer {
        name: McpServerName,
        scope: McpScope,
    },
    AddClaudeMcpServer {
        server: Box<ClaudeMcpServer>,
    },
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
                id.to_string(),
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
                name.to_string(),
            ],
            WriteInvocation::AddClaudeMcpServer { server } => {
                let mut arguments = vec![
                    "mcp".to_owned(),
                    "add".to_owned(),
                    "--scope".to_owned(),
                    server.scope.as_argument().to_owned(),
                    server.name.to_string(),
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
            name: McpServerName::from("serena"),
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
}

use {
    crate::{
        configuration::{
            ClaudeMcpServer, CrateName, GitHubAccount, GitHubRepository, McpServerName, Tool,
            WingetPackageId,
        },
        machine::{CommandOutput, Replacement, workspace_reading::Revision},
    },
    std::path::PathBuf,
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
    InstallWingetPackage { id: WingetPackageId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCargoSource {
    Registry,
    Path {
        path: PathBuf,
    },
    Repository {
        repository: GitHubRepository,
        account: GitHubAccount,
        revision: Revision,
    },
}

/// The closed set of invocations that write where the machine may be executing what they replace,
/// which is what keeps a destination unreachable without displacing: no variant of the set above
/// can name one. See ADR 0022.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplacingInvocation {
    InstallCargoCrate {
        crate_name: CrateName,
        source: ResolvedCargoSource,
    },
}

impl DisplacingInvocation {
    pub fn tool(&self) -> Tool {
        match self {
            DisplacingInvocation::InstallCargoCrate { .. } => Tool::Cargo,
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        match self {
            DisplacingInvocation::InstallCargoCrate {
                crate_name,
                source: ResolvedCargoSource::Registry,
            } => vec![
                "install".to_owned(),
                "--locked".to_owned(),
                "--force".to_owned(),
                crate_name.to_string(),
            ],
            DisplacingInvocation::InstallCargoCrate {
                source: ResolvedCargoSource::Path { path },
                ..
            } => vec![
                "install".to_owned(),
                "--locked".to_owned(),
                "--force".to_owned(),
                "--path".to_owned(),
                path.display().to_string(),
            ],
            DisplacingInvocation::InstallCargoCrate {
                crate_name,
                source:
                    ResolvedCargoSource::Repository {
                        repository,
                        account,
                        revision,
                    },
            } => vec![
                "install".to_owned(),
                "--locked".to_owned(),
                "--force".to_owned(),
                "--git".to_owned(),
                repository.fetch_url_as(account),
                "--rev".to_owned(),
                revision.to_string(),
                crate_name.to_string(),
            ],
        }
    }

    pub fn environment(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            // 2026-08-10: cargo's own libgit2 fetch cannot authenticate to a private GitHub
            // repository on a machine holding its credentials behind `gh auth git-credential`,
            // failing with "no authentication methods succeeded" against a cold cache. The git
            // command line runs that helper and fetches the same revision.
            DisplacingInvocation::InstallCargoCrate { .. } => {
                vec![("CARGO_NET_GIT_FETCH_WITH_CLI", "true")]
            }
        }
    }

    pub fn refused_destination(&self, output: &CommandOutput) -> Option<PathBuf> {
        match self {
            // 2026-08-06: cargo builds into a temporary directory beside the one it installs to
            // and moves the result over the old binary, naming both paths when that move is
            // denied. Observed on Windows 11 for claude-session.exe and tool-use-statistics.exe.
            DisplacingInvocation::InstallCargoCrate { .. } => {
                [&output.standard_error, &output.standard_output]
                    .into_iter()
                    .find_map(|text| destination_moved_to(text))
            }
        }
    }
}

impl WriteInvocation {
    pub fn tool(&self) -> Tool {
        match self {
            WriteInvocation::InstallWingetPackage { .. } => Tool::Winget,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplacingInvocation {
    ClaudeMcpServer { server: Box<ClaudeMcpServer> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementCommands {
    pub free_the_name: Vec<String>,
    pub claim_the_name: Vec<String>,
}

impl ReplacingInvocation {
    pub fn tool(&self) -> Tool {
        match self {
            ReplacingInvocation::ClaudeMcpServer { .. } => Tool::Claude,
        }
    }

    pub fn name(&self) -> &McpServerName {
        match self {
            ReplacingInvocation::ClaudeMcpServer { server } => &server.name,
        }
    }

    pub fn refused_claim(
        &self,
        the_name_was_freed: bool,
        cause: anyhow::Error,
    ) -> anyhow::Result<Replacement> {
        match the_name_was_freed {
            true => Ok(Replacement::RemovedButCouldNotAdd {
                name: self.name().clone(),
                cause,
            }),
            false => Err(cause),
        }
    }

    // 2026-09-13: `claude mcp add` refuses a name it already holds, reporting "MCP server <name>
    // already exists in local config", exiting 1 and leaving the registration unchanged, so the
    // name has to be freed before it can be claimed. `claude mcp remove` exits 0 when it removed
    // something and 1 when nothing was held under that name. Claude Code 2.1.270 on Windows 11.
    pub fn commands(&self) -> ReplacementCommands {
        match self {
            ReplacingInvocation::ClaudeMcpServer { server } => {
                let mut claim_the_name = vec![
                    "mcp".to_owned(),
                    "add".to_owned(),
                    "--scope".to_owned(),
                    server.scope.as_argument().to_owned(),
                    server.name.to_string(),
                ];
                for (key, value) in &server.environment {
                    claim_the_name.push("--env".to_owned());
                    claim_the_name.push(format!("{key}={value}"));
                }
                claim_the_name.push("--".to_owned());
                claim_the_name.push(server.command.clone());
                claim_the_name.extend(server.args.iter().cloned());

                ReplacementCommands {
                    free_the_name: vec![
                        "mcp".to_owned(),
                        "remove".to_owned(),
                        "--scope".to_owned(),
                        server.scope.as_argument().to_owned(),
                        server.name.to_string(),
                    ],
                    claim_the_name,
                }
            }
        }
    }
}

fn destination_moved_to(text: &str) -> Option<PathBuf> {
    let (_, refusal) = text.split_once("failed to move ")?;
    let mut quoted = refusal.split('`').skip(1).step_by(2);
    let _source = quoted.next()?;

    quoted.next().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{McpScope, RepositoryName, RepositoryOwner};
    use std::collections::BTreeMap;

    fn cargo_said(standard_error: &str) -> CommandOutput {
        CommandOutput {
            succeeded: false,
            standard_output: String::new(),
            standard_error: standard_error.to_owned(),
        }
    }

    fn installing_a_crate() -> DisplacingInvocation {
        installing("claude-session", ResolvedCargoSource::Registry)
    }

    fn installing(crate_name: &str, source: ResolvedCargoSource) -> DisplacingInvocation {
        DisplacingInvocation::InstallCargoCrate {
            crate_name: CrateName::from(crate_name),
            source,
        }
    }

    #[test]
    fn a_denied_move_is_read_as_the_destination_rather_than_the_file_it_was_built_into() {
        let output = cargo_said(
            "   Replacing C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe\n\
             error: failed to move `C:\\Users\\Alice\\.cargo\\bin\\cargo-installpDjhGc\\claude-session.exe` \
             to `C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe`\n\n\
             Caused by:\n  Access is denied. (os error 5)\n",
        );

        assert_eq!(
            installing_a_crate().refused_destination(&output),
            Some(PathBuf::from(
                "C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe"
            ))
        );
    }

    #[test]
    fn a_denied_move_whose_paths_wrapped_across_lines_still_names_its_destination() {
        let output = cargo_said(
            "error: failed to move `C:\\temp\\claude-session.exe`\n       \
             to `C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe`\n",
        );

        assert_eq!(
            installing_a_crate().refused_destination(&output),
            Some(PathBuf::from(
                "C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe"
            ))
        );
    }

    #[test]
    fn a_failure_that_names_no_path_it_could_not_write_is_not_recovered_from_by_displacing() {
        let output =
            cargo_said("error: could not compile `claude-session` (bin \"claude-session\")");

        assert_eq!(installing_a_crate().refused_destination(&output), None);
    }

    #[test]
    fn installing_a_crate_has_cargo_fetch_git_repositories_through_the_git_command_line() {
        assert_eq!(
            installing_a_crate().environment(),
            vec![("CARGO_NET_GIT_FETCH_WITH_CLI", "true")]
        );
    }

    #[test]
    fn a_crate_from_the_registry_is_installed_by_name_with_the_lockfile_it_publishes() {
        assert_eq!(
            installing("ripgrep", ResolvedCargoSource::Registry).arguments(),
            vec!["install", "--locked", "--force", "ripgrep"]
        );
    }

    #[test]
    fn a_crate_from_a_directory_is_installed_by_that_directory_rather_than_by_name() {
        let arguments = installing(
            "stop-gate",
            ResolvedCargoSource::Path {
                path: PathBuf::from("C:\\Repositories\\dotfiles\\tools\\stop-gate"),
            },
        )
        .arguments();

        assert_eq!(
            arguments,
            vec![
                "install",
                "--locked",
                "--force",
                "--path",
                "C:\\Repositories\\dotfiles\\tools\\stop-gate",
            ]
        );
    }

    #[test]
    fn a_crate_from_a_repository_is_installed_at_the_revision_read_from_the_clone() {
        let arguments = installing(
            "stop-gate",
            ResolvedCargoSource::Repository {
                repository: GitHubRepository {
                    owner: RepositoryOwner::from("Alice"),
                    repository: RepositoryName::from("dotfiles"),
                },
                account: GitHubAccount::from("Alice"),
                revision: Revision::from("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c"),
            },
        )
        .arguments();

        assert_eq!(
            arguments,
            vec![
                "install",
                "--locked",
                "--force",
                "--git",
                "https://Alice@github.com/Alice/dotfiles",
                "--rev",
                "2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c",
                "stop-gate",
            ]
        );
    }

    fn registering(server: ClaudeMcpServer) -> ReplacingInvocation {
        ReplacingInvocation::ClaudeMcpServer {
            server: Box::new(server),
        }
    }

    fn serena() -> ClaudeMcpServer {
        let mut environment = BTreeMap::new();
        environment.insert("SERENA_HOME".to_owned(), "C:\\dotfiles\\serena".to_owned());

        ClaudeMcpServer {
            name: McpServerName::from("serena"),
            scope: McpScope::User,
            command: "serena".to_owned(),
            args: vec!["start-mcp-server".to_owned()],
            environment,
        }
    }

    #[test]
    fn registering_a_server_passes_its_environment_before_the_command() {
        assert_eq!(
            registering(serena()).commands().claim_the_name,
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
    fn registering_a_server_frees_its_name_in_the_scope_the_new_registration_claims() {
        assert_eq!(
            registering(serena()).commands().free_the_name,
            vec!["mcp", "remove", "--scope", "user", "serena"]
        );
    }

    #[test]
    fn a_refused_claim_on_a_name_something_was_removed_from_leaves_that_name_holding_nothing() {
        let outcome = registering(serena())
            .refused_claim(true, anyhow::anyhow!("claude refused"))
            .expect("a half-applied replacement is an outcome rather than an error");

        match outcome {
            Replacement::RemovedButCouldNotAdd { name, .. } => {
                assert_eq!(name, McpServerName::from("serena"));
            }
            Replacement::Replaced => panic!("a refused claim did not replace anything"),
        }
    }

    #[test]
    fn a_refused_claim_on_a_name_nothing_was_removed_from_took_nothing_away_to_report() {
        let error = registering(serena())
            .refused_claim(false, anyhow::anyhow!("claude refused"))
            .expect_err("a refusal that changed nothing is an error");

        assert_eq!(error.to_string(), "claude refused");
    }
}

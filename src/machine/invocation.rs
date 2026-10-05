use {
    crate::{
        configuration::{
            ClaudeMcpServer, CrateName, CrateVersion, GitHubAccount, GitHubRepository,
            McpServerName, PythonInterpreter, Tool, UvToolName, WingetPackageId,
        },
        machine::{
            CommandOutput, Replacement,
            workspace_reading::{Revision, WorkspaceReading},
        },
    },
    std::{
        collections::BTreeSet,
        fmt::Display,
        path::{Path, PathBuf},
    },
};

// ADR 0006
// ADR 0010
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReadInvocation {
    WingetInstalledPackages,
    WingetPackage { id: WingetPackageId },
    CargoInstalledCrates,
    ClaudeMcpServer { name: McpServerName },
    UvInstalledTools,
    UvOutdatedTools,
}

impl ReadInvocation {
    pub fn tool(&self) -> Tool {
        match self {
            ReadInvocation::WingetInstalledPackages | ReadInvocation::WingetPackage { .. } => {
                Tool::Winget
            }
            ReadInvocation::CargoInstalledCrates => Tool::Cargo,
            ReadInvocation::ClaudeMcpServer { .. } => Tool::Claude,
            ReadInvocation::UvInstalledTools | ReadInvocation::UvOutdatedTools => Tool::Uv,
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
            // 2026-09-26: the whole listing showed Android Studio only as `ARP\Machine\X64\Android
            // Studio`, while this form matched it to `Google.AndroidStudio`. winget v1.29.380 on
            // Windows 11.
            ReadInvocation::WingetPackage { id } => vec![
                "list".to_owned(),
                "--id".to_owned(),
                id.to_string(),
                "--exact".to_owned(),
                "--accept-source-agreements".to_owned(),
                "--disable-interactivity".to_owned(),
            ],
            ReadInvocation::CargoInstalledCrates => {
                vec!["install".to_owned(), "--list".to_owned()]
            }
            ReadInvocation::ClaudeMcpServer { name } => {
                vec!["mcp".to_owned(), "get".to_owned(), name.to_string()]
            }
            // 2026-09-25: `uv tool list` prints each tool as `name vX.Y.Z` with its executables
            // indented beneath as `- name` lines. uv 0.10.12 on Windows 11.
            ReadInvocation::UvInstalledTools => vec!["tool".to_owned(), "list".to_owned()],
            // 2026-09-25: `--outdated` lists only the tools a newer version resolves for, as
            // `name vX.Y.Z [latest: A.B.C]`, and exits non-zero when the index cannot be reached.
            // Offline — `--offline` or `UV_OFFLINE` — it silently leaves out every tool it cannot
            // look up and exits 0, and `--no-offline` overrides the environment. uv 0.10.12 on
            // Windows 11.
            ReadInvocation::UvOutdatedTools => vec![
                "tool".to_owned(),
                "list".to_owned(),
                "--outdated".to_owned(),
                "--no-offline".to_owned(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteInvocation {
    UpdateWingetSources,
    InstallWingetPackage {
        id: WingetPackageId,
    },
    InstallUvTool {
        name: UvToolName,
        python: Option<PythonInterpreter>,
    },
    UpgradeUvTool {
        name: UvToolName,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCargoSource {
    Registry {
        version: Option<CrateVersion>,
    },
    Path {
        path: PathBuf,
    },
    Repository {
        repository: GitHubRepository,
        account: GitHubAccount,
        revision: Revision,
    },
}

// ADR 0022
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
                source: ResolvedCargoSource::Registry { version },
            } => {
                let mut arguments = vec![
                    "install".to_owned(),
                    "--locked".to_owned(),
                    "--force".to_owned(),
                ];
                if let Some(version) = version {
                    arguments.extend(["--version".to_owned(), version.to_string()]);
                }
                arguments.push(crate_name.to_string());
                arguments
            }
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

    pub fn build_directory(&self, build_cache: &Path) -> Option<PathBuf> {
        match self {
            DisplacingInvocation::InstallCargoCrate {
                source: ResolvedCargoSource::Repository { revision, .. },
                ..
            } => Some(revision.build_directory_in(build_cache)),
            DisplacingInvocation::InstallCargoCrate { .. } => None,
        }
    }

    pub fn environment(&self, build_cache: &Path) -> Vec<(String, String)> {
        match self {
            DisplacingInvocation::InstallCargoCrate { .. } => {
                cargo_environment(self.build_directory(build_cache).as_deref())
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceBuild<'reading> {
    clone_directory: PathBuf,
    reading: &'reading WorkspaceReading,
    members: BTreeSet<CrateName>,
}

impl<'reading> WorkspaceBuild<'reading> {
    pub fn of(
        clone_directory: PathBuf,
        reading: &'reading WorkspaceReading,
        changed: impl IntoIterator<Item = CrateName>,
    ) -> Option<Self> {
        let members: BTreeSet<CrateName> = changed
            .into_iter()
            .filter(|member| reading.members.contains_key(member))
            .collect();
        if members.is_empty() {
            return None;
        }

        Some(Self {
            clone_directory,
            reading,
            members,
        })
    }

    pub fn clone_directory(&self) -> &Path {
        &self.clone_directory
    }

    pub fn revision(&self) -> &Revision {
        &self.reading.revision
    }

    pub fn members(&self) -> &BTreeSet<CrateName> {
        &self.members
    }

    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "build".to_owned(),
            "--release".to_owned(),
            "--locked".to_owned(),
            "--keep-going".to_owned(),
        ];
        for member in &self.members {
            arguments.extend(["-p".to_owned(), member.to_string()]);
        }
        arguments
    }

    pub fn build_directory(&self, build_cache: &Path) -> PathBuf {
        self.revision().build_directory_in(build_cache)
    }

    pub fn source_directory(&self, build_cache: &Path) -> PathBuf {
        build_cache.join(format!("{}-source", self.revision()))
    }

    pub fn environment(&self, build_cache: &Path) -> Vec<(String, String)> {
        cargo_environment(Some(&self.build_directory(build_cache)))
    }
}

impl Display for WorkspaceBuild<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let members: Vec<String> = self.members.iter().map(CrateName::to_string).collect();
        write!(
            formatter,
            "{} at {} in {}",
            members.join(", "),
            self.revision(),
            self.clone_directory.display()
        )
    }
}

fn cargo_environment(build_directory: Option<&Path>) -> Vec<(String, String)> {
    // 2026-08-10: cargo's own libgit2 fetch cannot authenticate to a private GitHub repository on
    // a machine holding its credentials behind `gh auth git-credential`, failing with "no
    // authentication methods succeeded" against a cold cache. The git command line runs that
    // helper and fetches the same revision.
    let mut environment = vec![("CARGO_NET_GIT_FETCH_WITH_CLI".to_owned(), "true".to_owned())];

    if let Some(directory) = build_directory {
        environment.push((
            "CARGO_TARGET_DIR".to_owned(),
            directory.display().to_string(),
        ));
    }

    environment
}

impl WriteInvocation {
    pub fn tool(&self) -> Tool {
        match self {
            WriteInvocation::UpdateWingetSources | WriteInvocation::InstallWingetPackage { .. } => {
                Tool::Winget
            }
            WriteInvocation::InstallUvTool { .. } | WriteInvocation::UpgradeUvTool { .. } => {
                Tool::Uv
            }
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        match self {
            WriteInvocation::UpdateWingetSources => vec!["source".to_owned(), "update".to_owned()],
            WriteInvocation::InstallWingetPackage { id } => vec![
                "install".to_owned(),
                "--exact".to_owned(),
                "--id".to_owned(),
                id.to_string(),
                "--accept-package-agreements".to_owned(),
                "--accept-source-agreements".to_owned(),
                "--disable-interactivity".to_owned(),
            ],
            WriteInvocation::InstallUvTool { name, python } => {
                let mut arguments = vec!["tool".to_owned(), "install".to_owned()];
                if let Some(python) = python {
                    arguments.push("--python".to_owned());
                    arguments.push(python.to_string());
                }
                arguments.push(name.to_string());
                arguments
            }
            WriteInvocation::UpgradeUvTool { name } => {
                vec!["tool".to_owned(), "upgrade".to_owned(), name.to_string()]
            }
        }
    }

    // 2026-09-25: uv upgrades a tool's environment and then copies each executable from the
    // environment into its bin directory. A copy over an executable that is running fails with
    // os error 32, naming both paths, after the environment has already been upgraded; the
    // running launcher cannot be renamed aside either. uv 0.10.12 on Windows 11.
    pub fn refused_copy(&self, output: &CommandOutput) -> Option<RefusedCopy> {
        match self {
            WriteInvocation::UpdateWingetSources | WriteInvocation::InstallWingetPackage { .. } => {
                None
            }
            WriteInvocation::InstallUvTool { .. } | WriteInvocation::UpgradeUvTool { .. } => output
                .standard_error
                .lines()
                .find_map(copy_refused_by_a_running_image),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedCopy {
    pub source: PathBuf,
    pub destination: PathBuf,
}

const COPY_REFUSAL: &str = "failed to copy file from ";
const SHARING_VIOLATION: &str = "(os error 32)";

fn copy_refused_by_a_running_image(line: &str) -> Option<RefusedCopy> {
    if !line.trim_end().ends_with(SHARING_VIOLATION) {
        return None;
    }
    let (_, refusal) = line.split_once(COPY_REFUSAL)?;
    let (source, remainder) = refusal.split_once(" to ")?;
    let (destination, _) = remainder.split_once(": ")?;

    Some(RefusedCopy {
        source: PathBuf::from(source),
        destination: PathBuf::from(destination),
    })
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
    use crate::{
        configuration::{McpScope, RepositoryName, RepositoryOwner},
        machine::workspace_reading::{Fingerprint, InstalledState, MemberReading, ObjectHash},
    };
    use std::collections::BTreeMap;

    fn cargo_said(standard_error: &str) -> CommandOutput {
        CommandOutput {
            succeeded: false,
            standard_output: String::new(),
            standard_error: standard_error.to_owned(),
        }
    }

    fn installing_a_crate() -> DisplacingInvocation {
        installing(
            "claude-session",
            ResolvedCargoSource::Registry { version: None },
        )
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

    fn build_cache() -> PathBuf {
        PathBuf::from("C:\\Users\\Alice\\.dotfiles_configurator\\build-cache")
    }

    fn installing_from_revision(revision: &str) -> DisplacingInvocation {
        installing(
            "stop-gate",
            ResolvedCargoSource::Repository {
                repository: GitHubRepository {
                    owner: RepositoryOwner::from("Alice"),
                    repository: RepositoryName::from("dotfiles"),
                },
                account: GitHubAccount::from("Alice"),
                revision: Revision::from(revision),
            },
        )
    }

    fn value_of<'environment>(
        environment: &'environment [(String, String)],
        name: &str,
    ) -> Option<&'environment str> {
        environment
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn installing_a_crate_has_cargo_fetch_git_repositories_through_the_git_command_line() {
        assert_eq!(
            value_of(
                &installing_a_crate().environment(&build_cache()),
                "CARGO_NET_GIT_FETCH_WITH_CLI"
            ),
            Some("true")
        );
    }

    #[test]
    fn every_crate_of_one_revision_builds_in_the_directory_that_revision_names() {
        let directory = installing_from_revision("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c")
            .build_directory(&build_cache());

        assert_eq!(
            directory,
            Some(build_cache().join("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c"))
        );
    }

    #[test]
    fn two_revisions_of_one_repository_never_build_in_the_same_directory() {
        let earlier = installing_from_revision("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c")
            .build_directory(&build_cache());
        let later = installing_from_revision("f9a32f6c605fc1ed4584037a770376d084f74a8d")
            .build_directory(&build_cache());

        assert_ne!(earlier, later);
    }

    #[test]
    fn a_crate_from_a_repository_has_cargo_build_it_in_the_directory_its_revision_names() {
        let invocation = installing_from_revision("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c");
        let environment = invocation.environment(&build_cache());

        assert_eq!(
            value_of(&environment, "CARGO_TARGET_DIR"),
            Some(
                build_cache()
                    .join("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c")
                    .display()
                    .to_string()
                    .as_str()
            )
        );
    }

    #[test]
    fn a_crate_from_the_registry_is_left_to_build_where_cargo_would_put_it() {
        let invocation = installing("ripgrep", ResolvedCargoSource::Registry { version: None });

        assert_eq!(invocation.build_directory(&build_cache()), None);
        assert_eq!(
            value_of(&invocation.environment(&build_cache()), "CARGO_TARGET_DIR"),
            None
        );
    }

    #[test]
    fn a_crate_from_a_path_is_left_to_build_where_cargo_would_put_it() {
        let invocation = installing(
            "stop-gate",
            ResolvedCargoSource::Path {
                path: PathBuf::from("C:\\Repositories\\dotfiles\\tools\\stop-gate"),
            },
        );

        assert_eq!(invocation.build_directory(&build_cache()), None);
    }

    #[test]
    fn a_crate_from_the_registry_is_installed_by_name_with_the_lockfile_it_publishes() {
        assert_eq!(
            installing("ripgrep", ResolvedCargoSource::Registry { version: None }).arguments(),
            vec!["install", "--locked", "--force", "ripgrep"]
        );
    }

    #[test]
    fn a_crate_pinned_to_a_version_is_installed_at_exactly_that_version() {
        let arguments = installing(
            "cargo-mutants",
            ResolvedCargoSource::Registry {
                version: Some(CrateVersion::try_from("27.1.0").unwrap()),
            },
        )
        .arguments();

        assert_eq!(
            arguments,
            vec![
                "install",
                "--locked",
                "--force",
                "--version",
                "27.1.0",
                "cargo-mutants"
            ]
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

    fn workspace_at(revision: &str, members: &[&str]) -> WorkspaceReading {
        WorkspaceReading {
            revision: Revision::from(revision),
            members: members
                .iter()
                .map(|name| {
                    let fingerprint = Fingerprint {
                        crate_subtree: ObjectHash::from("aaa"),
                        workspace_manifest: ObjectHash::from("bbb"),
                        lock_closure: ObjectHash::from("ccc"),
                        dependency_subtrees: BTreeMap::new(),
                    };
                    (
                        CrateName::from(*name),
                        MemberReading {
                            desired: fingerprint.clone(),
                            installed: InstalledState::At(fingerprint),
                            absent_binaries: BTreeSet::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn changed(members: &[&str]) -> Vec<CrateName> {
        members.iter().map(|name| CrateName::from(*name)).collect()
    }

    fn clone_directory() -> PathBuf {
        PathBuf::from("C:\\Repositories\\Alice\\dotfiles")
    }

    #[test]
    fn a_workspace_build_names_each_changed_member_and_keeps_going_past_one_that_fails() {
        let reading = workspace_at(
            "2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c",
            &["stop-gate", "live-set", "session-mining"],
        );

        let build = WorkspaceBuild::of(
            clone_directory(),
            &reading,
            changed(&["stop-gate", "live-set"]),
        )
        .expect("two members to build");

        assert_eq!(
            build.arguments(),
            vec![
                "build",
                "--release",
                "--locked",
                "--keep-going",
                "-p",
                "live-set",
                "-p",
                "stop-gate",
            ]
        );
    }

    #[test]
    fn a_workspace_build_leaves_out_a_crate_its_workspace_does_not_hold() {
        let reading = workspace_at("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c", &["stop-gate"]);

        let build = WorkspaceBuild::of(
            clone_directory(),
            &reading,
            changed(&["stop-gate", "ripgrep"]),
        )
        .expect("one member to build");

        assert_eq!(
            build.members(),
            &BTreeSet::from([CrateName::from("stop-gate")])
        );
    }

    #[test]
    fn a_workspace_with_no_member_to_change_has_nothing_to_build() {
        let reading = workspace_at("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c", &["stop-gate"]);

        assert_eq!(
            WorkspaceBuild::of(clone_directory(), &reading, changed(&["ripgrep"])),
            None
        );
    }

    #[test]
    fn a_workspace_build_targets_the_directory_its_revisions_installs_build_in() {
        let reading = workspace_at("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c", &["stop-gate"]);
        let build = WorkspaceBuild::of(clone_directory(), &reading, changed(&["stop-gate"]))
            .expect("one member to build");
        let install = installing_from_revision("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c");

        let built_in = build.environment(&build_cache());
        let installed_from = install.environment(&build_cache());

        assert_eq!(
            value_of(&built_in, "CARGO_TARGET_DIR"),
            value_of(&installed_from, "CARGO_TARGET_DIR")
        );
    }

    #[test]
    fn a_workspace_build_checks_its_revision_out_beside_rather_than_inside_its_build_directory() {
        let reading = workspace_at("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c", &["stop-gate"]);
        let build = WorkspaceBuild::of(clone_directory(), &reading, changed(&["stop-gate"]))
            .expect("one member to build");

        let source = build.source_directory(&build_cache());

        assert!(!source.starts_with(build.build_directory(&build_cache())));
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

    #[test]
    fn a_uv_tool_declaring_an_interpreter_is_installed_into_an_environment_built_with_it() {
        let installing = WriteInvocation::InstallUvTool {
            name: UvToolName::from("serena-agent"),
            python: Some(PythonInterpreter::from("3.13")),
        };

        assert_eq!(
            installing.arguments(),
            vec!["tool", "install", "--python", "3.13", "serena-agent"]
        );
    }

    #[test]
    fn a_uv_tool_declaring_no_interpreter_leaves_the_choice_of_one_to_uv() {
        let installing = WriteInvocation::InstallUvTool {
            name: UvToolName::from("serena-agent"),
            python: None,
        };

        assert_eq!(
            installing.arguments(),
            vec!["tool", "install", "serena-agent"]
        );
    }

    fn upgrading_serena() -> WriteInvocation {
        WriteInvocation::UpgradeUvTool {
            name: UvToolName::from("serena-agent"),
        }
    }

    // 2026-09-25: taken verbatim from `uv tool upgrade serena-agent` with the tool's launcher
    // running, under uv 0.10.12 on Windows 11.
    const UV_REFUSED_A_RUNNING_LAUNCHER: &str = concat!(
        "error: Failed to upgrade serena-agent\n",
        "  Caused by: Failed to install entrypoint\n",
        "  Caused by: failed to copy file from ",
        "C:\\t\\uvprobe\\tools\\serena-agent\\Scripts\\serena.exe to ",
        "C:/t/uvprobe/bin\\serena.exe: The process cannot access the file because it is being ",
        "used by another process. (os error 32)\n",
    );

    #[test]
    fn a_copy_refused_over_a_running_launcher_names_both_the_copy_and_what_it_would_replace() {
        assert_eq!(
            upgrading_serena().refused_copy(&cargo_said(UV_REFUSED_A_RUNNING_LAUNCHER)),
            Some(RefusedCopy {
                source: PathBuf::from("C:\\t\\uvprobe\\tools\\serena-agent\\Scripts\\serena.exe"),
                destination: PathBuf::from("C:/t/uvprobe/bin\\serena.exe"),
            })
        );
    }

    #[test]
    fn a_copy_refused_for_any_other_reason_is_not_read_as_a_running_launcher() {
        let output = cargo_said(
            "  Caused by: failed to copy file from C:\\env\\serena.exe to C:\\bin\\serena.exe: \
             Access is denied. (os error 5)\n",
        );

        assert_eq!(upgrading_serena().refused_copy(&output), None);
    }

    #[test]
    fn the_tools_uv_reports_as_behind_are_asked_for_even_where_uv_is_set_to_work_offline() {
        assert!(
            ReadInvocation::UvOutdatedTools
                .arguments()
                .contains(&"--no-offline".to_owned())
        );
    }
}

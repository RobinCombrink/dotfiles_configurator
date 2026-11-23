use {
    crate::{
        configuration::{
            ClaudeMcpServer, CrateName, CrateVersion, DartPackage, DartSource, DotnetToolId,
            DotnetToolVersion, GitHubAccount, GitHubRepository, McpServerName, PythonInterpreter,
            Tool, UvToolName, UvToolVersion, WingetPackageId, WingetVersion,
        },
        machine::{
            CommandOutput, Exited, HeldReason, Replacement,
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
    WingetUpgrades,
    CargoInstalledCrates,
    ClaudeMcpServer { name: McpServerName },
    UvInstalledTools,
    UvOutdatedTools,
    DotnetInstalledTools,
    DotnetToolSearch { id: DotnetToolId },
}

impl ReadInvocation {
    pub fn tool(&self) -> Tool {
        match self {
            ReadInvocation::WingetInstalledPackages
            | ReadInvocation::WingetPackage { .. }
            | ReadInvocation::WingetUpgrades => Tool::Winget,
            ReadInvocation::CargoInstalledCrates => Tool::Cargo,
            ReadInvocation::ClaudeMcpServer { .. } => Tool::Claude,
            ReadInvocation::UvInstalledTools | ReadInvocation::UvOutdatedTools => Tool::Uv,
            ReadInvocation::DotnetInstalledTools | ReadInvocation::DotnetToolSearch { .. } => {
                Tool::Dotnet
            }
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
            // 2026-10-06: lists each installed package winget offers a newer version of, under
            // the columns Name, Id, Version, Available and Source, and closes on a count of them.
            // A package whose installed version winget cannot determine, or one pinned in winget,
            // is left out. winget v1.29.380 on Windows 11.
            ReadInvocation::WingetUpgrades => vec![
                "upgrade".to_owned(),
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
            // 2026-10-07: `--show-version-specifiers` appends `[required: ==24.1.0]` to a tool
            // installed as `black==24.1.0` and nothing to one installed as `black@latest`. uv
            // 0.10.12 on Windows 11.
            ReadInvocation::UvInstalledTools => vec![
                "tool".to_owned(),
                "list".to_owned(),
                "--show-version-specifiers".to_owned(),
            ],
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
            // 2026-11-23: prints a heading row, a row of dashes, and one row per global tool
            // opening on its package id and the version it is installed at. .NET SDK 10.0.102 on
            // Windows 11.
            ReadInvocation::DotnetInstalledTools => {
                vec!["tool".to_owned(), "list".to_owned(), "--global".to_owned()]
            }
            // 2026-11-23: matches the term against ids and descriptions alike, so searching
            // csharp-ls also listed csharp-ls-vs and csharplspmcp, each in a row opening on its
            // package id and the newest stable version NuGet publishes; with nothing matching it
            // prints "Could not find any results." and exits 0. .NET SDK 10.0.102 on Windows 11.
            ReadInvocation::DotnetToolSearch { id } => {
                vec!["tool".to_owned(), "search".to_owned(), id.to_string()]
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteInvocation {
    UpdateWingetSources,
    InstallWingetPackage {
        id: WingetPackageId,
        version: Option<WingetVersion>,
    },
    UpgradeWingetPackage {
        id: WingetPackageId,
    },
    InstallUvTool {
        name: UvToolName,
        python: Option<PythonInterpreter>,
        version: Option<UvToolVersion>,
    },
    UpgradeUvTool {
        name: UvToolName,
    },
    ReinstallUvToolAtLatest {
        name: UvToolName,
        python: Option<PythonInterpreter>,
    },
    InstallDartPackage {
        package: Box<DartPackage>,
    },
    UpdateDotnetTool {
        id: DotnetToolId,
        version: Option<DotnetToolVersion>,
    },
}

pub fn without_terminal_prompts() -> Vec<(String, String)> {
    vec![("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned())]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCargoSource {
    Registry { version: Option<CrateVersion> },
    Path { path: PathBuf },
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
        }
    }

    pub fn environment(&self) -> Vec<(String, String)> {
        match self {
            DisplacingInvocation::InstallCargoCrate { .. } => cargo_environment(None),
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
pub struct MemberInstall {
    pub crate_name: CrateName,
    pub repository: GitHubRepository,
    pub account: GitHubAccount,
    pub revision: Revision,
}

impl MemberInstall {
    pub fn staging_directory(&self, build_cache: &Path) -> PathBuf {
        build_cache.join(format!("{}-staging-{}", self.revision, self.crate_name))
    }

    pub fn staged_binaries_directory(&self, build_cache: &Path) -> PathBuf {
        self.staging_directory(build_cache).join("bin")
    }

    pub fn arguments(&self, build_cache: &Path) -> Vec<String> {
        vec![
            "install".to_owned(),
            "--locked".to_owned(),
            "--no-track".to_owned(),
            "--root".to_owned(),
            self.staging_directory(build_cache).display().to_string(),
            "--git".to_owned(),
            self.repository.fetch_url_as(&self.account),
            "--rev".to_owned(),
            self.revision.to_string(),
            self.crate_name.to_string(),
        ]
    }

    pub fn environment(&self, build_cache: &Path) -> Vec<(String, String)> {
        cargo_environment(Some(&self.revision.build_directory_in(build_cache)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordUntracking {
    pub specification: String,
    pub binary_files: Vec<String>,
}

impl RecordUntracking {
    pub fn arguments(&self) -> Vec<String> {
        vec!["uninstall".to_owned(), self.specification.clone()]
    }

    pub fn environment(&self) -> Vec<(String, String)> {
        cargo_environment(None)
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
        let members: BTreeSet<CrateName> = changed.into_iter().collect();
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
            WriteInvocation::UpdateWingetSources
            | WriteInvocation::InstallWingetPackage { .. }
            | WriteInvocation::UpgradeWingetPackage { .. } => Tool::Winget,
            WriteInvocation::InstallUvTool { .. }
            | WriteInvocation::UpgradeUvTool { .. }
            | WriteInvocation::ReinstallUvToolAtLatest { .. } => Tool::Uv,
            WriteInvocation::InstallDartPackage { .. } => Tool::Dart,
            WriteInvocation::UpdateDotnetTool { .. } => Tool::Dotnet,
        }
    }

    pub fn environment(&self) -> Vec<(String, String)> {
        match self {
            WriteInvocation::InstallDartPackage { .. } => without_terminal_prompts(),
            WriteInvocation::UpdateWingetSources
            | WriteInvocation::InstallWingetPackage { .. }
            | WriteInvocation::UpgradeWingetPackage { .. }
            | WriteInvocation::InstallUvTool { .. }
            | WriteInvocation::UpgradeUvTool { .. }
            | WriteInvocation::ReinstallUvToolAtLatest { .. }
            | WriteInvocation::UpdateDotnetTool { .. } => Vec::new(),
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        match self {
            WriteInvocation::UpdateWingetSources => vec!["source".to_owned(), "update".to_owned()],
            WriteInvocation::InstallWingetPackage { id, version } => {
                let mut arguments = vec![
                    "install".to_owned(),
                    "--exact".to_owned(),
                    "--id".to_owned(),
                    id.to_string(),
                ];
                if let Some(version) = version {
                    arguments.extend([
                        "--version".to_owned(),
                        version.to_string(),
                        "--force".to_owned(),
                    ]);
                }
                arguments.extend([
                    "--accept-package-agreements".to_owned(),
                    "--accept-source-agreements".to_owned(),
                    "--disable-interactivity".to_owned(),
                ]);
                arguments
            }
            WriteInvocation::UpgradeWingetPackage { id } => vec![
                "upgrade".to_owned(),
                "--id".to_owned(),
                id.to_string(),
                "--exact".to_owned(),
                "--silent".to_owned(),
                "--disable-interactivity".to_owned(),
                "--accept-package-agreements".to_owned(),
            ],
            // 2026-10-06: `uv tool install cowsay==6.0` over cowsay 6.1, and `cowsay==6.1` over
            // 6.0, each replaced the installed version and exited 0. uv 0.10.12 on Windows 11.
            WriteInvocation::InstallUvTool {
                name,
                python,
                version,
            } => {
                let mut arguments = vec!["tool".to_owned(), "install".to_owned()];
                if let Some(python) = python {
                    arguments.push("--python".to_owned());
                    arguments.push(python.to_string());
                }
                arguments.push(match version {
                    None => name.to_string(),
                    Some(version) => format!("{name}=={version}"),
                });
                arguments
            }
            WriteInvocation::UpgradeUvTool { name } => {
                vec!["tool".to_owned(), "upgrade".to_owned(), name.to_string()]
            }
            // 2026-10-07: `uv tool install black@latest` over `black==24.1.0` installed the
            // newest black and dropped the pin, keeping the interpreter the environment was built
            // with, where `uv tool upgrade` answered "Nothing to upgrade". uv 0.10.12 on Windows 11.
            WriteInvocation::ReinstallUvToolAtLatest { name, python } => {
                let mut arguments = vec!["tool".to_owned(), "install".to_owned()];
                if let Some(python) = python {
                    arguments.extend(["--python".to_owned(), python.to_string()]);
                }
                arguments.push(format!("{name}@latest"));
                arguments
            }
            WriteInvocation::InstallDartPackage { package } => {
                let DartSource::Git {
                    url,
                    path,
                    reference,
                } = &package.source;
                vec![
                    "install".to_owned(),
                    format!(
                        "{}@{{git: {{url: {url}, path: {path}, ref: {reference}}}}}",
                        package.name
                    ),
                    "--overwrite".to_owned(),
                ]
            }
            // 2026-11-23: `dotnet tool update` installed a tool it did not hold, and moved one it
            // held up to the newest version or to the one `--version` named; a lower version than
            // the one installed it refused with exit 1 unless `--allow-downgrade` was passed. .NET
            // SDK 10.0.102 on Windows 11.
            WriteInvocation::UpdateDotnetTool { id, version } => {
                let mut arguments = vec![
                    "tool".to_owned(),
                    "update".to_owned(),
                    "--global".to_owned(),
                    id.to_string(),
                ];
                if let Some(version) = version {
                    arguments.extend([
                        "--version".to_owned(),
                        version.to_string(),
                        "--allow-downgrade".to_owned(),
                    ]);
                }
                arguments
            }
        }
    }

    // 2026-09-25: uv upgrades a tool's environment and then copies each executable from the
    // environment into its bin directory. A copy over an executable that is running fails with
    // os error 32, naming both paths, after the environment has already been upgraded; the
    // running launcher cannot be renamed aside either. uv 0.10.12 on Windows 11.
    pub fn refused_copy(&self, output: &CommandOutput) -> Option<RefusedCopy> {
        match self {
            WriteInvocation::UpdateWingetSources
            | WriteInvocation::InstallWingetPackage { .. }
            | WriteInvocation::UpgradeWingetPackage { .. }
            | WriteInvocation::InstallDartPackage { .. }
            | WriteInvocation::UpdateDotnetTool { .. } => None,
            WriteInvocation::InstallUvTool { .. }
            | WriteInvocation::UpgradeUvTool { .. }
            | WriteInvocation::ReinstallUvToolAtLatest { .. } => output
                .standard_error
                .lines()
                .find_map(copy_refused_by_a_running_image),
        }
    }

    pub fn held_by(&self, output: &CommandOutput) -> Option<HeldReason> {
        match self {
            WriteInvocation::InstallWingetPackage { .. }
            | WriteInvocation::UpgradeWingetPackage { .. } => winget_held_by(output),
            WriteInvocation::InstallDartPackage { .. } => {
                [&output.standard_error, &output.standard_output]
                    .into_iter()
                    .flat_map(|said| said.lines())
                    .find(|line| line.contains(DART_BUNDLE_IN_USE))
                    .map(|line| HeldReason::ReportedInUse(line.trim().to_owned()))
            }
            WriteInvocation::ReinstallUvToolAtLatest { .. } => output
                .standard_error
                .lines()
                .find(|line| line.contains(UV_ENVIRONMENT_IN_USE) && line.contains(ACCESS_DENIED))
                .map(|line| HeldReason::ReportedInUse(line.trim().to_owned())),
            WriteInvocation::UpdateWingetSources
            | WriteInvocation::InstallUvTool { .. }
            | WriteInvocation::UpgradeUvTool { .. }
            | WriteInvocation::UpdateDotnetTool { .. } => None,
        }
    }
}

// 2026-10-06: `winget upgrade --id mvdan.shfmt --exact --silent` with shfmt.exe running exited
// 0x8A150052, printing `remove: Access is denied.: "<path of shfmt.exe>"`, and left the running
// process and the installed version as they were; the same upgrade with nothing running exited 0.
// winget v1.29.380 on Windows 11.
const WINGET_PORTABLE_INSTALL_FAILED: i32 = 0x8A15_0052_u32.cast_signed();
const ACCESS_DENIED: &str = "Access is denied";

// 2026-10-06: `winget error` names 0x8A150101 APPINSTALLER_CLI_ERROR_INSTALL_PACKAGE_IN_USE and
// 0x8A150111 APPINSTALLER_CLI_ERROR_INSTALL_PACKAGE_IN_USE_BY_APPLICATION. winget v1.29.380.
const WINGET_PACKAGE_IN_USE: [i32; 2] =
    [0x8A15_0101_u32.cast_signed(), 0x8A15_0111_u32.cast_signed()];

const WINGET_REFUSES_TO_UPGRADE: &str = "cannot be upgraded using winget";

// 2026-10-07: `dart install dhttpd --overwrite` with dhttpd.exe running exited 255, printing
// "Deletion failed. The application might be in use." on its standard error after it had already
// deleted the package's bin shim. Dart 3.11.1 on Windows 11.
const DART_BUNDLE_IN_USE: &str = "The application might be in use.";

// 2026-10-07: `uv tool install black@latest` with black.exe running exited 2, printing "error:
// failed to remove directory `<tool directory>\black\Scripts`: Access is denied. (os error 5)",
// after which `uv tool list` could not find black in its environment until a later install with
// nothing running restored it. uv 0.10.12 on Windows 11.
const UV_ENVIRONMENT_IN_USE: &str = "failed to remove directory";

fn winget_held_by(output: &CommandOutput) -> Option<HeldReason> {
    let said = format!("{}\n{}", output.standard_output, output.standard_error);
    if said.to_lowercase().contains(WINGET_REFUSES_TO_UPGRADE) {
        return Some(HeldReason::UpgradedByItsPublisher);
    }

    let Exited::Code(code) = output.exited else {
        return None;
    };
    if WINGET_PACKAGE_IN_USE.contains(&code) {
        return Some(HeldReason::ReportedInUse(format!(
            "winget {}: {}",
            output.exited,
            last_line_of(&said)
        )));
    }
    if code == WINGET_PORTABLE_INSTALL_FAILED {
        return said
            .lines()
            .find(|line| line.contains(ACCESS_DENIED))
            .map(|line| HeldReason::ReportedInUse(line.trim().to_owned()));
    }
    None
}

fn last_line_of(said: &str) -> &str {
    said.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
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
mod tests;

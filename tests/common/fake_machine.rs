// Each integration test file is its own crate and pulls this module in whole, so a helper only
// one of them needs reads as dead in the others.
#![allow(dead_code)]

use {
    anyhow::{Result, anyhow, bail},
    dotfiles_configurator::{
        configuration::{
            Answer, ApplicationName, ApplicationSource, BinaryName, ClaudeMcpServer, CloneDepth,
            CrateName, CrateVersion, DartPackage, DartPackageName, GitHubAccount, GitHubRepository,
            Installer, MachineClass, MachineManifest, McpServerName, Migration, PresenceCheck,
            PythonInterpreter, ReleasedBinary, RepositoryClone, Shell, Tool, UvToolName,
            UvToolVersion, VariableName, VariableValue, WingetPackageId, WingetVersion,
        },
        configuration_source::{EnvironmentFailure, WriteSource},
        currency::{own_currency, own_release_asset_name, own_release_repository},
        machine::{
            Batched, CommandOutput, ContentDigest, DisplacingInvocation, Downloaded, ElevatedBatch,
            ElevatedOutcome, ElevatedWork, Elevation, Exited, FileReadFailure, HeldReason,
            MemberInstall, OsError, Placement, PrivilegeRefusal, ProgramStartFailure,
            ReadInvocation, ReadMachine, RecordUntracking, Replacement, ReplacingInvocation,
            ResolvedCargoSource, VersionReportFailure, WorkspaceBuild, WriteInvocation,
            WriteMachine,
            clone_reading::CloneReadFailure,
            crate_index_reading::CrateIndexReadFailure,
            dart_reading::{DartDrift, DartReadFailure, DartReading},
            environment_reading::{
                MachineSearchPath, RegistryReadFailure, SearchPathPlace, SearchPathReading,
            },
            release_reading::{ReleaseAsset, ReleaseReadFailure, ReleaseReading},
            superseded_name,
            workspace_reading::{Fingerprint, Revision, WorkspaceReadFailure, WorkspaceReading},
        },
        tool_directory_within,
        version::Version,
    },
    github_authentication::cli,
    reqwest::StatusCode,
    std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
        sync::Arc,
    },
    url::Url,
};

#[cfg(target_family = "windows")]
use dotfiles_configurator::machine::environment_reading::RegistryKey;

#[derive(Debug)]
pub struct FakeMachine {
    home_directory: PathBuf,
    tool_directory: PathBuf,
    repositories_root: PathBuf,
    dotfiles_repository_path: PathBuf,
    cargo_binaries_directory: PathBuf,
    state: RefCell<MachineState>,
}

#[derive(Debug, Default)]
struct MachineState {
    paths: BTreeSet<PathBuf>,
    links: BTreeMap<PathBuf, PathBuf>,
    unreadable_directories: BTreeSet<PathBuf>,
    link_removals_refused: bool,
    text_files: BTreeMap<PathBuf, String>,
    tools: BTreeSet<Tool>,
    installed_applications: BTreeSet<ApplicationName>,
    winget_packages: BTreeSet<WingetPackageId>,
    winget_packages_matched_only_by_identifier: BTreeSet<WingetPackageId>,
    winget_versions: BTreeMap<WingetPackageId, WingetVersion>,
    winget_offers: BTreeMap<WingetPackageId, WingetVersion>,
    winget_upgrade_refusals: BTreeMap<WingetPackageId, CommandOutput>,
    dart_readings: BTreeMap<DartPackageName, Result<DartReading, DartReadFailure>>,
    dart_install_refusals: BTreeMap<DartPackageName, CommandOutput>,
    uv_tools: BTreeMap<UvToolName, UvToolVersion>,
    uv_newest_versions: BTreeMap<UvToolName, UvToolVersion>,
    uv_tool_interpreters: BTreeMap<UvToolName, Option<PythonInterpreter>>,
    uv_running_launchers: BTreeMap<PathBuf, LauncherCopy>,
    uv_tools_failing_to_upgrade: BTreeSet<UvToolName>,
    uv_pinned_tools: BTreeSet<UvToolName>,
    failing_applications: BTreeSet<ApplicationName>,
    silent_applications: BTreeSet<ApplicationName>,
    install_attempts: Vec<ApplicationName>,
    installed_as: BTreeMap<ApplicationName, GitHubAccount>,
    commands_run: Vec<Vec<String>>,
    repository_contents: BTreeSet<PathBuf>,
    reads: Vec<ReadInvocation>,
    reads_answered_with: Vec<(ReadInvocation, CommandOutput)>,
    presence_check_answers: Vec<(PresenceCheck, bool)>,
    unreadable_presence_checks: Vec<PresenceCheck>,
    unreadable_releases: BTreeSet<GitHubRepository>,
    cargo_workspaces: BTreeMap<PathBuf, WorkspaceReading>,
    earlier_cargo_workspaces: BTreeMap<(PathBuf, Revision), BTreeMap<CrateName, Fingerprint>>,
    install_records: Vec<InstallRecord>,
    uninstalls_refused: bool,
    commits_in_clones: BTreeSet<(PathBuf, Revision)>,
    unopenable_clones: BTreeSet<PathBuf>,
    versions_asked: Vec<PathBuf>,
    workspace_reads: Vec<PathBuf>,
    executing_binaries: BTreeMap<PathBuf, Displacement>,
    superseded_images: BTreeSet<PathBuf>,
    cargo_installs: usize,
    displacing_installs: usize,
    member_install_arguments: Vec<Vec<String>>,
    cargo_commands: Vec<CargoCommand>,
    workspace_builds_fail: bool,
    registry_crates: BTreeMap<CrateName, CrateVersion>,
    published_crates: BTreeMap<CrateName, CrateVersion>,
    crates_io_refusals: BTreeMap<CrateName, StatusCode>,
    releases: BTreeMap<GitHubRepository, ReleaseReading>,
    release_reads: Vec<(GitHubRepository, GitHubAccount)>,
    clones: Vec<(GitHubRepository, GitHubAccount)>,
    shallow_clones: BTreeMap<PathBuf, CloneDepth>,
    version_output_by_binary_path: BTreeMap<PathBuf, String>,
    user_search_path: Vec<PathBuf>,
    machine_search_path: Vec<PathBuf>,
    machine_search_path_unreadable: bool,
    environment_variables: BTreeMap<VariableName, VariableValue>,
    claude_mcp_servers: BTreeMap<McpServerName, ClaudeMcpServer>,
    mcp_servers_claude_refuses_to_add: BTreeSet<McpServerName>,
    journal: Vec<Step>,
    failing_downloads: BTreeSet<String>,
    winget_sources_fail_to_update: bool,
    links_refused_for_want_of_a_privilege: bool,
    installers_demanding_elevation: BTreeSet<ApplicationName>,
    elevated: bool,
    elevation_declined: bool,
    elevated_batch_unrunnable: bool,
    elevation_prompts: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Began(String),
    Ended(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoCommand {
    ReapedBuildsOtherThan(BTreeSet<Revision>),
    Built {
        clone_directory: PathBuf,
        revision: Revision,
        members: BTreeSet<CrateName>,
    },
    Installed(CrateName),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallRecord {
    crate_name: CrateName,
    source: String,
    commit: Revision,
    binaries: Vec<BinaryName>,
}

impl InstallRecord {
    fn specification(&self) -> String {
        format!("git+{}#{}@0.1.0", self.source, self.crate_name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherCopy {
    Identical,
    Differs,
}

fn uv_tools_directory() -> PathBuf {
    home_directory_path()
        .join(".local")
        .join("share")
        .join("uv")
        .join("tools")
}

fn uv_bin_directory() -> PathBuf {
    home_directory_path().join(".local").join("bin")
}

fn uv_launcher_path(name: &UvToolName) -> PathBuf {
    uv_bin_directory().join(format!("{name}.exe"))
}

fn uv_environment_launcher_path(name: &UvToolName) -> PathBuf {
    uv_tools_directory()
        .join(name.as_ref())
        .join("Scripts")
        .join(format!("{name}.exe"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Displacement {
    Succeeds,
    Refused,
}

pub const CONFIGURATOR_VERSION_RUNNING_AND_NEWEST_PUBLISHED: &str = "9.9.9";

pub fn filesystem_root() -> PathBuf {
    match cfg!(windows) {
        true => PathBuf::from(r"C:\"),
        false => PathBuf::from("/"),
    }
}

pub fn home_directory_path() -> PathBuf {
    filesystem_root().join("home").join("alice")
}

pub fn repositories_root_path() -> PathBuf {
    filesystem_root().join("repositories")
}

pub fn dotfiles_repository_path() -> PathBuf {
    repositories_root_path().join("Personal").join("dotfiles")
}

impl Default for FakeMachine {
    fn default() -> Self {
        let home_directory = home_directory_path();
        let repositories_root = repositories_root_path();
        let dotfiles_repository_path = dotfiles_repository_path();
        let machine = Self {
            cargo_binaries_directory: home_directory.join(".cargo").join("bin"),
            tool_directory: tool_directory_within(&home_directory),
            home_directory,
            repositories_root,
            dotfiles_repository_path,
            state: RefCell::new(MachineState {
                tools: BTreeSet::from([
                    Tool::Winget,
                    Tool::Cargo,
                    Tool::Claude,
                    Tool::Wsl,
                    Tool::Git,
                    Tool::Uv,
                    Tool::Dart,
                ]),
                ..MachineState::default()
            }),
        };

        machine.publish_release(
            own_release_repository(),
            ReleaseReading {
                version: Version::try_from(CONFIGURATOR_VERSION_RUNNING_AND_NEWEST_PUBLISHED)
                    .expect("a version"),
                assets: vec![ReleaseAsset {
                    name: own_release_asset_name().to_owned(),
                    download_url: Url::parse("https://example.invalid/configurator.zip")
                        .expect("a url"),
                }],
            },
        );
        machine.hold_binary(
            machine
                .binaries_directory()
                .join(own_currency().installed_name().file_name()),
            format!("dotfiles_configurator {CONFIGURATOR_VERSION_RUNNING_AND_NEWEST_PUBLISHED}"),
        );
        machine.hold_user_search_path_entry(machine.binaries_directory());
        machine.hold_user_search_path_entry(machine.cargo_binaries_directory());
        machine.hold_machine_search_path_entry(machine.cargo_binaries_directory());
        machine.hold_machine_manifest(MachineClass::Personal);

        machine
    }
}

impl FakeMachine {
    pub fn hold_cargo_workspace(&self, repository_path: PathBuf, reading: WorkspaceReading) {
        self.state
            .borrow_mut()
            .cargo_workspaces
            .insert(repository_path, reading);
    }

    pub fn hold_cargo_workspace_member_at(
        &self,
        repository_path: PathBuf,
        revision: Revision,
        crate_name: CrateName,
        fingerprint: Fingerprint,
    ) {
        self.state
            .borrow_mut()
            .earlier_cargo_workspaces
            .entry((repository_path, revision))
            .or_default()
            .insert(crate_name, fingerprint);
    }

    pub fn hold_install_record(
        &self,
        crate_name: &CrateName,
        repository: &GitHubRepository,
        commit: &Revision,
        binaries: &[BinaryName],
    ) {
        self.state.borrow_mut().install_records.push(InstallRecord {
            crate_name: crate_name.clone(),
            source: format!("https://Alice@github.com/{repository}?rev={commit}"),
            commit: commit.clone(),
            binaries: binaries.to_vec(),
        });
    }

    pub fn refuse_every_uninstall(&self) {
        self.state.borrow_mut().uninstalls_refused = true;
    }

    pub fn cargo_records_the_crate(&self, crate_name: &CrateName) -> bool {
        self.state
            .borrow()
            .install_records
            .iter()
            .any(|record| record.crate_name == *crate_name)
    }

    pub fn hold_commit_in_clone(&self, clone_directory: PathBuf, commit: Revision) {
        self.state
            .borrow_mut()
            .commits_in_clones
            .insert((clone_directory, commit));
    }

    pub fn make_clone_unopenable(&self, clone_directory: PathBuf) {
        self.state
            .borrow_mut()
            .unopenable_clones
            .insert(clone_directory);
    }

    pub fn versions_asked(&self) -> Vec<PathBuf> {
        self.state.borrow().versions_asked.clone()
    }

    pub fn holds_cargo_binary(&self, name: &str) -> bool {
        self.state
            .borrow()
            .paths
            .contains(&self.cargo_binary_path(name))
    }

    pub fn hold_cargo_binary(&self, name: &str, version_output: String) {
        self.hold_binary(
            self.cargo_binaries_directory
                .join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
            version_output,
        );
    }

    pub fn remove_cargo_binary(&self, name: &str) {
        let path = self
            .cargo_binaries_directory
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        let mut state = self.state.borrow_mut();
        state.paths.remove(&path);
        state.version_output_by_binary_path.remove(&path);
    }

    pub fn cargo_workspace_reads(&self) -> Vec<PathBuf> {
        self.state.borrow().workspace_reads.clone()
    }

    pub fn repositories_root(&self) -> &Path {
        &self.repositories_root
    }

    pub fn dotfiles_repository_path(&self) -> &Path {
        &self.dotfiles_repository_path
    }

    fn cargo_binary_path(&self, name: &str) -> PathBuf {
        self.cargo_binaries_directory
            .join(BinaryName::from(name).file_name())
    }

    pub fn execute_binary(&self, name: &str) {
        self.state
            .borrow_mut()
            .executing_binaries
            .insert(self.cargo_binary_path(name), Displacement::Succeeds);
    }

    pub fn execute_binary_that_cannot_be_displaced(&self, name: &str) {
        self.state
            .borrow_mut()
            .executing_binaries
            .insert(self.cargo_binary_path(name), Displacement::Refused);
    }

    pub fn leave_superseded_image(&self, name: &str) {
        let displaced = superseded_name(&self.cargo_binary_path(name));
        self.state.borrow_mut().superseded_images.insert(displaced);
    }

    pub fn member_install_arguments(&self) -> Vec<Vec<String>> {
        self.state.borrow().member_install_arguments.clone()
    }

    pub fn displacing_installs(&self) -> usize {
        self.state.borrow().displacing_installs
    }

    pub fn leave_a_superseded_image_of_the_configurator(&self) {
        let displaced = superseded_name(&own_currency().path_within(&self.binaries_directory()));
        self.state.borrow_mut().superseded_images.insert(displaced);
    }

    pub fn own_binary_is_executing_and_will_not_release(&self) {
        self.state.borrow_mut().executing_binaries.insert(
            own_currency().path_within(&self.binaries_directory()),
            Displacement::Refused,
        );
    }

    pub fn superseded_image_count(&self) -> usize {
        self.state.borrow().superseded_images.len()
    }

    pub fn cargo_installs(&self) -> usize {
        self.state.borrow().cargo_installs
    }

    pub fn cargo_commands(&self) -> Vec<CargoCommand> {
        self.state.borrow().cargo_commands.clone()
    }

    pub fn make_workspace_builds_fail(&self) {
        self.state.borrow_mut().workspace_builds_fail = true;
    }

    pub fn times_the_declared_command_ran(&self, argument: &str) -> usize {
        self.state
            .borrow()
            .commands_run
            .iter()
            .filter(|arguments| arguments.iter().any(|given| given == argument))
            .count()
    }

    pub fn run_uv_tool_launcher(&self, name: &UvToolName, copy: LauncherCopy) {
        self.state
            .borrow_mut()
            .uv_running_launchers
            .insert(uv_launcher_path(name), copy);
    }

    pub fn make_uv_tool_upgrade_fail(&self, name: &UvToolName) {
        self.state
            .borrow_mut()
            .uv_tools_failing_to_upgrade
            .insert(name.clone());
    }

    pub fn journal(&self) -> Vec<Step> {
        self.state.borrow().journal.clone()
    }

    pub fn make_downloading_fail(&self, name: &str) {
        self.state
            .borrow_mut()
            .failing_downloads
            .insert(name.to_owned());
    }

    pub fn make_winget_sources_fail_to_update(&self) {
        self.state.borrow_mut().winget_sources_fail_to_update = true;
    }

    pub fn refuse_links_for_want_of_a_privilege(&self) {
        self.state
            .borrow_mut()
            .links_refused_for_want_of_a_privilege = true;
    }

    pub fn make_installer_demand_elevation(&self, name: &ApplicationName) {
        self.state
            .borrow_mut()
            .installers_demanding_elevation
            .insert(name.clone());
    }

    pub fn start_the_apply_elevated(&self) {
        self.state.borrow_mut().elevated = true;
    }

    pub fn decline_elevation(&self) {
        self.state.borrow_mut().elevation_declined = true;
    }

    pub fn make_the_elevated_batch_unrunnable(&self) {
        self.state.borrow_mut().elevated_batch_unrunnable = true;
    }

    pub fn allow_elevation(&self) {
        self.state.borrow_mut().elevation_declined = false;
    }

    pub fn elevation_prompts(&self) -> usize {
        self.state.borrow().elevation_prompts
    }

    fn performed_elevated(&self, work: &ElevatedWork) -> ElevatedOutcome {
        let mut state = self.state.borrow_mut();
        match work {
            ElevatedWork::Link {
                link_path,
                target_path,
            } => {
                state.links.insert(link_path.clone(), target_path.clone());
                ElevatedOutcome::Converged
            }
            ElevatedWork::Installer { installer_path, .. } => {
                let name = ApplicationName::from(
                    installer_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref(),
                );
                state.install_attempts.push(name.clone());
                if state.failing_applications.contains(&name) {
                    return ElevatedOutcome::Failed {
                        reason: format!("the installer for {name} exited non-zero"),
                    };
                }
                state.installed_applications.insert(name);
                ElevatedOutcome::Converged
            }
            #[cfg(target_family = "windows")]
            ElevatedWork::MachineSearchPathFront { directory } => {
                fronted(&mut state.machine_search_path, directory);
                ElevatedOutcome::Converged
            }
        }
    }

    async fn working<Output>(&self, work: String, act: impl FnOnce() -> Output) -> Output {
        self.state
            .borrow_mut()
            .journal
            .push(Step::Began(work.clone()));
        tokio::task::yield_now().await;
        let output = act();
        tokio::task::yield_now().await;
        self.state.borrow_mut().journal.push(Step::Ended(work));
        output
    }

    fn worked(&self, work: String) {
        let mut state = self.state.borrow_mut();
        state.journal.push(Step::Began(work.clone()));
        state.journal.push(Step::Ended(work));
    }

    fn run_write(&self, invocation: &WriteInvocation) -> Result<CommandOutput> {
        let mut state = self.state.borrow_mut();
        let mut standard_error = String::new();
        match invocation {
            WriteInvocation::UpdateWingetSources => {
                if state.winget_sources_fail_to_update {
                    standard_error = "Failed to update source: winget".to_owned();
                }
            }
            WriteInvocation::InstallWingetPackage { id, version } => {
                state.winget_packages.insert(id.clone());
                let installed = version
                    .clone()
                    .or_else(|| state.winget_offers.get(id).cloned());
                if let Some(installed) = installed {
                    state.winget_versions.insert(id.clone(), installed);
                }
            }
            WriteInvocation::UpgradeWingetPackage { id } => {
                if let Some(refusal) = state.winget_upgrade_refusals.get(id) {
                    return Ok(refusal.clone());
                }
                match state.winget_offers.get(id).cloned() {
                    Some(offered) if state.winget_packages.contains(id) => {
                        state.winget_versions.insert(id.clone(), offered);
                    }
                    Some(_) | None => {
                        standard_error = format!("No available upgrade found for {id}.");
                    }
                }
            }
            WriteInvocation::InstallUvTool {
                name,
                python,
                version: Some(version),
            } => {
                state.uv_tools.insert(name.clone(), version.clone());
                state.uv_pinned_tools.insert(name.clone());
                state
                    .uv_tool_interpreters
                    .entry(name.clone())
                    .or_insert_with(|| python.clone());
            }
            WriteInvocation::InstallUvTool {
                name,
                python,
                version: None,
            } => {
                if !state.uv_tools.contains_key(name) {
                    let Some(newest) = state.uv_newest_versions.get(name).cloned() else {
                        bail!("no version of {name} resolves");
                    };
                    state.uv_tools.insert(name.clone(), newest);
                    state
                        .uv_tool_interpreters
                        .insert(name.clone(), python.clone());
                }
            }
            WriteInvocation::InstallDartPackage { package } => {
                if let Some(refusal) = state.dart_install_refusals.get(&package.name) {
                    return Ok(refusal.clone());
                }
                state
                    .dart_readings
                    .insert(package.name.clone(), Ok(DartReading::Current));
            }
            WriteInvocation::ReinstallUvToolAtLatest { name, .. } => {
                let Some(newest) = state.uv_newest_versions.get(name).cloned() else {
                    bail!("no version of {name} resolves");
                };
                state.uv_tools.insert(name.clone(), newest);
                state.uv_pinned_tools.remove(name);
            }
            WriteInvocation::UpgradeUvTool { name } => {
                if !state.uv_tools.contains_key(name) {
                    bail!("Failed to upgrade {name}: `{name}` is not installed");
                }
                if state.uv_pinned_tools.contains(name) {
                    return Ok(CommandOutput {
                        exited: Exited::Code(0),
                        standard_output: "Nothing to upgrade\n".to_owned(),
                        standard_error: String::new(),
                    });
                }
                if state.uv_tools_failing_to_upgrade.contains(name) {
                    standard_error = format!(
                        "error: Failed to upgrade {name}\n  Caused by: Failed to fetch the \
                         index for {name} (os error 10061)\n"
                    );
                } else {
                    if let Some(newest) = state.uv_newest_versions.get(name).cloned() {
                        state.uv_tools.insert(name.clone(), newest);
                    }
                    let launcher = uv_launcher_path(name);
                    if state.uv_running_launchers.contains_key(&launcher) {
                        standard_error = format!(
                            "error: Failed to upgrade {name}\n  Caused by: Failed to install \
                             entrypoint\n  Caused by: failed to copy file from {} to {}: The \
                             process cannot access the file because it is being used by another \
                             process. (os error 32)\n",
                            uv_environment_launcher_path(name).display(),
                            launcher.display()
                        );
                    }
                }
            }
        }

        Ok(CommandOutput {
            exited: match standard_error.is_empty() {
                true => Exited::Code(0),
                false => Exited::Code(1),
            },
            standard_output: String::new(),
            standard_error,
        })
    }

    fn installing_member(&self, install: &MemberInstall) -> Placement {
        let mut state = self.state.borrow_mut();
        state.cargo_installs += 1;
        state
            .cargo_commands
            .push(CargoCommand::Installed(install.crate_name.clone()));
        state
            .member_install_arguments
            .push(install.arguments(&self.build_cache_directory()));

        let built: Vec<(PathBuf, String)> = state
            .cargo_workspaces
            .values()
            .filter_map(|reading| reading.members.get(&install.crate_name))
            .flat_map(|member| {
                member.binaries.iter().map(|binary| {
                    (
                        self.cargo_binaries_directory.join(binary.file_name()),
                        format!("{binary} {}\n", install.revision),
                    )
                })
            })
            .collect();

        let mut placement = Placement::Placed;
        for (path, version_output) in built {
            match state.executing_binaries.get(&path) {
                Some(Displacement::Refused) => {
                    placement = Placement::Held(HeldReason::BeingExecuted(path));
                    continue;
                }
                Some(Displacement::Succeeds) => {
                    state.executing_binaries.remove(&path);
                    state.superseded_images.insert(superseded_name(&path));
                }
                None => {}
            }
            state.paths.insert(path.clone());
            state
                .version_output_by_binary_path
                .insert(path, version_output);
        }
        placement
    }

    fn run_cargo(&self, invocation: &DisplacingInvocation) -> CommandOutput {
        let mut state = self.state.borrow_mut();
        state.cargo_installs += 1;
        state.displacing_installs += 1;

        let DisplacingInvocation::InstallCargoCrate { crate_name, source } = invocation;
        state
            .cargo_commands
            .push(CargoCommand::Installed(crate_name.clone()));

        if let Some(destination) = state.executing_binaries.keys().next().cloned() {
            return CommandOutput {
                exited: Exited::Code(101),
                standard_output: String::new(),
                standard_error: format!(
                    "error: failed to move `{}` to `{}`\n\nCaused by:\n  Access is denied. \
                     (os error 5)",
                    destination.with_extension("tmp").display(),
                    destination.display()
                ),
            };
        }

        if let ResolvedCargoSource::Registry { version } = source {
            let installed = version
                .clone()
                .or_else(|| state.published_crates.get(crate_name).cloned());
            if let Some(installed) = installed {
                state.registry_crates.insert(crate_name.clone(), installed);
            }
        }

        let built: Vec<(PathBuf, String)> = state
            .cargo_workspaces
            .values()
            .filter_map(|reading| Some((reading.members.get(crate_name)?, &reading.revision)))
            .flat_map(|(member, revision)| {
                member.binaries.iter().map(move |binary| {
                    (
                        self.cargo_binaries_directory.join(binary.file_name()),
                        format!("{binary} {revision}\n"),
                    )
                })
            })
            .collect();
        for (path, version_output) in built {
            state.paths.insert(path.clone());
            state
                .version_output_by_binary_path
                .insert(path, version_output);
        }

        CommandOutput {
            exited: Exited::Code(0),
            standard_output: String::new(),
            standard_error: String::new(),
        }
    }

    pub fn publish_a_newer_configurator(&self, version: &str) {
        self.publish_release(
            own_release_repository(),
            ReleaseReading {
                version: Version::try_from(version).expect("a version"),
                assets: vec![ReleaseAsset {
                    name: own_release_asset_name().to_owned(),
                    download_url: Url::parse("https://example.invalid/configurator.zip")
                        .expect("a url"),
                }],
            },
        );
    }

    pub fn configurator_reports(&self) -> Option<String> {
        self.binary_reports(&own_currency().path_within(&self.binaries_directory()))
    }

    pub fn publish_release(&self, repository: GitHubRepository, reading: ReleaseReading) {
        self.state.borrow_mut().releases.insert(repository, reading);
    }

    pub fn make_release_reading_fail(&self, repository: GitHubRepository) {
        self.state
            .borrow_mut()
            .unreadable_releases
            .insert(repository);
    }

    pub fn hold_claude_mcp_server(&self, server: ClaudeMcpServer) {
        self.state
            .borrow_mut()
            .claude_mcp_servers
            .insert(server.name.clone(), server);
    }

    pub fn refuse_to_add_claude_mcp_server(&self, name: &McpServerName) {
        self.state
            .borrow_mut()
            .mcp_servers_claude_refuses_to_add
            .insert(name.clone());
    }

    pub fn claude_mcp_server(&self, name: &McpServerName) -> Option<ClaudeMcpServer> {
        self.state.borrow().claude_mcp_servers.get(name).cloned()
    }

    pub fn answer_reading_with(&self, invocation: ReadInvocation, output: CommandOutput) {
        self.state
            .borrow_mut()
            .reads_answered_with
            .push((invocation, output));
    }

    pub fn make_reading_fail(&self, invocation: ReadInvocation, standard_error: &str) {
        self.answer_reading_with(
            invocation,
            CommandOutput {
                exited: Exited::Code(1),
                standard_output: String::new(),
                standard_error: standard_error.to_owned(),
            },
        );
    }

    pub fn answer_presence_check(&self, check: PresenceCheck, answer: bool) {
        self.state
            .borrow_mut()
            .presence_check_answers
            .push((check, answer));
    }

    pub fn make_presence_check_fail_to_run(&self, check: PresenceCheck) {
        self.state
            .borrow_mut()
            .unreadable_presence_checks
            .push(check);
    }

    pub fn account_cloning(&self, repository: &GitHubRepository) -> Option<GitHubAccount> {
        self.state
            .borrow()
            .clones
            .iter()
            .find(|(cloned, _)| cloned == repository)
            .map(|(_, account)| account.clone())
    }

    pub fn account_installing(&self, name: &ApplicationName) -> Option<GitHubAccount> {
        self.state.borrow().installed_as.get(name).cloned()
    }

    pub fn account_reading_releases_of(
        &self,
        repository: &GitHubRepository,
    ) -> Option<GitHubAccount> {
        self.state
            .borrow()
            .release_reads
            .iter()
            .find(|(read, _)| read == repository)
            .map(|(_, account)| account.clone())
    }

    pub fn release_reads(&self, repository: &GitHubRepository) -> usize {
        self.state
            .borrow()
            .release_reads
            .iter()
            .filter(|(read, _)| read == repository)
            .count()
    }

    pub fn hold_binary(&self, path: PathBuf, version_output: String) {
        let mut state = self.state.borrow_mut();
        state.paths.insert(path.clone());
        state
            .version_output_by_binary_path
            .insert(path, version_output);
    }

    pub fn binary_reports(&self, path: &Path) -> Option<String> {
        self.state
            .borrow()
            .version_output_by_binary_path
            .get(path)
            .cloned()
    }

    pub fn hold_user_search_path_entry(&self, directory: PathBuf) {
        self.state.borrow_mut().user_search_path.push(directory);
    }

    pub fn hold_machine_search_path_entry(&self, directory: PathBuf) {
        self.state.borrow_mut().machine_search_path.push(directory);
    }

    pub fn hold_machine_search_path_entry_first(&self, directory: PathBuf) {
        self.state
            .borrow_mut()
            .machine_search_path
            .insert(0, directory);
    }

    pub fn machine_search_path(&self) -> Vec<PathBuf> {
        self.state.borrow().machine_search_path.clone()
    }

    pub fn make_the_machine_search_path_unreadable(&self) {
        self.state.borrow_mut().machine_search_path_unreadable = true;
    }

    pub fn user_search_path(&self) -> Vec<PathBuf> {
        self.state.borrow().user_search_path.clone()
    }

    pub fn clear_the_search_path(&self) {
        let mut state = self.state.borrow_mut();
        state.user_search_path.clear();
        state.machine_search_path.clear();
    }

    pub fn hold_machine_manifest(&self, machine: MachineClass) {
        let manifest = crate::declarations::rendered(crate::declarations::manifest_for(machine));

        self.write_text_file(
            &MachineManifest::path_within(self.home_directory()),
            manifest.document(),
        )
        .expect("a manifest on the fake machine");
    }

    pub fn forget_the_machine_manifest(&self) {
        let path = MachineManifest::path_within(self.home_directory());
        let mut state = self.state.borrow_mut();
        state.text_files.remove(&path);
        state.paths.remove(&path);
    }

    pub fn machine_manifest(&self) -> Option<String> {
        self.text_file_at(&MachineManifest::path_within(self.home_directory()))
            .expect("a fake reads its own held files")
    }

    pub fn hold_environment_variable(&self, name: &VariableName, value: &VariableValue) {
        self.state
            .borrow_mut()
            .environment_variables
            .insert(name.clone(), value.clone());
    }

    pub fn environment_variable(&self, name: &VariableName) -> Option<VariableValue> {
        self.state.borrow().environment_variables.get(name).cloned()
    }

    pub fn remove_tool(&self, tool: Tool) {
        self.state.borrow_mut().tools.remove(&tool);
    }

    pub fn install_winget_package(&self, id: &WingetPackageId) {
        self.state.borrow_mut().winget_packages.insert(id.clone());
    }

    pub fn install_winget_package_at(&self, id: &WingetPackageId, version: &WingetVersion) {
        let mut state = self.state.borrow_mut();
        state.winget_packages.insert(id.clone());
        state.winget_versions.insert(id.clone(), version.clone());
    }

    pub fn offer_winget_upgrade(&self, id: &WingetPackageId, version: &WingetVersion) {
        self.state
            .borrow_mut()
            .winget_offers
            .insert(id.clone(), version.clone());
    }

    pub fn hold_dart_package_reading(
        &self,
        name: &DartPackageName,
        reading: Result<DartReading, DartReadFailure>,
    ) {
        self.state
            .borrow_mut()
            .dart_readings
            .insert(name.clone(), reading);
    }

    pub fn refuse_dart_install(&self, name: &DartPackageName, refusal: CommandOutput) {
        self.state
            .borrow_mut()
            .dart_install_refusals
            .insert(name.clone(), refusal);
    }

    pub fn refuse_winget_upgrade(&self, id: &WingetPackageId, refusal: CommandOutput) {
        self.state
            .borrow_mut()
            .winget_upgrade_refusals
            .insert(id.clone(), refusal);
    }

    pub fn winget_package_version(&self, id: &WingetPackageId) -> Option<WingetVersion> {
        let state = self.state.borrow();
        state.winget_packages.contains(id).then(|| {
            state
                .winget_versions
                .get(id)
                .cloned()
                .unwrap_or_else(|| WingetVersion::from(WINGET_VERSION_UNLESS_DECLARED))
        })
    }

    pub fn install_winget_package_matched_only_by_identifier(&self, id: &WingetPackageId) {
        self.state
            .borrow_mut()
            .winget_packages_matched_only_by_identifier
            .insert(id.clone());
    }

    pub fn install_registry_crate(&self, crate_name: &CrateName, version: &CrateVersion) {
        self.state
            .borrow_mut()
            .registry_crates
            .insert(crate_name.clone(), version.clone());
    }

    pub fn registry_crate_version(&self, crate_name: &CrateName) -> Option<CrateVersion> {
        self.state.borrow().registry_crates.get(crate_name).cloned()
    }

    pub fn publish_crate(&self, crate_name: &CrateName, version: &CrateVersion) {
        self.state
            .borrow_mut()
            .published_crates
            .insert(crate_name.clone(), version.clone());
    }

    pub fn make_crates_io_refuse_to_list(&self, crate_name: CrateName, status: StatusCode) {
        self.state
            .borrow_mut()
            .crates_io_refusals
            .insert(crate_name, status);
    }

    pub fn install_uv_tool(&self, name: &UvToolName, version: &UvToolVersion) {
        self.state
            .borrow_mut()
            .uv_tools
            .insert(name.clone(), version.clone());
    }

    pub fn install_uv_tool_pinned(&self, name: &UvToolName, version: &UvToolVersion) {
        self.install_uv_tool(name, version);
        self.state.borrow_mut().uv_pinned_tools.insert(name.clone());
    }

    pub fn publish_uv_tool(&self, name: &UvToolName, version: &UvToolVersion) {
        self.state
            .borrow_mut()
            .uv_newest_versions
            .insert(name.clone(), version.clone());
    }

    pub fn uv_tool_version(&self, name: &UvToolName) -> Option<UvToolVersion> {
        self.state.borrow().uv_tools.get(name).cloned()
    }

    pub fn uv_tool_interpreter(&self, name: &UvToolName) -> Option<PythonInterpreter> {
        self.state
            .borrow()
            .uv_tool_interpreters
            .get(name)
            .cloned()
            .flatten()
    }

    pub fn install_application(&self, name: &ApplicationName) {
        self.state
            .borrow_mut()
            .installed_applications
            .insert(name.clone());
    }

    pub fn application_is_installed(&self, name: &ApplicationName) -> bool {
        self.state.borrow().installed_applications.contains(name)
    }

    pub fn make_installing_fail(&self, name: &ApplicationName) {
        self.state
            .borrow_mut()
            .failing_applications
            .insert(name.clone());
    }

    pub fn install_attempts(&self, name: &ApplicationName) -> usize {
        self.state
            .borrow()
            .install_attempts
            .iter()
            .filter(|attempted| *attempted == name)
            .count()
    }

    pub fn add_own_file(&self, path: PathBuf) {
        self.state.borrow_mut().paths.insert(path);
    }

    pub fn own_file_is_intact(&self, path: &Path) -> bool {
        let state = self.state.borrow();
        state.paths.contains(path) && !state.links.contains_key(path)
    }

    pub fn make_installing_silently_do_nothing(&self, name: &ApplicationName) {
        self.state
            .borrow_mut()
            .silent_applications
            .insert(name.clone());
    }

    pub fn repository_holds(&self, path: PathBuf) {
        self.state.borrow_mut().repository_contents.insert(path);
    }

    pub fn clone_dotfiles_repository(&self) {
        let mut state = self.state.borrow_mut();
        materialise_clone(&mut state, &self.dotfiles_repository_path);
    }

    pub fn hold_shallow_clone(&self, clone_directory: &Path, depth: CloneDepth) {
        let mut state = self.state.borrow_mut();
        materialise_clone(&mut state, clone_directory);
        state
            .shallow_clones
            .insert(clone_directory.to_path_buf(), depth);
    }

    pub fn shallow_clone_depth(&self, clone_directory: &Path) -> Option<CloneDepth> {
        self.state
            .borrow()
            .shallow_clones
            .get(clone_directory)
            .copied()
    }

    pub fn dotfiles_repository_is_cloned(&self) -> bool {
        self.state
            .borrow()
            .paths
            .contains(&self.dotfiles_repository_path.join(".git"))
    }

    pub fn link_at(&self, link_path: &Path) -> Option<PathBuf> {
        self.state.borrow().links.get(link_path).cloned()
    }

    pub fn add_link(&self, link_path: PathBuf, target_path: PathBuf) {
        self.state.borrow_mut().links.insert(link_path, target_path);
    }

    pub fn refuse_to_remove_links(&self) {
        self.state.borrow_mut().link_removals_refused = true;
    }

    pub fn make_directory_unreadable(&self, directory: PathBuf) {
        self.state
            .borrow_mut()
            .unreadable_directories
            .insert(directory);
    }

    pub fn fingerprint(&self) -> String {
        let state = self.state.borrow();
        let MachineState {
            paths,
            links,
            unreadable_directories,
            link_removals_refused,
            text_files,
            tools,
            installed_applications,
            winget_packages,
            winget_packages_matched_only_by_identifier,
            winget_versions,
            winget_offers,
            winget_upgrade_refusals,
            dart_readings,
            dart_install_refusals,
            uv_tools,
            uv_newest_versions,
            uv_tool_interpreters,
            uv_running_launchers,
            uv_tools_failing_to_upgrade,
            uv_pinned_tools,
            failing_applications,
            silent_applications,
            install_attempts,
            installed_as,
            commands_run,
            repository_contents,
            reads: _,
            reads_answered_with: _,
            presence_check_answers: _,
            unreadable_presence_checks,
            unreadable_releases,
            cargo_workspaces,
            earlier_cargo_workspaces,
            install_records,
            uninstalls_refused,
            commits_in_clones,
            unopenable_clones,
            versions_asked: _,
            workspace_reads: _,
            executing_binaries,
            superseded_images,
            cargo_installs,
            displacing_installs,
            member_install_arguments: _,
            cargo_commands,
            workspace_builds_fail,
            registry_crates,
            published_crates,
            crates_io_refusals,
            releases,
            release_reads: _,
            clones,
            shallow_clones,
            version_output_by_binary_path,
            user_search_path,
            machine_search_path,
            machine_search_path_unreadable,
            environment_variables,
            claude_mcp_servers,
            mcp_servers_claude_refuses_to_add,
            journal: _,
            failing_downloads,
            winget_sources_fail_to_update,
            links_refused_for_want_of_a_privilege,
            installers_demanding_elevation,
            elevated,
            elevation_declined,
            elevated_batch_unrunnable,
            elevation_prompts: _,
        } = &*state;
        format!(
            "{paths:?}|{links:?}|{unreadable_directories:?}|{link_removals_refused:?}|\
             {text_files:?}|{tools:?}|{installed_applications:?}|\
             {winget_packages:?}|{winget_packages_matched_only_by_identifier:?}|\
             {winget_versions:?}|{winget_offers:?}|{winget_upgrade_refusals:?}|\
             {dart_readings:?}|{dart_install_refusals:?}|{uv_tools:?}|\
             {uv_newest_versions:?}|{uv_tool_interpreters:?}|{uv_running_launchers:?}|\
             {uv_tools_failing_to_upgrade:?}|{uv_pinned_tools:?}|{failing_applications:?}|\
             {silent_applications:?}|\
             {install_attempts:?}|{installed_as:?}|{commands_run:?}|{repository_contents:?}|\
             {unreadable_presence_checks:?}|{unreadable_releases:?}|{cargo_workspaces:?}|\
             {earlier_cargo_workspaces:?}|{install_records:?}|{uninstalls_refused:?}|{commits_in_clones:?}|{unopenable_clones:?}|{executing_binaries:?}|{superseded_images:?}|{cargo_installs:?}|{displacing_installs:?}|\
             {cargo_commands:?}|{workspace_builds_fail:?}|{registry_crates:?}|\
             {published_crates:?}|{crates_io_refusals:?}|{releases:?}|\
             {clones:?}|{shallow_clones:?}|{version_output_by_binary_path:?}|{user_search_path:?}|\
             {machine_search_path:?}|{machine_search_path_unreadable:?}|\
             {environment_variables:?}|{claude_mcp_servers:?}|\
             {mcp_servers_claude_refuses_to_add:?}|{failing_downloads:?}|\
             {winget_sources_fail_to_update:?}|{links_refused_for_want_of_a_privilege:?}|\
             {installers_demanding_elevation:?}|{elevated:?}|{elevation_declined:?}|\
             {elevated_batch_unrunnable:?}"
        )
    }
}

fn program_running(check: &PresenceCheck) -> PathBuf {
    match check {
        PresenceCheck::CommandOutputContains { shell, .. } => PathBuf::from(match shell {
            Shell::Bash => "bash",
            Shell::CommandPrompt => "cmd",
            Shell::PowerShell => "pwsh",
            Shell::Wsl => "wsl",
        }),
        PresenceCheck::CommandOnPath { command } => PathBuf::from(command),
        PresenceCheck::PathExists { .. } => panic!("a path check runs no program"),
    }
}

#[cfg(target_family = "windows")]
fn machine_environment_refused() -> RegistryReadFailure {
    RegistryReadFailure::KeyUnopened {
        key: RegistryKey::MachineEnvironment,
        error: Arc::new(windows_result::Error::from_hresult(
            windows_result::HRESULT::from_win32(ACCESS_DENIED.0.cast_unsigned()),
        )),
    }
}

#[cfg(target_family = "unix")]
fn machine_environment_refused() -> RegistryReadFailure {
    RegistryReadFailure::NoRegistry
}

fn fronted(machine_search_path: &mut Vec<PathBuf>, directory: &Path) {
    let reading = MachineSearchPath::of(machine_search_path.iter().cloned());
    if reading.place_of(directory) != SearchPathPlace::First {
        machine_search_path.insert(0, directory.to_path_buf());
    }
}

fn materialise_clone(state: &mut MachineState, clone_directory: &Path) {
    state.paths.insert(clone_directory.to_path_buf());
    state.paths.insert(clone_directory.join(".git"));
    let held: Vec<PathBuf> = state
        .repository_contents
        .iter()
        .map(|path| clone_directory.join(path))
        .collect();
    state.paths.extend(held);
}

fn claude_mcp_get_output(server: &ClaudeMcpServer) -> String {
    let mut reported = format!(
        "{}:\n  Scope: User config (available in all your projects)\n  Status: Connected\n  \
         Type: stdio\n  Command: {}\n  Args: {}\n",
        server.name,
        server.command,
        server.args.join(" ")
    );

    if !server.environment.is_empty() {
        reported.push_str("  Environment:\n");
        for (key, value) in &server.environment {
            reported.push_str(&format!("    {key}={value}\n"));
        }
    }
    reported
}

fn cargo_crate_listing(state: &MachineState) -> String {
    let registry = state.registry_crates.iter().map(|(crate_name, installed)| {
        format!(
            "{crate_name} v{installed}:\n    {}\n",
            BinaryName::from(crate_name.as_ref()).file_name()
        )
    });
    let from_git = state.install_records.iter().map(|record| {
        let mut listed = format!(
            "{} v0.1.0 ({}#{}):\n",
            record.crate_name,
            record.source,
            &record.commit.as_ref()[..8]
        );
        for binary in &record.binaries {
            listed.push_str(&format!("    {}\n", binary.file_name()));
        }
        listed
    });

    registry.chain(from_git).collect()
}

fn uv_tool_listing(state: &MachineState) -> String {
    state
        .uv_tools
        .iter()
        .map(|(name, installed)| {
            let required = match state.uv_pinned_tools.contains(name) {
                true => format!(" [required: =={installed}]"),
                false => String::new(),
            };
            format!("{name} v{installed}{required}\n- {name}\n")
        })
        .collect()
}

fn uv_outdated_tool_listing(state: &MachineState) -> String {
    state
        .uv_tools
        .iter()
        .filter_map(|(name, installed)| {
            let newest = state.uv_newest_versions.get(name)?;
            (newest != installed)
                .then(|| format!("{name} v{installed} [latest: {newest}]\n- {name}\n"))
        })
        .collect()
}

const WINGET_FINDS_NO_PACKAGE: &str = "No installed package found matching input criteria.\n";

const WINGET_EXITS_FINDING_NO_PACKAGE: i32 = 0x8A15_0014_u32.cast_signed();

const WINGET_VERSION_UNLESS_DECLARED: &str = "1.0.0";

pub const PRIVILEGE_NOT_HELD: OsError = OsError(1314);

const ELEVATION_REQUIRED: OsError = OsError(740);

#[cfg(target_family = "windows")]
const ACCESS_DENIED: OsError = OsError(5);

struct WingetRow {
    id: String,
    version: String,
    available: String,
}

fn winget_row_of(state: &MachineState, id: &WingetPackageId) -> WingetRow {
    let version = state.winget_versions.get(id).map_or_else(
        || WINGET_VERSION_UNLESS_DECLARED.to_owned(),
        ToString::to_string,
    );
    let available = state
        .winget_offers
        .get(id)
        .map(ToString::to_string)
        .filter(|offered| *offered != version)
        .unwrap_or_default();

    WingetRow {
        id: id.to_string(),
        version,
        available,
    }
}

fn winget_listing(rows: &[WingetRow]) -> String {
    const PACKAGE_NAME: &str = "A package";

    let width = |heading: &str, values: &mut dyn Iterator<Item = &String>| {
        heading.len().max(
            values
                .map(|value| value.chars().count())
                .max()
                .unwrap_or_default(),
        ) + 2
    };
    let name_width = "Name".len().max(PACKAGE_NAME.len()) + 2;
    let id_width = width("Id", &mut rows.iter().map(|row| &row.id));
    let version_width = width("Version", &mut rows.iter().map(|row| &row.version));
    let available_width = width("Available", &mut rows.iter().map(|row| &row.available));

    let mut listing = format!(
        "{:name_width$}{:id_width$}{:version_width$}{:available_width$}{}\n",
        "Name", "Id", "Version", "Available", "Source"
    );
    listing.push_str(
        &"-".repeat(name_width + id_width + version_width + available_width + "Source".len()),
    );
    listing.push('\n');
    for row in rows {
        listing.push_str(&format!(
            "{PACKAGE_NAME:name_width$}{:id_width$}{:version_width$}{:available_width$}winget\n",
            row.id, row.version, row.available
        ));
    }
    listing
}

impl FakeMachine {
    pub fn times_read(&self, invocation: &ReadInvocation) -> usize {
        self.state
            .borrow()
            .reads
            .iter()
            .filter(|read| *read == invocation)
            .count()
    }
}

impl ReadMachine for FakeMachine {
    fn home_directory(&self) -> &Path {
        &self.home_directory
    }

    fn tool_directory(&self) -> &Path {
        &self.tool_directory
    }

    fn is_elevated(&self) -> bool {
        self.state.borrow().elevated
    }

    fn superseded_images(&self) -> Vec<PathBuf> {
        self.state
            .borrow()
            .superseded_images
            .iter()
            .cloned()
            .collect()
    }

    fn displacement_directories(&self) -> Vec<PathBuf> {
        vec![
            self.cargo_binaries_directory.clone(),
            self.binaries_directory(),
        ]
    }

    fn path_exists(&self, path: &Path) -> bool {
        let state = self.state.borrow();
        state.paths.contains(path) || state.links.contains_key(path)
    }

    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        self.state.borrow().links.get(path).cloned()
    }

    fn directory_entries(&self, directory: &Path) -> Result<Vec<PathBuf>, FileReadFailure> {
        let state = self.state.borrow();
        if state.unreadable_directories.contains(directory) {
            return Err(FileReadFailure::of(
                directory,
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            ));
        }
        let entries: BTreeSet<PathBuf> = state
            .paths
            .iter()
            .chain(state.links.keys())
            .filter(|path| path.parent() == Some(directory))
            .cloned()
            .collect();
        Ok(entries.into_iter().collect())
    }

    fn canonical_path(&self, path: &Path) -> Option<PathBuf> {
        let state = self.state.borrow();
        match state.links.get(path) {
            Some(target) => Some(target.clone()),
            None if state.paths.contains(path) => Some(path.to_path_buf()),
            None => None,
        }
    }

    fn text_file_at(&self, path: &Path) -> Result<Option<String>, FileReadFailure> {
        Ok(self.state.borrow().text_files.get(path).cloned())
    }

    fn tool_is_present(&self, tool: Tool) -> bool {
        self.state.borrow().tools.contains(&tool)
    }

    fn read(&self, invocation: &ReadInvocation) -> Result<CommandOutput, ProgramStartFailure> {
        self.state.borrow_mut().reads.push(invocation.clone());

        let declared = self
            .state
            .borrow()
            .reads_answered_with
            .iter()
            .find(|(asked, _)| asked == invocation)
            .map(|(_, output)| output.clone());
        if let Some(output) = declared {
            return Ok(output);
        }

        let mut standard_error = String::new();
        let succeeded = Exited::Code(0);
        let (exited, standard_output) = match invocation {
            ReadInvocation::WingetInstalledPackages => {
                let state = self.state.borrow();
                let rows: Vec<WingetRow> = state
                    .winget_packages
                    .iter()
                    .map(|id| winget_row_of(&state, id))
                    .collect();
                (succeeded, winget_listing(&rows))
            }
            ReadInvocation::WingetPackage { id } => {
                let state = self.state.borrow();
                match state.winget_packages.contains(id)
                    || state
                        .winget_packages_matched_only_by_identifier
                        .contains(id)
                {
                    true => (succeeded, winget_listing(&[winget_row_of(&state, id)])),
                    false => (
                        Exited::Code(WINGET_EXITS_FINDING_NO_PACKAGE),
                        WINGET_FINDS_NO_PACKAGE.to_owned(),
                    ),
                }
            }
            ReadInvocation::WingetUpgrades => {
                let state = self.state.borrow();
                let rows: Vec<WingetRow> = state
                    .winget_packages
                    .iter()
                    .chain(&state.winget_packages_matched_only_by_identifier)
                    .map(|id| winget_row_of(&state, id))
                    .filter(|row| !row.available.is_empty())
                    .collect();
                match rows.is_empty() {
                    true => (
                        Exited::Code(WINGET_EXITS_FINDING_NO_PACKAGE),
                        WINGET_FINDS_NO_PACKAGE.to_owned(),
                    ),
                    false => (succeeded, winget_listing(&rows)),
                }
            }
            ReadInvocation::CargoInstalledCrates => {
                (succeeded, cargo_crate_listing(&self.state.borrow()))
            }
            ReadInvocation::UvInstalledTools => (succeeded, uv_tool_listing(&self.state.borrow())),
            ReadInvocation::UvOutdatedTools => {
                (succeeded, uv_outdated_tool_listing(&self.state.borrow()))
            }
            ReadInvocation::ClaudeMcpServer { name } => {
                match self.state.borrow().claude_mcp_servers.get(name) {
                    None => {
                        standard_error = format!("No MCP server named \"{name}\".");
                        (Exited::Code(1), String::new())
                    }
                    Some(server) => (succeeded, claude_mcp_get_output(server)),
                }
            }
        };

        Ok(CommandOutput {
            exited,
            standard_output,
            standard_error,
        })
    }

    fn read_cargo_workspace(
        &self,
        repository_path: &Path,
    ) -> Result<Option<WorkspaceReading>, WorkspaceReadFailure> {
        let is_cloned = self.path_exists(&repository_path.join(".git"));

        let mut state = self.state.borrow_mut();
        state.workspace_reads.push(repository_path.to_path_buf());
        if !is_cloned {
            return Ok(None);
        }

        Ok(state.cargo_workspaces.get(repository_path).cloned())
    }

    fn read_cargo_workspace_at(
        &self,
        repository_path: &Path,
        revision: &Revision,
    ) -> Result<BTreeMap<CrateName, Fingerprint>, WorkspaceReadFailure> {
        let state = self.state.borrow();
        if let Some(reading) = state.cargo_workspaces.get(repository_path)
            && reading.revision == *revision
        {
            return Ok(reading
                .members
                .iter()
                .map(|(crate_name, member)| (crate_name.clone(), member.desired.clone()))
                .collect());
        }

        match state
            .earlier_cargo_workspaces
            .get(&(repository_path.to_path_buf(), revision.clone()))
        {
            Some(members) => Ok(members.clone()),
            None => Err(WorkspaceReadFailure::RevisionNotInClone {
                revision: revision.clone(),
                error: Arc::new(git2::Error::new(
                    git2::ErrorCode::NotFound,
                    git2::ErrorClass::Reference,
                    format!("revspec '{revision}' not found"),
                )),
            }),
        }
    }

    fn cargo_binaries_directory(&self) -> PathBuf {
        self.cargo_binaries_directory.clone()
    }

    fn resolve_commit(
        &self,
        repository_path: &Path,
        revision: &Revision,
    ) -> Result<(), WorkspaceReadFailure> {
        let state = self.state.borrow();
        if state.unopenable_clones.contains(repository_path) {
            return Err(WorkspaceReadFailure::CloneUnopened {
                clone: repository_path.to_path_buf(),
                error: Arc::new(git2::Error::from_str("could not find repository")),
            });
        }
        let at_its_head = state
            .cargo_workspaces
            .get(repository_path)
            .is_some_and(|reading| reading.revision == *revision);
        let held = (repository_path.to_path_buf(), revision.clone());
        if at_its_head
            || state.earlier_cargo_workspaces.contains_key(&held)
            || state.commits_in_clones.contains(&held)
        {
            return Ok(());
        }

        Err(WorkspaceReadFailure::RevisionNotInClone {
            revision: revision.clone(),
            error: Arc::new(git2::Error::from_str("revspec not found")),
        })
    }

    fn check_presence(&self, check: &PresenceCheck) -> Result<Option<Answer>, ProgramStartFailure> {
        let declared = {
            let state = self.state.borrow();
            if state.unreadable_presence_checks.contains(check) {
                return Err(ProgramStartFailure::of(
                    &program_running(check),
                    std::io::Error::from(std::io::ErrorKind::NotFound),
                ));
            }
            state
                .presence_check_answers
                .iter()
                .find(|(declared, _)| declared == check)
                .map(|(_, answer)| *answer)
        };
        if let Some(answer) = declared {
            return Ok(answer.then_some(Answer::TheCheckItself));
        }

        match check {
            PresenceCheck::PathExists { paths } => Ok(paths
                .first_found(|candidate| self.path_exists(&self.resolve_against_home(candidate)))),
            PresenceCheck::CommandOnPath { command } => Ok(self
                .state
                .borrow()
                .installed_applications
                .iter()
                .any(|installed| installed.to_string() == *command)
                .then_some(Answer::TheCheckItself)),
            PresenceCheck::CommandOutputContains { .. } => {
                panic!("no scenario declared what {check} answers on this machine")
            }
        }
    }

    fn clone_is_shallow(&self, clone_directory: &Path) -> Result<bool, CloneReadFailure> {
        Ok(self
            .state
            .borrow()
            .shallow_clones
            .contains_key(clone_directory))
    }

    async fn latest_release(
        &self,
        repository: &GitHubRepository,
        account: &GitHubAccount,
    ) -> Result<Option<ReleaseReading>, ReleaseReadFailure> {
        let mut state = self.state.borrow_mut();
        state
            .release_reads
            .push((repository.clone(), account.clone()));

        if state.unreadable_releases.contains(repository) {
            return Err(ReleaseReadFailure::AccountUnavailable(Arc::new(
                EnvironmentFailure::GitHubCli(cli::Refusal::AccountUnheld {
                    account: account.to_string(),
                }),
            )));
        }

        Ok(state.releases.get(repository).cloned())
    }

    fn read_dart_package(&self, package: &DartPackage) -> Result<DartReading, DartReadFailure> {
        match self.state.borrow().dart_readings.get(&package.name) {
            None => Ok(DartReading::Drifted(DartDrift::NoBundle)),
            Some(reading) => reading.clone(),
        }
    }

    async fn newest_published_crate(
        &self,
        crate_name: &CrateName,
    ) -> Result<CrateVersion, CrateIndexReadFailure> {
        let state = self.state.borrow();
        if let Some(status) = state.crates_io_refusals.get(crate_name) {
            return Err(CrateIndexReadFailure::Refused {
                crate_name: crate_name.clone(),
                status: *status,
            });
        }

        state
            .published_crates
            .get(crate_name)
            .cloned()
            .ok_or_else(|| CrateIndexReadFailure::NoCrateNamed {
                crate_name: crate_name.clone(),
            })
    }

    fn read_search_path(&self) -> Result<SearchPathReading, RegistryReadFailure> {
        let state = self.state.borrow();

        Ok(SearchPathReading::of(
            state
                .machine_search_path
                .iter()
                .chain(state.user_search_path.iter())
                .cloned(),
        ))
    }

    fn read_machine_search_path(&self) -> Result<MachineSearchPath, RegistryReadFailure> {
        let state = self.state.borrow();
        if state.machine_search_path_unreadable {
            return Err(machine_environment_refused());
        }

        Ok(MachineSearchPath::of(
            state.machine_search_path.iter().cloned(),
        ))
    }

    fn read_environment_variable(
        &self,
        name: &VariableName,
    ) -> Result<Option<VariableValue>, RegistryReadFailure> {
        Ok(self.state.borrow().environment_variables.get(name).cloned())
    }

    fn report_version(
        &self,
        binary_path: &Path,
        _arguments: &[String],
    ) -> Result<CommandOutput, VersionReportFailure> {
        let mut state = self.state.borrow_mut();
        state.versions_asked.push(binary_path.to_path_buf());
        let Some(printed) = state.version_output_by_binary_path.get(binary_path) else {
            return Err(VersionReportFailure::NotStarted(ProgramStartFailure::of(
                binary_path,
                std::io::Error::from(std::io::ErrorKind::InvalidInput),
            )));
        };

        Ok(CommandOutput {
            exited: Exited::Code(0),
            standard_output: printed.clone(),
            standard_error: String::new(),
        })
    }
}

impl WriteMachine for FakeMachine {
    fn create_link(&self, link_path: &Path, target_path: &Path) -> Result<Placement> {
        self.worked(format!("link {}", link_path.display()));
        let mut state = self.state.borrow_mut();
        if state.paths.contains(link_path) && !state.links.contains_key(link_path) {
            bail!(
                "{} already exists and is not a link. Move it aside to let the dotfiles \
                 repository own it; this tool will not delete something it did not create.",
                link_path.display()
            );
        }
        if state.links_refused_for_want_of_a_privilege {
            return Ok(Placement::Refused(PrivilegeRefusal {
                work: ElevatedWork::Link {
                    link_path: link_path.to_path_buf(),
                    target_path: target_path.to_path_buf(),
                },
                os_error: PRIVILEGE_NOT_HELD,
            }));
        }

        state
            .links
            .insert(link_path.to_path_buf(), target_path.to_path_buf());
        Ok(Placement::Placed)
    }

    fn remove_binary(&self, path: &Path) -> Result<()> {
        self.worked(format!("remove {}", path.display()));
        let mut state = self.state.borrow_mut();
        match state.executing_binaries.get(path) {
            Some(Displacement::Refused) => {
                bail!(
                    "Could not remove {}: it is being executed and cannot be moved aside",
                    path.display()
                );
            }
            Some(Displacement::Succeeds) => {
                state.executing_binaries.remove(path);
                state.superseded_images.insert(superseded_name(path));
            }
            None => {}
        }
        state.paths.remove(path);
        state.version_output_by_binary_path.remove(path);
        Ok(())
    }

    fn remove_link(&self, link_path: &Path) -> Result<()> {
        self.worked(format!("unlink {}", link_path.display()));
        let mut state = self.state.borrow_mut();
        if state.link_removals_refused {
            bail!("Could not remove the link at {}", link_path.display());
        }
        match state.links.remove(link_path) {
            Some(_) => Ok(()),
            None => bail!("{} is not a link", link_path.display()),
        }
    }

    fn write_text_file(&self, path: &Path, contents: &str) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state
            .text_files
            .insert(path.to_path_buf(), contents.to_owned());
        state.paths.insert(path.to_path_buf());
        Ok(())
    }

    async fn clone_repository(
        &self,
        clone: &RepositoryClone,
        clone_directory: &Path,
        account: &GitHubAccount,
    ) -> Result<()> {
        self.working(format!("clone {}", clone.repository), || {
            let mut state = self.state.borrow_mut();
            state
                .clones
                .push((clone.repository.clone(), account.clone()));
            materialise_clone(&mut state, clone_directory);
            if let Some(depth) = clone.depth {
                state
                    .shallow_clones
                    .insert(clone_directory.to_path_buf(), depth);
            }
            Ok(())
        })
        .await
    }

    async fn deepen_clone(
        &self,
        _repository: &GitHubRepository,
        clone_directory: &Path,
        _account: &GitHubAccount,
    ) -> Result<()> {
        self.state
            .borrow_mut()
            .shallow_clones
            .remove(clone_directory);
        Ok(())
    }

    async fn download_installer(
        &self,
        installer: &Installer,
        _release_asset: Option<&ReleaseAsset>,
    ) -> Result<Downloaded<Installer>> {
        let name = installer.name.to_string();
        self.working(format!("download {name}"), || {
            if self.state.borrow().failing_downloads.contains(&name) {
                bail!("the download of {name} was cut off");
            }
            Ok(Downloaded::fetched(
                installer.clone(),
                home_directory_path().join("Downloads").join(&name),
            ))
        })
        .await
    }

    async fn install_application(&self, downloaded: Downloaded<Installer>) -> Result<Placement> {
        let installer = downloaded.declared();
        self.working(format!("install {}", installer.name), || {
            let demands_elevation = self
                .state
                .borrow()
                .installers_demanding_elevation
                .contains(&installer.name);
            if !demands_elevation {
                return self
                    .installing_application(installer)
                    .map(|()| Placement::Placed);
            }

            Ok(Placement::Refused(PrivilegeRefusal {
                work: ElevatedWork::Installer {
                    installer_path: downloaded.file().to_path_buf(),
                    digest: ContentDigest::of(installer.name.to_string().as_bytes())?,
                },
                os_error: ELEVATION_REQUIRED,
            }))
        })
        .await
    }

    async fn run_elevated(&self, batch: &ElevatedBatch<()>) -> Result<Elevation> {
        self.working("elevated batch".to_owned(), || {
            let (declined, unrunnable) = {
                let mut state = self.state.borrow_mut();
                state.elevation_prompts += 1;
                (state.elevation_declined, state.elevated_batch_unrunnable)
            };
            if declined {
                return Ok(Elevation::Declined);
            }
            if unrunnable {
                bail!("the elevated relaunch exited with code 1");
            }

            let entries = batch
                .entries
                .iter()
                .map(|Batched { work, .. }| Batched {
                    work: work.clone(),
                    outcome: self.performed_elevated(work),
                })
                .collect();
            Ok(Elevation::Performed(ElevatedBatch { entries }))
        })
        .await
    }

    async fn download_released_binary(
        &self,
        binary: &ReleasedBinary,
        asset: &ReleaseAsset,
    ) -> Result<Downloaded<ReleasedBinary>> {
        let name = binary.installed_name().to_string();
        self.working(format!("download {name}"), || {
            if self.state.borrow().failing_downloads.contains(&name) {
                bail!("the download of {name} was cut off");
            }
            Ok(Downloaded::fetched(
                binary.clone(),
                home_directory_path().join("Downloads").join(&asset.name),
            ))
        })
        .await
    }

    fn install_released_binary(&self, downloaded: Downloaded<ReleasedBinary>) -> Result<Placement> {
        let binary = downloaded.declared();
        self.worked(format!("install {}", binary.installed_name()));
        self.installing_released_binary(binary)
    }

    fn put_on_search_path(&self, directory: &Path) -> Result<()> {
        self.worked(format!("search path {}", directory.display()));
        let mut state = self.state.borrow_mut();
        let carried = SearchPathReading::of(state.user_search_path.iter().cloned());
        if !carried.carries(directory) {
            state.user_search_path.push(directory.to_path_buf());
        }
        Ok(())
    }

    #[cfg(target_family = "windows")]
    fn put_first_on_machine_search_path(&self, directory: &Path) -> Result<Placement> {
        self.worked(format!("Machine PATH {}", directory.display()));
        let mut state = self.state.borrow_mut();
        if !state.elevated {
            return Ok(Placement::Refused(PrivilegeRefusal {
                work: ElevatedWork::MachineSearchPathFront {
                    directory: directory.to_path_buf(),
                },
                os_error: ACCESS_DENIED,
            }));
        }

        fronted(&mut state.machine_search_path, directory);
        Ok(Placement::Placed)
    }

    fn set_environment_variable(&self, name: &VariableName, value: &VariableValue) -> Result<()> {
        self.worked(format!("set {name}"));
        self.state
            .borrow_mut()
            .environment_variables
            .insert(name.clone(), value.clone());
        Ok(())
    }

    async fn write(&self, invocation: &WriteInvocation) -> Result<CommandOutput> {
        let output = self
            .working(work_of(invocation), || self.run_write(invocation))
            .await?;
        match output.exited.succeeded() {
            true => Ok(output),
            false => bail!("{} failed: {}", invocation.tool(), output.standard_error),
        }
    }

    async fn attempt_write(&self, invocation: &WriteInvocation) -> Result<CommandOutput> {
        self.working(work_of(invocation), || self.run_write(invocation))
            .await
    }

    async fn write_over_running_images(&self, invocation: &WriteInvocation) -> Result<Placement> {
        let output = self
            .working(work_of(invocation), || self.run_write(invocation))
            .await?;
        if output.exited.succeeded() {
            return Ok(Placement::Placed);
        }
        if let Some(reason) = invocation.held_by(&output) {
            return Ok(Placement::Held(reason));
        }

        let Some(copy) = invocation.refused_copy(&output) else {
            bail!("{} failed: {}", invocation.tool(), output.standard_error);
        };
        let identical = self
            .state
            .borrow()
            .uv_running_launchers
            .get(&copy.destination)
            == Some(&LauncherCopy::Identical);

        match identical {
            true => Ok(Placement::Placed),
            false => Ok(Placement::Held(HeldReason::BeingExecuted(copy.destination))),
        }
    }

    async fn replace(&self, invocation: &ReplacingInvocation) -> Result<Replacement> {
        let ReplacingInvocation::ClaudeMcpServer { server } = invocation;
        self.working(format!("register {}", server.name), || {
            let mut state = self.state.borrow_mut();
            let the_name_was_freed = state.claude_mcp_servers.remove(&server.name).is_some();

            if state
                .mcp_servers_claude_refuses_to_add
                .contains(&server.name)
            {
                return invocation.refused_claim(
                    the_name_was_freed,
                    anyhow!("claude refused to add {}", server.name),
                );
            }

            state
                .claude_mcp_servers
                .insert(server.name.clone(), (**server).clone());
            Ok(Replacement::Replaced)
        })
        .await
    }

    async fn write_displacing(&self, invocation: &DisplacingInvocation) -> Result<Placement> {
        let DisplacingInvocation::InstallCargoCrate { crate_name, .. } = invocation;
        self.working(format!("cargo install {crate_name}"), || {
            self.writing_displacing(invocation)
        })
        .await
    }

    async fn install_workspace_member(&self, install: &MemberInstall) -> Result<Placement> {
        Ok(self
            .working(format!("cargo install {}", install.crate_name), || {
                self.installing_member(install)
            })
            .await)
    }

    async fn untrack_cargo_record(&self, untracking: &RecordUntracking) -> Result<()> {
        self.working(
            format!("cargo uninstall {}", untracking.specification),
            || {
                let mut state = self.state.borrow_mut();
                if state.uninstalls_refused {
                    bail!("cargo refused to uninstall {}", untracking.specification);
                }
                let Some(position) = state
                    .install_records
                    .iter()
                    .position(|record| record.specification() == untracking.specification)
                else {
                    bail!(
                        "package ID specification `{}` did not match any packages",
                        untracking.specification
                    );
                };
                state.install_records.remove(position);
                Ok(())
            },
        )
        .await
    }

    fn reap_builds_of_other_revisions(&self, building: &BTreeSet<Revision>) {
        self.state
            .borrow_mut()
            .cargo_commands
            .push(CargoCommand::ReapedBuildsOtherThan(building.clone()));
    }

    async fn build_workspace_members(&self, build: &WorkspaceBuild<'_>) -> Result<()> {
        self.working("cargo build".to_owned(), || {
            let mut state = self.state.borrow_mut();
            state.cargo_commands.push(CargoCommand::Built {
                clone_directory: build.clone_directory().to_path_buf(),
                revision: build.revision().clone(),
                members: build.members().clone(),
            });

            match state.workspace_builds_fail {
                true => bail!("cargo could not build {build}"),
                false => Ok(()),
            }
        })
        .await
    }

    fn sweep_superseded_images(&self) {
        let mut state = self.state.borrow_mut();
        let still_held: BTreeSet<PathBuf> = state
            .executing_binaries
            .keys()
            .map(|path| superseded_name(path))
            .collect();
        state
            .superseded_images
            .retain(|image| still_held.contains(image));
    }

    async fn run_declared_command(&self, _shell: Shell, args: &[String]) -> Result<CommandOutput> {
        self.working(format!("command {}", args.join(" ")), || {
            self.state.borrow_mut().commands_run.push(args.to_vec());
            Ok(CommandOutput {
                exited: Exited::Code(0),
                standard_output: String::new(),
                standard_error: String::new(),
            })
        })
        .await
    }
}

fn work_of(invocation: &WriteInvocation) -> String {
    match invocation {
        WriteInvocation::UpdateWingetSources => "winget source update".to_owned(),
        WriteInvocation::InstallWingetPackage { id, .. } => format!("winget install {id}"),
        WriteInvocation::UpgradeWingetPackage { id } => format!("winget upgrade {id}"),
        WriteInvocation::InstallUvTool { name, .. }
        | WriteInvocation::UpgradeUvTool { name }
        | WriteInvocation::ReinstallUvToolAtLatest { name, .. } => format!("uv {name}"),
        WriteInvocation::InstallDartPackage { package } => format!("dart install {}", package.name),
    }
}

impl FakeMachine {
    fn installing_application(&self, installer: &Installer) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.install_attempts.push(installer.name.clone());

        if let ApplicationSource::GitHubRelease {
            owner, repository, ..
        } = &installer.source
        {
            let repository = GitHubRepository {
                owner: owner.clone(),
                repository: repository.clone(),
            };
            if let Some(account) = state
                .release_reads
                .iter()
                .find(|(read, _)| *read == repository)
                .map(|(_, account)| account.clone())
            {
                state.installed_as.insert(installer.name.clone(), account);
            }
        }

        if state.failing_applications.contains(&installer.name) {
            bail!("the installer for {} exited non-zero", installer.name);
        }
        if state.silent_applications.contains(&installer.name) {
            return Ok(());
        }

        state.installed_applications.insert(installer.name.clone());
        Ok(())
    }

    fn installing_released_binary(&self, binary: &ReleasedBinary) -> Result<Placement> {
        let installed_path = binary.path_within(&self.binaries_directory());
        let mut state = self.state.borrow_mut();

        if state.executing_binaries.get(&installed_path) == Some(&Displacement::Refused) {
            return Ok(Placement::Held(HeldReason::BeingExecuted(installed_path)));
        }
        let was_executing = state.executing_binaries.remove(&installed_path).is_some();

        let Some(release) = state.releases.get(&binary.repository) else {
            bail!("{} has published no release", binary.repository);
        };
        let reported = format!("{} {}", binary.installed_name(), release.version);

        if was_executing {
            state
                .superseded_images
                .insert(superseded_name(&installed_path));
        }
        state.paths.insert(installed_path.clone());
        state
            .version_output_by_binary_path
            .insert(installed_path, reported);
        Ok(Placement::Placed)
    }

    fn writing_displacing(&self, invocation: &DisplacingInvocation) -> Result<Placement> {
        let output = self.run_cargo(invocation);
        if output.exited.succeeded() {
            return Ok(Placement::Placed);
        }

        let Some(destination) = invocation.refused_destination(&output) else {
            bail!("{} failed", invocation.tool());
        };

        let mut state = self.state.borrow_mut();
        if state.executing_binaries.get(&destination) == Some(&Displacement::Refused) {
            return Ok(Placement::Held(HeldReason::BeingExecuted(destination)));
        }

        state.executing_binaries.remove(&destination);
        state
            .superseded_images
            .insert(superseded_name(&destination));
        drop(state);

        match self.run_cargo(invocation).exited.succeeded() {
            true => Ok(Placement::Placed),
            false => bail!("cargo failed once the image in its way had been displaced"),
        }
    }
}

impl WriteSource for FakeMachine {
    fn rewrite(&self, migration: &Migration) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.text_files.insert(
            migration.path().to_path_buf(),
            migration.contents().to_owned(),
        );
        state.paths.insert(migration.path().to_path_buf());
        Ok(())
    }
}

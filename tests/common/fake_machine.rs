//! A machine held in memory, substituted for the real one wherever a test needs to say what is
//! true of a machine. Substituting a machine is ordinary use of the capability traits rather than
//! a mode of the program, which is what ADR 0006 traded the `--debug` directory relocation for.

// Each integration test file is its own crate and pulls this module in whole, so a helper only
// one of them needs reads as dead in the others.
#![allow(dead_code)]

use {
    anyhow::{Result, anyhow, bail},
    dotfiles_configurator::{
        configuration::{
            ApplicationName, CrateName, GitHubAccount, GitHubRepository, Installer, MachineClass,
            MachineManifest, PresenceCheck, ReleasedBinary, Shell, VariableName, VariableValue,
            WingetPackageId,
        },
        convergence::{machine_manifest_document, machine_manifest_path},
        currency::{own_currency, own_release_asset_name, own_release_repository},
        machine::{
            CommandOutput, DisplacingInvocation, Placement, ReadInvocation, ReadMachine, Tool,
            WriteInvocation, WriteMachine,
            environment_reading::SearchPathReading,
            release_reading::{ReleaseAsset, ReleaseReading},
            superseded_name,
            workspace_reading::{Revision, WorkspaceReading},
        },
        version::Version,
    },
    std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
    },
    url::Url,
};

#[derive(Debug)]
pub struct FakeMachine {
    home_directory: PathBuf,
    repositories_root: PathBuf,
    dotfiles_repository_path: PathBuf,
    cargo_binaries_directory: PathBuf,
    state: RefCell<MachineState>,
}

#[derive(Debug, Default)]
struct MachineState {
    paths: BTreeSet<PathBuf>,
    links: BTreeMap<PathBuf, PathBuf>,
    text_files: BTreeMap<PathBuf, String>,
    tools: BTreeSet<Tool>,
    installed_applications: BTreeSet<ApplicationName>,
    winget_packages: BTreeSet<WingetPackageId>,
    failing_applications: BTreeSet<ApplicationName>,
    /// Installers that exit zero without putting anything on the machine.
    silent_applications: BTreeSet<ApplicationName>,
    install_attempts: Vec<(ApplicationName, GitHubAccount)>,
    commands_run: Vec<Vec<String>>,
    /// What the dotfiles repository holds, which only appears on the machine once it is cloned.
    repository_contents: BTreeSet<PathBuf>,
    reads: Vec<ReadInvocation>,
    cargo_workspaces: BTreeMap<PathBuf, WorkspaceReading>,
    workspace_reads: Vec<PathBuf>,
    executing_binaries: BTreeMap<PathBuf, Displacement>,
    superseded_images: BTreeSet<PathBuf>,
    cargo_installs: usize,
    releases: BTreeMap<GitHubRepository, ReleaseReading>,
    release_reads: Vec<(GitHubRepository, GitHubAccount)>,
    clones: Vec<(GitHubRepository, GitHubAccount)>,
    version_output_by_binary_path: BTreeMap<PathBuf, String>,
    user_search_path: Vec<PathBuf>,
    machine_search_path: Vec<PathBuf>,
    environment_variables: BTreeMap<VariableName, VariableValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Displacement {
    Succeeds,
    Refused,
}

/// The version the machine is already running and the newest one published, which are the same
/// so that the configurator's own currency is converged in every scenario that is not about it.
pub const CONFIGURATOR_VERSION: &str = "9.9.9";

impl Default for FakeMachine {
    fn default() -> Self {
        let home_directory = PathBuf::from("/home/alice");
        let repositories_root = PathBuf::from("/repositories");
        let dotfiles_repository_path = repositories_root.join("Personal").join("dotfiles");
        let machine = Self {
            cargo_binaries_directory: home_directory.join(".cargo").join("bin"),
            home_directory,
            repositories_root,
            dotfiles_repository_path,
            state: RefCell::new(MachineState {
                // Every shell but WSL ships with the machines that have it, and the fixed set of
                // scenarios only ever takes tools away.
                tools: BTreeSet::from([
                    Tool::Winget,
                    Tool::Cargo,
                    Tool::Claude,
                    Tool::Wsl,
                    Tool::Git,
                ]),
                ..MachineState::default()
            }),
        };

        machine.publish_release(
            own_release_repository(),
            ReleaseReading {
                version: Version::try_from(CONFIGURATOR_VERSION).expect("a version"),
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
                .join(own_currency().installed_name().as_ref()),
            format!("dotfiles_configurator {CONFIGURATOR_VERSION}"),
        );
        machine.hold_user_search_path_entry(machine.binaries_directory());
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

    pub fn cargo_workspace_reads(&self) -> Vec<PathBuf> {
        self.state.borrow().workspace_reads.clone()
    }

    pub fn cargo_binaries_directory(&self) -> &Path {
        &self.cargo_binaries_directory
    }

    pub fn repositories_root(&self) -> &Path {
        &self.repositories_root
    }

    pub fn dotfiles_repository_path(&self) -> &Path {
        &self.dotfiles_repository_path
    }

    pub fn execute_binary(&self, name: &str) {
        self.state.borrow_mut().executing_binaries.insert(
            self.cargo_binaries_directory.join(name),
            Displacement::Succeeds,
        );
    }

    pub fn execute_binary_that_cannot_be_displaced(&self, name: &str) {
        self.state.borrow_mut().executing_binaries.insert(
            self.cargo_binaries_directory.join(name),
            Displacement::Refused,
        );
    }

    pub fn leave_superseded_image(&self, name: &str) {
        let displaced = superseded_name(&self.cargo_binaries_directory.join(name));
        self.state.borrow_mut().superseded_images.insert(displaced);
    }

    pub fn superseded_image_count(&self) -> usize {
        self.state.borrow().superseded_images.len()
    }

    pub fn cargo_installs(&self) -> usize {
        self.state.borrow().cargo_installs
    }

    fn run_cargo(&self, invocation: &DisplacingInvocation) -> CommandOutput {
        let mut state = self.state.borrow_mut();
        state.cargo_installs += 1;

        let crate_name = invocation
            .arguments()
            .last()
            .map(|name| CrateName::from(name.as_str()));

        if let Some(destination) = state.executing_binaries.keys().next().cloned() {
            return CommandOutput {
                succeeded: false,
                standard_output: String::new(),
                standard_error: format!(
                    "error: failed to move `{}` to `{}`\n\nCaused by:\n  Access is denied. \
                     (os error 5)",
                    destination.with_extension("tmp").display(),
                    destination.display()
                ),
            };
        }

        if let Some(crate_name) = crate_name {
            for reading in state.cargo_workspaces.values_mut() {
                let Some(member) = reading.members.get_mut(&crate_name) else {
                    continue;
                };
                member.installed = Some(member.desired.clone());
                member.absent_binaries.clear();
            }
        }

        CommandOutput {
            succeeded: true,
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
        self.binary_reports(
            &self
                .binaries_directory()
                .join(own_currency().installed_name().as_ref()),
        )
    }

    pub fn publish_release(&self, repository: GitHubRepository, reading: ReleaseReading) {
        self.state.borrow_mut().releases.insert(repository, reading);
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
        self.state
            .borrow()
            .install_attempts
            .iter()
            .find(|(attempted, _)| attempted == name)
            .map(|(_, account)| account.clone())
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

    pub fn user_search_path(&self) -> Vec<PathBuf> {
        self.state.borrow().user_search_path.clone()
    }

    pub fn clear_the_search_path(&self) {
        let mut state = self.state.borrow_mut();
        state.user_search_path.clear();
        state.machine_search_path.clear();
    }

    pub fn hold_machine_manifest(&self, machine: MachineClass) {
        let manifest = MachineManifest {
            repositories_directory_path: self.repositories_root.join(machine.repositories_leaf()),
        };
        let document = machine_manifest_document(&manifest).expect("a manifest that serialises");

        self.write_text_file(&machine_manifest_path(self), &document)
            .expect("a manifest on the fake machine");
    }

    pub fn forget_the_machine_manifest(&self) {
        let path = machine_manifest_path(self);
        let mut state = self.state.borrow_mut();
        state.text_files.remove(&path);
        state.paths.remove(&path);
    }

    pub fn machine_manifest(&self) -> Option<String> {
        self.text_file_at(&machine_manifest_path(self))
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
            .filter(|(attempted, _)| attempted == name)
            .count()
    }

    /// A real file a person put there, which convergence must leave alone.
    pub fn add_own_file(&self, path: PathBuf) {
        self.state.borrow_mut().paths.insert(path);
    }

    /// Intact means still a real file rather than something a link now stands in for.
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

    pub fn dotfiles_repository_is_cloned(&self) -> bool {
        self.state
            .borrow()
            .paths
            .contains(&self.dotfiles_repository_path.join(".git"))
    }

    pub fn link_at(&self, link_path: &Path) -> Option<PathBuf> {
        self.state.borrow().links.get(link_path).cloned()
    }

    /// Everything a scenario could observe as "the machine changed", collapsed into one value so
    /// a scenario can assert that planning changed nothing at all.
    pub fn fingerprint(&self) -> String {
        let state = self.state.borrow();
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            state.paths,
            state.links,
            state.installed_applications,
            state.winget_packages,
            state.commands_run,
            state.user_search_path,
            state.environment_variables
        )
    }
}

/// Cloning puts the repository's directory, the marker that makes it a clone, and everything it
/// holds onto the machine at once.
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

/// Renders what winget reports, in the fixed-width shape the real one emits: a header naming each
/// column, a rule beneath it, and every package on a line of its own. Columns are sized to the
/// widest value, which is what winget does whenever its output is redirected.
fn winget_listing(packages: &BTreeSet<WingetPackageId>) -> String {
    /// Every row carries the same name, so the name column is only ever as wide as this.
    const PACKAGE_NAME: &str = "A package";
    const VERSION: &str = "1.0.0";

    let identifiers: Vec<String> = packages.iter().map(WingetPackageId::to_string).collect();
    let width = |heading: &str, widest_value: usize| heading.len().max(widest_value) + 2;
    let name_width = width("Name", PACKAGE_NAME.len());
    let id_width = width(
        "Id",
        identifiers
            .iter()
            .map(|identifier| identifier.chars().count())
            .max()
            .unwrap_or_default(),
    );

    let mut listing = format!("{:name_width$}{:id_width$}{}\n", "Name", "Id", "Version");
    listing.push_str(&"-".repeat(name_width + id_width + "Version".len()));
    listing.push('\n');
    for identifier in identifiers {
        listing.push_str(&format!(
            "{PACKAGE_NAME:name_width$}{identifier:id_width$}{VERSION}\n"
        ));
    }
    listing
}

impl FakeMachine {
    /// How many times a source was interrogated, which is what makes "read once per change set" a
    /// property a test can hold the program to rather than one it takes on trust.
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

    fn superseded_images(&self) -> Vec<PathBuf> {
        self.state
            .borrow()
            .superseded_images
            .iter()
            .cloned()
            .collect()
    }

    fn path_exists(&self, path: &Path) -> bool {
        let state = self.state.borrow();
        state.paths.contains(path) || state.links.contains_key(path)
    }

    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        self.state.borrow().links.get(path).cloned()
    }

    fn text_file_at(&self, path: &Path) -> Option<String> {
        self.state.borrow().text_files.get(path).cloned()
    }

    fn tool_is_present(&self, tool: Tool) -> bool {
        self.state.borrow().tools.contains(&tool)
    }

    fn read(&self, invocation: &ReadInvocation) -> Result<CommandOutput> {
        self.state.borrow_mut().reads.push(invocation.clone());

        let (succeeded, standard_output) = match invocation {
            ReadInvocation::WingetInstalledPackages => {
                (true, winget_listing(&self.state.borrow().winget_packages))
            }
            ReadInvocation::CargoInstalledCrates => (true, String::new()),
            ReadInvocation::ClaudeMcpServer { .. } => (false, String::new()),
        };

        Ok(CommandOutput {
            succeeded,
            standard_output,
            standard_error: String::new(),
        })
    }

    fn read_cargo_workspace(
        &self,
        repository_path: &Path,
        _installed: &BTreeMap<CrateName, Revision>,
    ) -> Result<Option<WorkspaceReading>> {
        let is_cloned = self.path_exists(&repository_path.join(".git"));

        let mut state = self.state.borrow_mut();
        state.workspace_reads.push(repository_path.to_path_buf());
        if !is_cloned {
            return Ok(None);
        }

        Ok(state.cargo_workspaces.get(repository_path).cloned())
    }

    fn check_presence(&self, check: &PresenceCheck) -> Result<bool> {
        match check {
            PresenceCheck::PathExists { path } => {
                Ok(self.path_exists(&self.resolve_against_home(path)))
            }
            PresenceCheck::CommandOnPath { command } => Ok(self
                .state
                .borrow()
                .installed_applications
                .iter()
                .any(|installed| installed.to_string() == *command)),
            PresenceCheck::CommandOutputContains { .. } => Ok(false),
        }
    }

    async fn latest_release(
        &self,
        repository: &GitHubRepository,
        account: &GitHubAccount,
    ) -> Result<ReleaseReading> {
        let mut state = self.state.borrow_mut();
        state
            .release_reads
            .push((repository.clone(), account.clone()));

        state
            .releases
            .get(repository)
            .cloned()
            .ok_or_else(|| anyhow!("{repository} has published no release"))
    }

    fn read_search_path(&self) -> Result<SearchPathReading> {
        let state = self.state.borrow();

        Ok(SearchPathReading::of(
            state
                .user_search_path
                .iter()
                .chain(state.machine_search_path.iter())
                .cloned(),
        ))
    }

    fn read_environment_variable(&self, name: &VariableName) -> Result<Option<VariableValue>> {
        Ok(self.state.borrow().environment_variables.get(name).cloned())
    }

    fn report_version(&self, binary_path: &Path, _arguments: &[String]) -> Result<CommandOutput> {
        let state = self.state.borrow();
        let Some(printed) = state.version_output_by_binary_path.get(binary_path) else {
            bail!("{} is not a program on this machine", binary_path.display());
        };

        Ok(CommandOutput {
            succeeded: true,
            standard_output: printed.clone(),
            standard_error: String::new(),
        })
    }
}

impl WriteMachine for FakeMachine {
    fn create_link(&self, link_path: &Path, target_path: &Path) -> Result<()> {
        self.state
            .borrow_mut()
            .links
            .insert(link_path.to_path_buf(), target_path.to_path_buf());
        Ok(())
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
        repository: &GitHubRepository,
        clone_directory: &Path,
        account: &GitHubAccount,
    ) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.clones.push((repository.clone(), account.clone()));
        materialise_clone(&mut state, clone_directory);
        Ok(())
    }

    async fn install_application(
        &self,
        installer: &Installer,
        account: &GitHubAccount,
    ) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state
            .install_attempts
            .push((installer.name.clone(), account.clone()));

        if state.failing_applications.contains(&installer.name) {
            bail!("the installer for {} exited non-zero", installer.name);
        }
        if state.silent_applications.contains(&installer.name) {
            return Ok(());
        }

        state.installed_applications.insert(installer.name.clone());
        Ok(())
    }

    async fn install_released_binary(
        &self,
        binary: &ReleasedBinary,
        _asset: &ReleaseAsset,
    ) -> Result<Placement> {
        let installed_path = self
            .binaries_directory()
            .join(binary.installed_name().as_ref());
        let mut state = self.state.borrow_mut();

        let Some(release) = state.releases.get(&binary.repository) else {
            bail!("{} has published no release", binary.repository);
        };
        let reported = format!("{} {}", binary.installed_name(), release.version);

        state.paths.insert(installed_path.clone());
        state
            .version_output_by_binary_path
            .insert(installed_path, reported);
        Ok(Placement::Placed)
    }

    /// Membership is the postcondition on a real machine, so the substitute has to answer the same
    /// way: a directory the path already carries is not added a second time.
    fn put_on_search_path(&self, directory: &Path) -> Result<()> {
        let mut state = self.state.borrow_mut();
        let carried = SearchPathReading::of(state.user_search_path.iter().cloned());
        if !carried.carries(directory) {
            state.user_search_path.push(directory.to_path_buf());
        }
        Ok(())
    }

    fn set_environment_variable(&self, name: &VariableName, value: &VariableValue) -> Result<()> {
        self.state
            .borrow_mut()
            .environment_variables
            .insert(name.clone(), value.clone());
        Ok(())
    }

    fn write(&self, invocation: &WriteInvocation) -> Result<CommandOutput> {
        if let WriteInvocation::InstallWingetPackage { id } = invocation {
            self.state.borrow_mut().winget_packages.insert(id.clone());
        }

        Ok(CommandOutput {
            succeeded: true,
            standard_output: String::new(),
            standard_error: String::new(),
        })
    }

    fn write_displacing(&self, invocation: &DisplacingInvocation) -> Result<Placement> {
        let output = self.run_cargo(invocation);
        if output.succeeded {
            return Ok(Placement::Placed);
        }

        let Some(destination) = invocation.refused_destination(&output) else {
            bail!("{} failed", invocation.tool());
        };

        let mut state = self.state.borrow_mut();
        if state.executing_binaries.get(&destination) == Some(&Displacement::Refused) {
            return Ok(Placement::Held(destination));
        }

        state.executing_binaries.remove(&destination);
        state
            .superseded_images
            .insert(superseded_name(&destination));
        drop(state);

        match self.run_cargo(invocation).succeeded {
            true => Ok(Placement::Placed),
            false => bail!("cargo failed once the image in its way had been displaced"),
        }
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

    fn run_declared_command(&self, _shell: Shell, args: &[String]) -> Result<CommandOutput> {
        self.state.borrow_mut().commands_run.push(args.to_vec());
        Ok(CommandOutput {
            succeeded: true,
            standard_output: String::new(),
            standard_error: String::new(),
        })
    }
}

//! A machine held in memory, substituted for the real one wherever a test needs to say what is
//! true of a machine. Substituting a machine is ordinary use of the capability traits rather than
//! a mode of the program, which is what ADR 0006 traded the `--debug` directory relocation for.

// Each integration test file is its own crate and pulls this module in whole, so a helper only
// one of them needs reads as dead in the others.
#![allow(dead_code)]

use {
    anyhow::{Result, bail},
    dotfiles_configurator::{
        configuration::{
            Application, ApplicationName, CrateName, GitHubRepository, PresenceCheck, Shell,
            WingetPackageId,
        },
        machine::{
            CommandOutput, Placement, ReadInvocation, ReadMachine, Tool, WriteInvocation,
            WriteMachine,
            workspace_reading::{Revision, WorkspaceReading},
        },
    },
    std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
    },
};

#[derive(Debug)]
pub struct FakeMachine {
    home_directory: PathBuf,
    repositories_directory: PathBuf,
    dotfiles_repository_path: PathBuf,
    cargo_binaries_directory: PathBuf,
    state: RefCell<MachineState>,
}

#[derive(Debug, Default)]
struct MachineState {
    paths: BTreeSet<PathBuf>,
    links: BTreeMap<PathBuf, PathBuf>,
    tools: BTreeSet<Tool>,
    installed_applications: BTreeSet<ApplicationName>,
    winget_packages: BTreeSet<WingetPackageId>,
    failing_applications: BTreeSet<ApplicationName>,
    /// Installers that exit zero without putting anything on the machine.
    silent_applications: BTreeSet<ApplicationName>,
    install_attempts: BTreeMap<ApplicationName, usize>,
    commands_run: Vec<Vec<String>>,
    /// What the dotfiles repository holds, which only appears on the machine once it is cloned.
    repository_contents: BTreeSet<PathBuf>,
    reads: Vec<ReadInvocation>,
    cargo_workspaces: BTreeMap<PathBuf, WorkspaceReading>,
    workspace_reads: Vec<PathBuf>,
    /// Destinations a write is refused at, and whether displacing them is refused too.
    executing_binaries: BTreeMap<PathBuf, Displacement>,
    superseded_images: BTreeSet<PathBuf>,
    cargo_installs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Displacement {
    Succeeds,
    Refused,
}

impl Default for FakeMachine {
    fn default() -> Self {
        let home_directory = PathBuf::from("/home/alice");
        let repositories_directory = PathBuf::from("/repositories");
        let dotfiles_repository_path = repositories_directory.join("dotfiles");
        Self {
            cargo_binaries_directory: home_directory.join(".cargo").join("bin"),
            home_directory,
            repositories_directory,
            dotfiles_repository_path,
            state: RefCell::new(MachineState {
                // Every shell but WSL ships with the machines that have it, and the fixed set of
                // scenarios only ever takes tools away.
                tools: BTreeSet::from([Tool::Winget, Tool::Cargo, Tool::Claude, Tool::Wsl]),
                ..MachineState::default()
            }),
        }
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

    /// A binary the machine is executing, which a write to its path is refused at until it is
    /// displaced.
    pub fn execute_binary(&self, name: &str) {
        self.state.borrow_mut().executing_binaries.insert(
            self.cargo_binaries_directory.join(name),
            Displacement::Succeeds,
        );
    }

    /// A binary the machine is executing and refuses to let go of at all, so displacing it fails.
    pub fn execute_binary_that_cannot_be_displaced(&self, name: &str) {
        self.state.borrow_mut().executing_binaries.insert(
            self.cargo_binaries_directory.join(name),
            Displacement::Refused,
        );
    }

    pub fn leave_superseded_image(&self, name: &str) {
        self.state
            .borrow_mut()
            .superseded_images
            .insert(self.cargo_binaries_directory.join(name));
    }

    pub fn superseded_image_count(&self) -> usize {
        self.state.borrow().superseded_images.len()
    }

    pub fn cargo_installs(&self) -> usize {
        self.state.borrow().cargo_installs
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
            .get(name)
            .copied()
            .unwrap_or_default()
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
            "{:?}|{:?}|{:?}|{:?}|{:?}",
            state.paths,
            state.links,
            state.installed_applications,
            state.winget_packages,
            state.commands_run
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

    fn repositories_directory(&self) -> &Path {
        &self.repositories_directory
    }

    fn dotfiles_repository_path(&self) -> &Path {
        &self.dotfiles_repository_path
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
}

impl WriteMachine for FakeMachine {
    fn create_link(&self, link_path: &Path, target_path: &Path) -> Result<()> {
        self.state
            .borrow_mut()
            .links
            .insert(link_path.to_path_buf(), target_path.to_path_buf());
        Ok(())
    }

    async fn clone_repository(&self, repository: &GitHubRepository) -> Result<()> {
        let clone_directory = self
            .repositories_directory
            .join(repository.repository.as_ref());
        let mut state = self.state.borrow_mut();
        materialise_clone(&mut state, &clone_directory);
        Ok(())
    }

    async fn install_application(&self, application: &Application) -> Result<()> {
        let mut state = self.state.borrow_mut();
        *state
            .install_attempts
            .entry(application.name.clone())
            .or_default() += 1;

        if state.failing_applications.contains(&application.name) {
            bail!("the installer for {} exited non-zero", application.name);
        }
        if state.silent_applications.contains(&application.name) {
            return Ok(());
        }

        state
            .installed_applications
            .insert(application.name.clone());
        Ok(())
    }

    fn write(&self, invocation: &WriteInvocation) -> Result<CommandOutput> {
        let mut state = self.state.borrow_mut();
        let mut installing: Option<CrateName> = None;
        match invocation {
            WriteInvocation::InstallWingetPackage { id } => {
                state.winget_packages.insert(id.clone());
            }
            WriteInvocation::InstallCargoCrate { arguments } => {
                state.cargo_installs += 1;
                installing = arguments.last().map(|name| CrateName::from(name.as_str()));
            }
            WriteInvocation::RemoveClaudeMcpServer { .. }
            | WriteInvocation::AddClaudeMcpServer { .. } => {}
        }

        let Some(destination) = state.executing_binaries.keys().next().cloned() else {
            if let Some(crate_name) = installing {
                for reading in state.cargo_workspaces.values_mut() {
                    let Some(member) = reading.members.get_mut(&crate_name) else {
                        continue;
                    };
                    member.installed = Some(member.desired.clone());
                    member.absent_binaries.clear();
                }
            }

            return Ok(CommandOutput {
                succeeded: true,
                standard_output: String::new(),
                standard_error: String::new(),
            });
        };

        Ok(CommandOutput {
            succeeded: false,
            standard_output: String::new(),
            standard_error: format!(
                "error: failed to move `{}` to `{}`\n\nCaused by:\n  Access is denied. (os error 5)",
                destination.with_extension("tmp").display(),
                destination.display()
            ),
        })
    }

    fn write_displacing(&self, invocation: &WriteInvocation) -> Result<Placement> {
        let output = self.write(invocation)?;
        if output.succeeded {
            return Ok(Placement::Placed(output));
        }

        let Some(destination) = invocation.refused_destination(&output) else {
            bail!("{} failed", invocation.tool());
        };

        let mut state = self.state.borrow_mut();
        if state.executing_binaries.get(&destination) == Some(&Displacement::Refused) {
            return Ok(Placement::Held(destination));
        }

        state.executing_binaries.remove(&destination);
        let mut superseded = destination.clone().into_os_string();
        superseded.push(".superseded");
        state.superseded_images.insert(PathBuf::from(superseded));
        drop(state);

        self.write(invocation).map(Placement::Placed)
    }

    fn sweep_superseded_images(&self) {
        let mut state = self.state.borrow_mut();
        let held: BTreeSet<PathBuf> = state
            .executing_binaries
            .keys()
            .map(|path| {
                let mut name = path.clone().into_os_string();
                name.push(".superseded");
                PathBuf::from(name)
            })
            .collect();
        state.superseded_images.retain(|image| held.contains(image));
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

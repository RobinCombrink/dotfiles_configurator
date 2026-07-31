//! A machine held in memory, substituted for the real one wherever a test needs to say what is
//! true of a machine. Substituting a machine is ordinary use of the capability traits rather than
//! a mode of the program, which is what ADR 0006 traded the `--debug` directory relocation for.

use {
    anyhow::{Result, bail},
    dotfiles::{
        configuration::{Application, ApplicationName, PresenceCheck, Shell},
        machine::{
            CommandOutput, ReadInvocation, ReadMachine, Tool, WriteInvocation, WriteMachine,
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
    state: RefCell<MachineState>,
}

#[derive(Debug, Default)]
struct MachineState {
    paths: BTreeSet<PathBuf>,
    links: BTreeMap<PathBuf, PathBuf>,
    tools: BTreeSet<Tool>,
    installed_applications: BTreeSet<ApplicationName>,
    winget_packages: BTreeSet<String>,
    failing_applications: BTreeSet<ApplicationName>,
    /// Installers that exit zero without putting anything on the machine.
    silent_applications: BTreeSet<ApplicationName>,
    install_attempts: BTreeMap<ApplicationName, usize>,
    commands_run: Vec<Vec<String>>,
    /// What the dotfiles repository holds, which only appears on the machine once it is cloned.
    repository_contents: BTreeSet<PathBuf>,
}

impl Default for FakeMachine {
    fn default() -> Self {
        let home_directory = PathBuf::from("/home/alice");
        let repositories_directory = PathBuf::from("/repositories");
        let dotfiles_repository_path = repositories_directory.join("dotfiles");
        Self {
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
    pub fn remove_tool(&self, tool: Tool) {
        self.state.borrow_mut().tools.remove(&tool);
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
        let succeeded = match invocation {
            ReadInvocation::WingetPackage { id } => {
                self.state.borrow().winget_packages.contains(id)
            }
            ReadInvocation::CargoInstalledCrates | ReadInvocation::ClaudeMcpServer { .. } => false,
        };

        Ok(CommandOutput {
            succeeded,
            standard_output: String::new(),
            standard_error: String::new(),
        })
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

    async fn clone_repository(&self, _owner: &str, repo: &str) -> Result<()> {
        let clone_directory = self.repositories_directory.join(repo);
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
        if let WriteInvocation::InstallWingetPackage { id } = invocation {
            self.state.borrow_mut().winget_packages.insert(id.clone());
        }

        Ok(CommandOutput {
            succeeded: true,
            standard_output: String::new(),
            standard_error: String::new(),
        })
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

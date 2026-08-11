use {
    crate::{
        configuration::{
            Application, CargoPackage, CargoSource, Command, EnvironmentVariable, GitHubAccount,
            GitHubRepository, Package, Registration, ReleasedBinary, Resource, Symlink,
            WingetPackage,
        },
        convergence::{
            SourceReadings, machine_manifest_document, machine_manifest_path, search_path_directory,
        },
        desired_state::ResolvedResource,
        machine::{DisplacingInvocation, Placement, WriteInvocation, WriteMachine},
    },
    anyhow::{Context, Result, anyhow, bail},
    std::path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum Convergence {
    Converged,
    Held(PathBuf),
}

impl From<Placement> for Convergence {
    fn from(placement: Placement) -> Self {
        match placement {
            Placement::Placed => Convergence::Converged,
            Placement::Held(path) => Convergence::Held(path),
        }
    }
}

/// Closes the drift on one resource. Only ever called for a resource a state reader has just
/// reported as drifted.
pub async fn converge(
    resource: &ResolvedResource,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Convergence> {
    let closed = match resource.declared() {
        Resource::Package(Package::Cargo(package)) => {
            return converge_cargo_package(package, resource, machine, readings);
        }
        Resource::Repository(repository) => {
            converge_repository(
                repository,
                &resource.clone_directory(repository),
                machine,
                resource.account(),
            )
            .await
        }
        Resource::Application(Application::Installer(installer)) => machine
            .install_application(installer, resource.account())
            .await
            .with_context(|| format!("Could not install {}", installer.name)),
        Resource::Application(Application::ReleasedBinary(binary)) => {
            return converge_released_binary(binary, machine, readings)
                .await
                .map(Convergence::from)
                .with_context(|| format!("Could not install {}", binary.installed_name()));
        }
        Resource::Package(Package::Winget(package)) => converge_winget_package(package, machine),
        Resource::EnvironmentVariable(EnvironmentVariable::Variable(variable)) => machine
            .set_environment_variable(&variable.name, &variable.value)
            .with_context(|| format!("Could not set {}", variable.name)),
        Resource::EnvironmentVariable(EnvironmentVariable::SearchPathEntry(entry)) => {
            let directory = search_path_directory(entry, resource, machine);
            machine.put_on_search_path(&directory).with_context(|| {
                format!("Could not put {} on the search path", directory.display())
            })
        }
        Resource::Symlink(symlink) => converge_symlink(symlink, resource, machine),
        Resource::Registration(Registration::MachineManifest(manifest)) => {
            let path = machine_manifest_path(machine);
            let document = machine_manifest_document(manifest)?;
            machine
                .write_text_file(&path, &document)
                .with_context(|| format!("Could not write {}", path.display()))
        }
        Resource::Registration(Registration::ClaudeMcpServer(server)) => {
            let removal = WriteInvocation::RemoveClaudeMcpServer {
                name: server.name.clone(),
                scope: server.scope,
            };
            // A server registered with the wrong details has to go before the right ones can be
            // added, and one that was never registered makes the removal fail harmlessly.
            let _ = machine.write(&removal);
            machine
                .write(&WriteInvocation::AddClaudeMcpServer {
                    server: Box::new(server.clone()),
                })
                .map(|_| ())
        }
        Resource::Command(command) => converge_command(command, machine),
    };

    closed.map(|()| Convergence::Converged)
}

async fn converge_released_binary(
    binary: &ReleasedBinary,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Placement> {
    let released = readings
        .release_of(&binary.repository)
        .map_err(|reason| anyhow!("{reason}"))?;
    let asset = released
        .asset_matching(&binary.asset)
        .map_err(|refusal| anyhow!("{refusal}"))?;

    machine.install_released_binary(binary, asset).await
}

async fn converge_repository(
    repository: &GitHubRepository,
    clone_directory: &Path,
    machine: &impl WriteMachine,
    account: &GitHubAccount,
) -> Result<()> {
    machine
        .clone_repository(repository, clone_directory, account)
        .await
}

fn converge_winget_package(package: &WingetPackage, machine: &impl WriteMachine) -> Result<()> {
    machine
        .write(&WriteInvocation::InstallWingetPackage {
            id: package.id.clone(),
        })
        .map(|_| ())
}

fn converge_cargo_package(
    package: &CargoPackage,
    resource: &ResolvedResource,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Convergence> {
    let mut arguments = vec![
        "install".to_owned(),
        "--locked".to_owned(),
        "--force".to_owned(),
    ];
    match &package.source {
        CargoSource::Registry => arguments.push(package.crate_name.to_string()),
        CargoSource::Path { path } => {
            arguments.push("--path".to_owned());
            arguments.push(path.display().to_string());
        }
        CargoSource::Workspace { repository } => {
            let clone_directory = resource.clone_directory(repository);
            let Some(revision) = readings.workspace_revision(&clone_directory) else {
                bail!("{repository} was not read, so there is no revision to install from");
            };
            arguments.push("--git".to_owned());
            arguments.push(repository.fetch_url_as(resource.account()));
            arguments.push("--rev".to_owned());
            arguments.push(revision.to_string());
            arguments.push(package.crate_name.to_string());
        }
    }

    machine
        .write_displacing(&DisplacingInvocation::InstallCargoCrate { arguments })
        .map(Convergence::from)
}

fn converge_symlink(
    symlink: &Symlink,
    resource: &ResolvedResource,
    machine: &impl WriteMachine,
) -> Result<()> {
    let link_path = machine.resolve_against_home(&symlink.link_path);
    let source_path = resource.files_root().join(&symlink.source_path);

    if !machine.path_exists(&source_path) {
        bail!(
            "The dotfiles repository holds nothing at {}",
            source_path.display()
        );
    }

    // A link is this tool's own work, so one pointing elsewhere is replaced. Anything else at
    // that path was put there by a person, and convergence never makes undeclared things false.
    // See ADR 0005.
    if machine.link_target(&link_path).is_none() && machine.path_exists(&link_path) {
        bail!(
            "{} already exists and is not a link. Move it aside to let the dotfiles repository \
             own it; this tool will not delete something it did not create.",
            link_path.display()
        );
    }

    machine.create_link(&link_path, &source_path)
}

fn converge_command(command: &Command, machine: &impl WriteMachine) -> Result<()> {
    let output = machine.run_declared_command(command.shell, &command.args)?;
    match output.succeeded {
        true => Ok(()),
        false => bail!(
            "`{}` failed:\n{}\n{}",
            command.rendered(),
            output.standard_output.trim(),
            output.standard_error.trim()
        ),
    }
}

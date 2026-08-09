use {
    crate::{
        configuration::{
            Application, CargoPackage, CargoSource, Command, GitHubRepository, Package,
            Registration, ReleasedBinary, Resource, Symlink, WingetPackage,
        },
        convergence::SourceReadings,
        machine::{DisplacingInvocation, Placement, WriteInvocation, WriteMachine},
    },
    anyhow::{Context, Result, anyhow, bail},
    std::path::PathBuf,
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
    resource: &Resource,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Convergence> {
    let closed = match resource {
        Resource::Package(Package::Cargo(package)) => {
            return converge_cargo_package(package, machine, readings);
        }
        Resource::Repository(repository) => converge_repository(repository, machine).await,
        Resource::Application(Application::Installer(installer)) => machine
            .install_application(installer)
            .await
            .with_context(|| format!("Could not install {}", installer.name)),
        Resource::Application(Application::ReleasedBinary(binary)) => {
            converge_released_binary(binary, machine, readings)
                .await
                .with_context(|| format!("Could not install {}", binary.installed_name()))
        }
        Resource::Package(Package::Winget(package)) => converge_winget_package(package, machine),
        Resource::Symlink(symlink) => converge_symlink(symlink, machine),
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
) -> Result<()> {
    let released = match readings.release(&binary.repository) {
        Some(Ok(release)) => release,
        Some(Err(reason)) => bail!("{reason}"),
        None => bail!("{} was not read for its latest release", binary.repository),
    };
    let asset = released
        .asset_matching(&binary.asset)
        .map_err(|refusal| anyhow!("{refusal}"))?;

    machine.install_released_binary(binary, asset).await
}

async fn converge_repository(
    repository: &GitHubRepository,
    machine: &impl WriteMachine,
) -> Result<()> {
    machine.clone_repository(repository).await
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
            let Some(revision) = readings.workspace_revision(repository) else {
                bail!("{repository} was not read, so there is no revision to install from");
            };
            arguments.push("--git".to_owned());
            arguments.push(repository.clone_url());
            arguments.push("--rev".to_owned());
            arguments.push(revision.to_string());
            arguments.push(package.crate_name.to_string());
        }
    }

    machine
        .write_displacing(&DisplacingInvocation::InstallCargoCrate { arguments })
        .map(Convergence::from)
}

fn converge_symlink(symlink: &Symlink, machine: &impl WriteMachine) -> Result<()> {
    let link_path = machine.resolve_against_home(&symlink.link_path);
    let source_path = machine
        .dotfiles_repository_path()
        .join(&symlink.source_path);

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

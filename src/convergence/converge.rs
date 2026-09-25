use {
    crate::{
        configuration::{
            Application, ApplicationSource, CargoPackage, CargoSource, ClaudeMcpServer, Command,
            EnvironmentVariable, GitHubAccount, GitHubRepository, Installer, MachineManifest,
            Package, Registration, ReleasedBinary, Resource, Symlink, UvToolPackage, WingetPackage,
        },
        convergence::{SourceReadings, search_path_directory, symlink_location},
        desired_state::ResolvedResource,
        machine::{
            DisplacingInvocation, Placement, Replacement, ReplacingInvocation, ResolvedCargoSource,
            WriteInvocation, WriteMachine,
            release_reading::{ReleaseAsset, ReleaseReading},
        },
    },
    anyhow::{Context, Result, anyhow, bail},
    std::path::Path,
};

/// Closes the drift on one resource. Only ever called for a resource a state reader has just
/// reported as drifted.
pub async fn converge(
    resource: &ResolvedResource,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Placement> {
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
        Resource::Application(Application::Installer(installer)) => {
            let release_asset = resolved_release_asset(installer, readings)?;
            machine
                .install_application(installer, release_asset)
                .await
                .with_context(|| format!("Could not install {}", installer.name))
        }
        Resource::Application(Application::ReleasedBinary(binary)) => {
            return converge_released_binary(binary, machine, readings)
                .await
                .with_context(|| format!("Could not install {}", binary.installed_name()));
        }
        Resource::Package(Package::Winget(package)) => converge_winget_package(package, machine),
        Resource::Package(Package::UvTool(package)) => {
            converge_uv_tool(package, machine, readings)
        }
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
            let path = MachineManifest::path_within(machine.home_directory());
            let document = String::try_from(manifest)?;
            machine
                .write_text_file(&path, &document)
                .with_context(|| format!("Could not write {}", path.display()))
        }
        Resource::Registration(Registration::ClaudeMcpServer(server)) => {
            converge_claude_mcp_server(server, machine)
        }
        Resource::Command(command) => converge_command(command, machine),
    };

    closed.map(|()| Placement::Placed)
}

fn resolved_release_asset<'readings>(
    installer: &Installer,
    readings: &'readings SourceReadings,
) -> Result<Option<&'readings ReleaseAsset>> {
    let ApplicationSource::GitHubRelease {
        owner,
        repository,
        asset,
    } = &installer.source
    else {
        return Ok(None);
    };

    let repository = GitHubRepository {
        owner: owner.clone(),
        repository: repository.clone(),
    };
    let released = readings
        .release_of(&repository)
        .map_err(|impediment| anyhow!("{impediment}"))?
        .ok_or_else(|| anyhow!("{repository} has published no release"))?;
    let matched = released
        .asset_matching(asset)
        .map_err(|refusal| anyhow!("{refusal}"))?;

    Ok(Some(matched))
}

async fn converge_released_binary(
    binary: &ReleasedBinary,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Placement> {
    let released = readings
        .release_of(&binary.repository)
        .map_err(|impediment| anyhow!("{impediment}"))?
        .ok_or_else(|| anyhow!("{} has published no release", binary.repository))?;

    install_release(binary, released, machine).await
}

pub async fn install_release(
    binary: &ReleasedBinary,
    release: &ReleaseReading,
    machine: &impl WriteMachine,
) -> Result<Placement> {
    let asset = release
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

fn converge_claude_mcp_server(server: &ClaudeMcpServer, machine: &impl WriteMachine) -> Result<()> {
    let replacement = machine.replace(&ReplacingInvocation::ClaudeMcpServer {
        server: Box::new(server.clone()),
    })?;

    match replacement {
        Replacement::Replaced => Ok(()),
        Replacement::RemovedButCouldNotAdd { name, cause } => Err(cause.context(format!(
            "The registration claude held for {name} was removed to make way for the declared \
             one, which could not be added, so claude now holds no server under that name"
        ))),
    }
}

fn converge_winget_package(package: &WingetPackage, machine: &impl WriteMachine) -> Result<()> {
    machine
        .write(&WriteInvocation::InstallWingetPackage {
            id: package.id.clone(),
        })
        .map(|_| ())
}

fn converge_uv_tool(
    package: &UvToolPackage,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<()> {
    let installed = readings
        .installed_uv_tool(&package.name)
        .map_err(|impediment| anyhow!("{impediment}"))?;
    let invocation = match installed {
        Some(_) => WriteInvocation::UpgradeUvTool {
            name: package.name.clone(),
        },
        None => WriteInvocation::InstallUvTool {
            name: package.name.clone(),
            python: package.python.clone(),
        },
    };

    machine.write(&invocation).map(|_| ())
}

fn converge_cargo_package(
    package: &CargoPackage,
    resource: &ResolvedResource,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Placement> {
    let source = match &package.source {
        CargoSource::Registry => ResolvedCargoSource::Registry,
        CargoSource::Path { path } => ResolvedCargoSource::Path { path: path.clone() },
        CargoSource::Workspace { repository } => {
            let clone_directory = resource.clone_directory(repository);
            let revision = match readings.workspace(&clone_directory) {
                Ok(Some(reading)) => reading.revision.clone(),
                Ok(None) => bail!(
                    "{repository} has not been cloned, so there is no revision to install from"
                ),
                Err(impediment) => bail!(
                    "{repository} could not be read, so there is no revision to install from: \
                     {impediment}"
                ),
            };

            ResolvedCargoSource::Repository {
                repository: repository.clone(),
                account: resource.account().clone(),
                revision,
            }
        }
    };

    machine.write_displacing(&DisplacingInvocation::InstallCargoCrate {
        crate_name: package.crate_name.clone(),
        source,
    })
}

fn converge_symlink(
    symlink: &Symlink,
    resource: &ResolvedResource,
    machine: &impl WriteMachine,
) -> Result<()> {
    let (link_path, source_path) = symlink_location(symlink, resource, machine);

    if !machine.path_exists(&source_path) {
        bail!(
            "The dotfiles repository holds nothing at {}",
            source_path.display()
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

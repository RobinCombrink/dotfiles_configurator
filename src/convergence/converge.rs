use {
    crate::{
        configuration::{
            Application, ApplicationSource, CargoPackage, CargoSource, ClaudeMcpServer, Command,
            EnvironmentVariable, GitHubAccount, GitHubRepository, Installer, MachineManifest,
            Package, PackageCurrency, Registration, ReleasedBinary, RepositoryClone, Resource,
            Symlink, UvToolPackage, WingetPackage,
        },
        convergence::{Change, SourceReadings, search_path_directory, symlink_location},
        desired_state::ResolvedResource,
        machine::{
            DisplacingInvocation, Downloaded, Placement, Replacement, ReplacingInvocation,
            ResolvedCargoSource, WriteInvocation, WriteMachine,
            release_reading::{ReleaseAsset, ReleaseReading},
        },
    },
    anyhow::{Context, Result, anyhow, bail},
    std::path::Path,
};

pub async fn converge(
    drifted: &Change,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Placement> {
    let resource = &drifted.resource;
    let closed = match resource.declared() {
        Resource::Package(Package::Cargo(package)) => {
            return converge_cargo_package(package, resource, machine, readings).await;
        }
        Resource::Repository(clone) => {
            converge_repository(
                clone,
                &resource.clone_directory(&clone.repository),
                machine,
                resource.account(),
            )
            .await
        }
        Resource::Application(Application::Installer(installer)) => {
            return Download::Installer(installer)
                .fetched(machine, readings)
                .await?
                .installed(machine)
                .await;
        }
        Resource::Application(Application::ReleasedBinary(binary)) => {
            return Download::ReleasedBinary(binary)
                .fetched(machine, readings)
                .await?
                .installed(machine)
                .await;
        }
        Resource::Package(Package::Winget(package)) => {
            return converge_winget_package(package, machine, readings).await;
        }
        Resource::Package(Package::UvTool(package)) => {
            return converge_uv_tool(package, machine, readings).await;
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
        Resource::Symlink(symlink) => return converge_symlink(symlink, resource, machine),
        Resource::Registration(Registration::MachineManifest(manifest)) => {
            let path = MachineManifest::path_within(machine.home_directory());
            machine
                .write_text_file(&path, manifest.document())
                .with_context(|| format!("Could not write {}", path.display()))
        }
        Resource::Registration(Registration::ClaudeMcpServer(server)) => {
            converge_claude_mcp_server(server, machine).await
        }
        Resource::Command(command) => converge_command(command, machine).await,
    };

    closed.map(|()| Placement::Placed)
}

#[derive(Debug, Clone, Copy)]
pub enum Download<'resource> {
    Installer(&'resource Installer),
    ReleasedBinary(&'resource ReleasedBinary),
}

#[derive(Debug)]
pub enum Fetched {
    Installer(Downloaded<Installer>),
    ReleasedBinary(Downloaded<ReleasedBinary>),
}

impl<'resource> Download<'resource> {
    pub fn of(resource: &'resource Resource) -> Option<Self> {
        match resource {
            Resource::Application(Application::Installer(installer)) => {
                Some(Download::Installer(installer))
            }
            Resource::Application(Application::ReleasedBinary(binary)) => {
                Some(Download::ReleasedBinary(binary))
            }
            Resource::Repository(_)
            | Resource::Package(_)
            | Resource::EnvironmentVariable(_)
            | Resource::Symlink(_)
            | Resource::Registration(_)
            | Resource::Command(_) => None,
        }
    }

    pub async fn fetched(
        self,
        machine: &impl WriteMachine,
        readings: &SourceReadings,
    ) -> Result<Fetched> {
        match self {
            Download::Installer(installer) => download_installer(installer, machine, readings)
                .await
                .map(Fetched::Installer),
            Download::ReleasedBinary(binary) => download_released_binary(binary, machine, readings)
                .await
                .map(Fetched::ReleasedBinary),
        }
    }
}

impl Fetched {
    pub async fn installed(self, machine: &impl WriteMachine) -> Result<Placement> {
        match self {
            Fetched::Installer(downloaded) => install_application(downloaded, machine).await,
            Fetched::ReleasedBinary(downloaded) => install_released_binary(downloaded, machine),
        }
    }
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

async fn download_installer(
    installer: &Installer,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Downloaded<Installer>> {
    let release_asset = resolved_release_asset(installer, readings)?;
    machine
        .download_installer(installer, release_asset)
        .await
        .with_context(|| format!("Could not install {}", installer.name))
}

async fn install_application(
    downloaded: Downloaded<Installer>,
    machine: &impl WriteMachine,
) -> Result<Placement> {
    let name = downloaded.declared().name.clone();
    machine
        .install_application(downloaded)
        .await
        .with_context(|| format!("Could not install {name}"))
}

async fn download_released_binary(
    binary: &ReleasedBinary,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Downloaded<ReleasedBinary>> {
    let released = readings
        .release_of(&binary.repository)
        .map_err(|impediment| anyhow!("{impediment}"))
        .and_then(|released| {
            released.ok_or_else(|| anyhow!("{} has published no release", binary.repository))
        })
        .with_context(|| format!("Could not install {}", binary.installed_name()))?;

    download_release(binary, released, machine).await
}

async fn download_release(
    binary: &ReleasedBinary,
    release: &ReleaseReading,
    machine: &impl WriteMachine,
) -> Result<Downloaded<ReleasedBinary>> {
    let asset = release
        .asset_matching(&binary.asset)
        .map_err(|refusal| anyhow!("{refusal}"))
        .with_context(|| format!("Could not install {}", binary.installed_name()))?;

    machine
        .download_released_binary(binary, asset)
        .await
        .with_context(|| format!("Could not install {}", binary.installed_name()))
}

fn install_released_binary(
    downloaded: Downloaded<ReleasedBinary>,
    machine: &impl WriteMachine,
) -> Result<Placement> {
    let installed_name = downloaded.declared().installed_name();
    machine
        .install_released_binary(downloaded)
        .with_context(|| format!("Could not install {installed_name}"))
}

pub async fn install_release(
    binary: &ReleasedBinary,
    release: &ReleaseReading,
    machine: &impl WriteMachine,
) -> Result<Placement> {
    let downloaded = download_release(binary, release, machine).await?;
    install_released_binary(downloaded, machine)
}

async fn converge_repository(
    clone: &RepositoryClone,
    clone_directory: &Path,
    machine: &impl WriteMachine,
    account: &GitHubAccount,
) -> Result<()> {
    if machine.path_exists(&clone_directory.join(".git")) {
        return machine
            .deepen_clone(&clone.repository, clone_directory, account)
            .await;
    }

    machine
        .clone_repository(clone, clone_directory, account)
        .await
}

async fn converge_claude_mcp_server(
    server: &ClaudeMcpServer,
    machine: &impl WriteMachine,
) -> Result<()> {
    let replacement = machine
        .replace(&ReplacingInvocation::ClaudeMcpServer {
            server: Box::new(server.clone()),
        })
        .await?;

    match replacement {
        Replacement::Replaced => Ok(()),
        Replacement::RemovedButCouldNotAdd { name, cause } => Err(cause.context(format!(
            "The registration claude held for {name} was removed to make way for the declared \
             one, which could not be added, so claude now holds no server under that name"
        ))),
    }
}

async fn converge_winget_package(
    package: &WingetPackage,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Placement> {
    let invocation = match &package.version {
        PackageCurrency::Exactly(version) => WriteInvocation::InstallWingetPackage {
            id: package.id.clone(),
            version: Some(version.clone()),
        },
        PackageCurrency::Latest => match readings
            .winget_upgrade_of(&package.id)
            .map_err(|impediment| anyhow!("{impediment}"))?
        {
            Some(_) => WriteInvocation::UpgradeWingetPackage {
                id: package.id.clone(),
            },
            None => WriteInvocation::InstallWingetPackage {
                id: package.id.clone(),
                version: None,
            },
        },
    };

    let output = machine.attempt_write(&invocation).await?;
    if output.exited.succeeded() {
        return Ok(Placement::Placed);
    }
    if let Some(reason) = invocation.held_by(&output) {
        return Ok(Placement::Held(reason));
    }

    bail!(
        "winget {} failed, {}:\n{}\n{}",
        invocation.arguments().join(" "),
        output.exited,
        output.standard_output.trim(),
        output.standard_error.trim()
    )
}

async fn converge_uv_tool(
    package: &UvToolPackage,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Placement> {
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

    machine.write_over_running_images(&invocation).await
}

async fn converge_cargo_package(
    package: &CargoPackage,
    resource: &ResolvedResource,
    machine: &impl WriteMachine,
    readings: &SourceReadings,
) -> Result<Placement> {
    let source = match &package.source {
        CargoSource::Registry { version } => ResolvedCargoSource::Registry {
            version: version.exact().cloned(),
        },
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

    machine
        .write_displacing(&DisplacingInvocation::InstallCargoCrate {
            crate_name: package.crate_name.clone(),
            source,
        })
        .await
}

fn converge_symlink(
    symlink: &Symlink,
    resource: &ResolvedResource,
    machine: &impl WriteMachine,
) -> Result<Placement> {
    let (link_path, source_path) = symlink_location(symlink, resource, machine);

    if !machine.path_exists(&source_path) {
        bail!(
            "The dotfiles repository holds nothing at {}",
            source_path.display()
        );
    }

    machine.create_link(&link_path, &source_path)
}

async fn converge_command(command: &Command, machine: &impl WriteMachine) -> Result<()> {
    let output = machine
        .run_declared_command(command.shell, &command.args)
        .await?;
    match output.exited.succeeded() {
        true => Ok(()),
        false => bail!(
            "`{}` failed, {}:\n{}\n{}",
            command.rendered(),
            output.exited,
            output.standard_output.trim(),
            output.standard_error.trim()
        ),
    }
}

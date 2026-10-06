use {
    crate::{
        configuration::{
            Application, ApplicationSource, BinaryName, CargoPackage, CargoSource, ClaudeMcpServer,
            Command, CrateName, CrateVersion, EnvironmentVariable, GitHubAccount, GitHubRepository,
            Installer, MachineManifest, Package, PackageCurrency, Registration, ReleasedBinary,
            RenderedManifest, RepositoryClone, Requirement, Resource, SearchPathEntry, Symlink,
            Tool, UvToolName, UvToolPackage, UvToolVersion, Variable, WingetPackage,
            WingetPackageId, WingetVersion,
        },
        convergence::{
            Assessment, Impediment, ReadSource, SourceReading, UnreadableReason,
            member_currency::{OwnCopies, OwnCopy, judged, own_copies_of, resolution_of},
            search_path_directory, symlink_location,
            withdrawal::{InstallRecord, install_records},
        },
        desired_state::{DesiredState, ResolvedResource},
        machine::{
            CommandOutput, Exited, ReadInvocation, ReadMachine,
            environment_reading::SearchPathReading, release_reading::ReleaseReading,
            workspace_reading::WorkspaceReading,
        },
        version::Version,
    },
    std::{
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
    },
};

// ADR 0010
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReadings {
    winget_packages: SourceReading<String>,
    winget_upgrades: SourceReading<WingetUpgradeListing>,
    cargo_crates: SourceReading<String>,
    uv_tools: SourceReading<String>,
    uv_outdated_tools: SourceReading<String>,
    workspaces: BTreeMap<PathBuf, SourceReading<Option<WorkspaceReading>>>,
    own_copies: BTreeMap<PathBuf, OwnCopies>,
    releases: BTreeMap<GitHubRepository, SourceReading<Option<ReleaseReading>>>,
    newest_crates: BTreeMap<CrateName, SourceReading<CrateVersion>>,
    search_path: SourceReading<SearchPathReading>,
}

impl SourceReadings {
    pub async fn read_for(desired_state: &DesiredState, machine: &impl ReadMachine) -> Self {
        let mut winget_is_needed = false;
        let mut winget_upgrades_are_needed = false;
        let mut uv_is_needed = false;
        let mut cargo_is_needed = !desired_state.workspaces.is_empty();
        let mut search_path_is_needed = false;
        let mut released_from: BTreeMap<GitHubRepository, GitHubAccount> = BTreeMap::new();
        let mut kept_at_newest: BTreeSet<CrateName> = BTreeSet::new();
        for resource in &desired_state.resources {
            match resource.declared() {
                Resource::Package(Package::Winget(package)) => {
                    winget_is_needed = true;
                    winget_upgrades_are_needed |= package.version.is_latest();
                }
                Resource::Package(Package::Cargo(package)) => {
                    cargo_is_needed = true;
                    if let CargoSource::Registry {
                        version: PackageCurrency::Latest,
                    } = package.source
                    {
                        kept_at_newest.insert(package.crate_name.clone());
                    }
                }
                Resource::Package(Package::UvTool(_)) => uv_is_needed = true,
                Resource::EnvironmentVariable(EnvironmentVariable::SearchPathEntry(_)) => {
                    search_path_is_needed = true;
                }
                Resource::Application(Application::ReleasedBinary(binary)) => {
                    released_from
                        .entry(binary.repository.clone())
                        .or_insert_with(|| resource.account().clone());
                }
                Resource::Application(Application::Installer(installer)) => {
                    if let ApplicationSource::GitHubRelease {
                        owner, repository, ..
                    } = &installer.source
                    {
                        released_from
                            .entry(GitHubRepository {
                                owner: owner.clone(),
                                repository: repository.clone(),
                            })
                            .or_insert_with(|| resource.account().clone());
                    }
                }
                Resource::Repository(_)
                | Resource::EnvironmentVariable(EnvironmentVariable::Variable(_))
                | Resource::Symlink(_)
                | Resource::Registration(_)
                | Resource::Command(_) => {}
            }
        }

        let mut releases = BTreeMap::new();
        for (repository, account) in released_from {
            let reading = match machine.latest_release(&repository, &account).await {
                Ok(release) => SourceReading::Read(release),
                Err(error) => SourceReading::Unreadable(format!("{error:#}").into()),
            };
            releases.insert(repository, reading);
        }

        let mut newest_crates = BTreeMap::new();
        for crate_name in kept_at_newest {
            let reading = match machine.newest_published_crate(&crate_name).await {
                Ok(newest) => SourceReading::Read(newest),
                Err(error) => SourceReading::Unreadable(format!("{error:#}").into()),
            };
            newest_crates.insert(crate_name, reading);
        }

        let mut workspaces = BTreeMap::new();
        let mut own_copies = BTreeMap::new();
        for workspace in &desired_state.workspaces {
            let repository_path = workspace.clone_directory(&workspace.declared().repository);
            let reading = match machine.read_cargo_workspace(&repository_path) {
                Ok(reading) => SourceReading::Read(reading),
                Err(error) => SourceReading::Unreadable(format!("{error:#}").into()),
            };
            if let SourceReading::Read(Some(reading)) = &reading {
                own_copies.insert(
                    repository_path.clone(),
                    own_copies_of(reading, &repository_path, machine),
                );
            }
            workspaces.insert(repository_path, reading);
        }
        search_path_is_needed |= !desired_state.workspaces.is_empty();

        Self {
            winget_packages: read_listing(
                winget_is_needed,
                ReadInvocation::WingetInstalledPackages,
                machine,
            ),
            winget_upgrades: read_winget_upgrades(winget_upgrades_are_needed, machine),
            cargo_crates: read_listing(
                cargo_is_needed,
                ReadInvocation::CargoInstalledCrates,
                machine,
            ),
            uv_tools: read_listing(uv_is_needed, ReadInvocation::UvInstalledTools, machine),
            uv_outdated_tools: read_listing(uv_is_needed, ReadInvocation::UvOutdatedTools, machine),
            workspaces,
            own_copies,
            releases,
            newest_crates,
            search_path: match search_path_is_needed {
                false => SourceReading::NotRequested(ReadSource::SearchPath),
                true => match machine.read_search_path() {
                    Ok(reading) => SourceReading::Read(reading),
                    Err(error) => SourceReading::Unreadable(format!("{error:#}").into()),
                },
            },
        }
    }

    pub fn search_path(&self) -> Result<&SearchPathReading, Impediment> {
        self.search_path.read()
    }

    /// The version uv reports a tool installed at, or `None` where uv has not installed it.
    ///
    /// ```no_run
    /// # use dotfiles_configurator::{
    /// #     configuration::UvToolName, convergence::SourceReadings,
    /// # };
    /// # fn describe(readings: &SourceReadings) -> String {
    /// match readings.installed_uv_tool(&UvToolName::from("serena-agent")) {
    ///     Ok(Some(version)) => format!("installed at {version}"),
    ///     Ok(None) => "not installed".to_owned(),
    ///     Err(impediment) => impediment.to_string(),
    /// }
    /// # }
    /// ```
    pub fn installed_uv_tool(
        &self,
        name: &UvToolName,
    ) -> Result<Option<UvToolVersion>, Impediment> {
        let listing = self.uv_tools.read()?;
        listed_uv_tool(listing, name)
            .map(|listed| listed.map(|(installed, _)| installed))
            .map_err(Impediment::ActualStateUnreadable)
    }

    /// The newer version a tool would be upgraded to, or `None` where no newer one resolves.
    ///
    /// ```no_run
    /// # use dotfiles_configurator::{
    /// #     configuration::UvToolName, convergence::SourceReadings,
    /// # };
    /// # fn describe(readings: &SourceReadings) -> String {
    /// match readings.newer_uv_tool(&UvToolName::from("serena-agent")) {
    ///     Ok(Some(latest)) => format!("{latest} resolves"),
    ///     Ok(None) => "nothing newer resolves".to_owned(),
    ///     Err(impediment) => impediment.to_string(),
    /// }
    /// # }
    /// ```
    pub fn newer_uv_tool(&self, name: &UvToolName) -> Result<Option<UvToolVersion>, Impediment> {
        let listing = self.uv_outdated_tools.read()?;
        match listed_uv_tool(listing, name).map_err(Impediment::ActualStateUnreadable)? {
            None => Ok(None),
            Some((_, Some(latest))) => Ok(Some(latest)),
            Some((_, None)) => Err(Impediment::ActualStateUnreadable(
                format!("uv listed {name} as behind without naming the version it is behind")
                    .into(),
            )),
        }
    }

    /// The latest release of a repository, or the typed absence of one where the repository has
    /// published nothing at all.
    ///
    /// ```no_run
    /// # use dotfiles_configurator::{
    /// #     configuration::GitHubRepository, convergence::SourceReadings,
    /// # };
    /// # fn version_of(readings: &SourceReadings, repository: &GitHubRepository) -> String {
    /// match readings.release_of(repository) {
    ///     Ok(Some(release)) => release.version.to_string(),
    ///     Ok(None) => "nothing published".to_owned(),
    ///     Err(impediment) => impediment.to_string(),
    /// }
    /// # }
    /// ```
    pub fn release_of(
        &self,
        repository: &GitHubRepository,
    ) -> Result<Option<&ReleaseReading>, Impediment> {
        match self.releases.get(repository) {
            Some(reading) => reading.read().map(Option::as_ref),
            None => Err(ReadSource::LatestRelease(repository.clone()).was_not_read()),
        }
    }

    /// The newer version winget would upgrade a package to, or `None` where it offers none.
    ///
    /// ```no_run
    /// # use dotfiles_configurator::{
    /// #     configuration::WingetPackageId, convergence::SourceReadings,
    /// # };
    /// # fn describe(readings: &SourceReadings) -> String {
    /// match readings.winget_upgrade_of(&WingetPackageId::from("jqlang.jq")) {
    ///     Ok(Some(offered)) => format!("winget offers {offered}"),
    ///     Ok(None) => "winget offers nothing newer".to_owned(),
    ///     Err(impediment) => impediment.to_string(),
    /// }
    /// # }
    /// ```
    pub fn winget_upgrade_of(
        &self,
        id: &WingetPackageId,
    ) -> Result<Option<WingetVersion>, Impediment> {
        let listing = match self.winget_upgrades.read()? {
            WingetUpgradeListing::NothingToUpgrade => return Ok(None),
            WingetUpgradeListing::Listed(listing) => listing,
        };

        match winget_row(listing, id.as_ref()).map_err(Impediment::ActualStateUnreadable)? {
            None => Ok(None),
            Some(WingetRow {
                available: Some(offered),
                ..
            }) => Ok(Some(WingetVersion::from(offered))),
            Some(WingetRow {
                available: None, ..
            }) => Err(Impediment::ActualStateUnreadable(
                format!("winget lists an upgrade of {id} without the version it offers").into(),
            )),
        }
    }

    pub fn newest_crate(&self, crate_name: &CrateName) -> Result<&CrateVersion, Impediment> {
        match self.newest_crates.get(crate_name) {
            Some(reading) => reading.read(),
            None => Err(ReadSource::CratesIndex(crate_name.clone()).was_not_read()),
        }
    }

    pub fn workspace(
        &self,
        clone_directory: &Path,
    ) -> Result<Option<&WorkspaceReading>, Impediment> {
        match self.workspaces.get(clone_directory) {
            Some(reading) => reading.read().map(Option::as_ref),
            None => Err(ReadSource::CargoWorkspace(clone_directory.to_path_buf()).was_not_read()),
        }
    }

    // ADR 0007
    pub fn resolved_workspace(
        &self,
        clone_directory: &Path,
    ) -> Result<&WorkspaceReading, Impediment> {
        match self.workspace(clone_directory)? {
            Some(reading) => Ok(reading),
            None => Err(Impediment::ActualStateUnreadable(
                format!(
                    "{} holds no clone, so its members cannot be read",
                    clone_directory.display()
                )
                .into(),
            )),
        }
    }

    // ADR 0041
    pub fn install_records(&self) -> Result<Vec<InstallRecord>, Impediment> {
        self.cargo_crates
            .read()
            .map(|listing| install_records(listing))
    }

    // ADR 0040
    fn own_copies(
        &self,
        clone_directory: &Path,
        crate_name: &CrateName,
    ) -> Result<&[(BinaryName, OwnCopy)], Impediment> {
        self.own_copies
            .get(clone_directory)
            .and_then(|members| members.get(crate_name))
            .map(Vec::as_slice)
            .ok_or_else(|| ReadSource::CargoWorkspace(clone_directory.to_path_buf()).was_not_read())
    }
}

fn read_listing(
    is_needed: bool,
    invocation: ReadInvocation,
    machine: &impl ReadMachine,
) -> SourceReading<String> {
    let tool = invocation.tool();
    if !is_needed || !machine.tool_is_present(tool) {
        return SourceReading::NotRequested(ReadSource::Tool(tool));
    }

    match machine.read(&invocation) {
        Ok(output) if output.exited.succeeded() => SourceReading::Read(output.standard_output),
        Ok(output) => SourceReading::Unreadable(
            format!(
                "{tool} could not be read, {}: {}",
                output.exited,
                output.standard_error.trim()
            )
            .into(),
        ),
        Err(error) => {
            SourceReading::Unreadable(format!("{tool} could not be read: {error}").into())
        }
    }
}

pub fn assess(
    resource: &ResolvedResource,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Assessment {
    if let Err(impediment) = readiness(resource, machine, readings) {
        return Assessment::Unassessable(impediment);
    }

    match resource.declared() {
        Resource::Repository(clone) => {
            assess_repository(clone, &resource.clone_directory(&clone.repository), machine)
        }
        Resource::Application(Application::Installer(installer)) => {
            assess_installer(installer, machine)
        }
        Resource::Application(Application::ReleasedBinary(binary)) => {
            assess_released_binary(binary, machine, readings)
        }
        Resource::Package(Package::Winget(package)) => {
            assess_winget_package(package, machine, readings)
        }
        Resource::Package(Package::UvTool(package)) => assess_uv_tool(package, readings),
        Resource::Package(Package::Cargo(package)) => {
            assess_cargo_package(package, resource, machine, readings)
        }
        Resource::EnvironmentVariable(EnvironmentVariable::Variable(variable)) => {
            assess_variable(variable, machine)
        }
        Resource::EnvironmentVariable(EnvironmentVariable::SearchPathEntry(entry)) => {
            assess_search_path_entry(entry, resource, machine, readings)
        }
        Resource::Symlink(symlink) => assess_symlink(symlink, resource, machine),
        Resource::Registration(Registration::ClaudeMcpServer(server)) => {
            assess_claude_mcp_server(server, machine)
        }
        Resource::Registration(Registration::MachineManifest(manifest)) => {
            assess_machine_manifest(manifest, machine)
        }
        Resource::Command(command) => assess_command(command, machine),
    }
}

// ADR 0004
fn readiness(
    resource: &ResolvedResource,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Result<(), Impediment> {
    for requirement in resource.requirements() {
        if !requirement_is_met(&requirement, resource, machine, readings)? {
            return Err(Impediment::Absent(requirement));
        }
    }
    Ok(())
}

fn requirement_is_met(
    requirement: &Requirement,
    resource: &ResolvedResource,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Result<bool, Impediment> {
    match requirement {
        Requirement::Tool(tool) => Ok(machine.tool_is_present(*tool)),
        Requirement::DotfilesRepository(_) => {
            Ok(machine.path_exists(&resource.files_root().join(".git")))
        }
        Requirement::CargoBinariesOnSearchPath => Ok(readings
            .search_path()?
            .carries(&machine.cargo_binaries_directory())),
    }
}

fn assess_repository(
    clone: &RepositoryClone,
    clone_directory: &Path,
    machine: &impl ReadMachine,
) -> Assessment {
    if !machine.path_exists(&clone_directory.join(".git")) {
        return Assessment::Drifted(format!("{} holds no clone", clone_directory.display()).into());
    }
    if !clone.holds_whole_history() {
        return Assessment::Converged;
    }

    match machine.clone_is_shallow(clone_directory) {
        Ok(false) => Assessment::Converged,
        Ok(true) => Assessment::Drifted(
            format!(
                "{} holds a shallow clone rather than the whole history",
                clone_directory.display()
            )
            .into(),
        ),
        Err(error) => Assessment::Unassessable(Impediment::ActualStateUnreadable(
            format!("the history of the clone could not be read: {error}").into(),
        )),
    }
}

fn assess_installer(installer: &Installer, machine: &impl ReadMachine) -> Assessment {
    match machine.check_presence(&installer.presence_check) {
        Ok(Some(answer)) => match installer.presence_check.answered(&answer) {
            Some(finding) => Assessment::Found(finding.into()),
            None => Assessment::Converged,
        },
        Ok(None) => {
            Assessment::Drifted(format!("not installed — {}", installer.presence_check).into())
        }
        Err(error) => Assessment::Unassessable(Impediment::ActualStateUnreadable(
            format!("presence could not be read: {error}").into(),
        )),
    }
}

fn assess_released_binary(
    binary: &ReleasedBinary,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Assessment {
    let released = match readings.release_of(&binary.repository) {
        Ok(Some(release)) => release,
        Ok(None) => {
            return Assessment::Unassessable(Impediment::ActualStateUnreadable(
                format!("{} has published no release", binary.repository).into(),
            ));
        }
        Err(impediment) => return Assessment::Unassessable(impediment),
    };

    let installed_path = machine
        .binaries_directory()
        .join(binary.installed_name().file_name());
    if !machine.path_exists(&installed_path) {
        return Assessment::Drifted(
            format!(
                "not installed — nothing at {}, and the latest release of {} is {}",
                installed_path.display(),
                binary.repository,
                released.version
            )
            .into(),
        );
    }

    match installed_version(binary, &installed_path, machine) {
        Err(reason) => Assessment::Unassessable(Impediment::ActualStateUnreadable(reason)),
        Ok(installed) if installed == released.version => Assessment::Converged,
        Ok(installed) => Assessment::Drifted(
            format!(
                "{installed} is installed, and the latest release of {} is {}",
                binary.repository, released.version
            )
            .into(),
        ),
    }
}

fn installed_version(
    binary: &ReleasedBinary,
    installed_path: &Path,
    machine: &impl ReadMachine,
) -> Result<Version, UnreadableReason> {
    let output = machine
        .report_version(installed_path, &binary.version_arguments)
        .map_err(|error| UnreadableReason::from(format!("{error:#}")))?;

    if !output.exited.succeeded() {
        return Err(format!(
            "`{}` failed, {}: {}",
            binary.rendered_version_invocation(),
            output.exited,
            output.standard_error.trim()
        )
        .into());
    }

    binary
        .reported_version(&output.standard_output)
        .map_err(UnreadableReason::from)
}

fn assess_winget_package(
    package: &WingetPackage,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Assessment {
    let installed = match installed_winget_package(package, machine, readings) {
        Ok(Some(row)) => row.version,
        Ok(None) => return Assessment::Drifted("winget reports it as not installed".into()),
        Err(impediment) => return Assessment::Unassessable(impediment),
    };

    match &package.version {
        PackageCurrency::Latest => match readings.winget_upgrade_of(&package.id) {
            Ok(None) => Assessment::Converged,
            Ok(Some(offered)) => Assessment::Drifted(
                format!("{installed} is installed, and winget offers {offered}").into(),
            ),
            Err(impediment) => Assessment::Unassessable(impediment),
        },
        PackageCurrency::Exactly(declared) => assess_winget_version(declared, &installed),
    }
}

fn assess_winget_version(declared: &WingetVersion, installed: &str) -> Assessment {
    if installed == declared.as_ref() {
        return Assessment::Converged;
    }
    if !installed.starts_with(|character: char| character.is_ascii_digit()) {
        return Assessment::Unassessable(Impediment::ActualStateUnreadable(
            format!("winget reports no one version it is installed at, only {installed:?}").into(),
        ));
    }

    Assessment::Drifted(
        format!("{installed} is installed, and the version declared is {declared}").into(),
    )
}

fn installed_winget_package(
    package: &WingetPackage,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Result<Option<WingetRow>, Impediment> {
    let listing = readings.winget_packages.read()?;

    match winget_row(listing, package.id.as_ref()).map_err(Impediment::ActualStateUnreadable)? {
        Some(row) => Ok(Some(row)),
        None => winget_package_by_identifier(package, machine),
    }
}

// 2026-09-26: `winget list --id <id> --exact` exits 0x8A150014 and prints this on its standard
// output when nothing installed matches. winget v1.29.380 on Windows 11.
const WINGET_FINDS_NO_PACKAGE: &str = "No installed package found matching input criteria.";

const WINGET_EXITS_FINDING_NO_PACKAGE: Exited = Exited::Code(0x8A15_0014_u32.cast_signed());

fn winget_package_by_identifier(
    package: &WingetPackage,
    machine: &impl ReadMachine,
) -> Result<Option<WingetRow>, Impediment> {
    let invocation = ReadInvocation::WingetPackage {
        id: package.id.clone(),
    };
    let output = machine.read(&invocation).map_err(|error| {
        Impediment::ActualStateUnreadable(
            format!("winget could not be asked about {}: {error}", package.id).into(),
        )
    })?;

    if !output.exited.succeeded() {
        return match output.standard_output.trim() == WINGET_FINDS_NO_PACKAGE {
            true => Ok(None),
            false => Err(Impediment::ActualStateUnreadable(
                format!(
                    "winget could not be asked about {}, {}: {}",
                    package.id,
                    output.exited,
                    output.standard_error.trim()
                )
                .into(),
            )),
        };
    }

    winget_row(&output.standard_output, package.id.as_ref())
        .map_err(Impediment::ActualStateUnreadable)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WingetUpgradeListing {
    NothingToUpgrade,
    Listed(String),
}

// 2026-10-06: `winget upgrade --source winget --name <no such package>` exited 0x8A150014 and
// printed the same line `winget list` prints when nothing matches. winget v1.29.380 on Windows 11.
fn read_winget_upgrades(
    is_needed: bool,
    machine: &impl ReadMachine,
) -> SourceReading<WingetUpgradeListing> {
    if !is_needed || !machine.tool_is_present(Tool::Winget) {
        return SourceReading::NotRequested(ReadSource::Tool(Tool::Winget));
    }

    match machine.read(&ReadInvocation::WingetUpgrades) {
        Ok(output) if output.exited.succeeded() => {
            SourceReading::Read(WingetUpgradeListing::Listed(output.standard_output))
        }
        Ok(output)
            if output.exited == WINGET_EXITS_FINDING_NO_PACKAGE
                && output.standard_output.trim() == WINGET_FINDS_NO_PACKAGE =>
        {
            SourceReading::Read(WingetUpgradeListing::NothingToUpgrade)
        }
        Ok(output) => SourceReading::Unreadable(
            format!(
                "winget could not list what it would upgrade, {}: {} {}",
                output.exited,
                output.standard_output.trim(),
                output.standard_error.trim()
            )
            .into(),
        ),
        Err(error) => SourceReading::Unreadable(
            format!("winget could not list what it would upgrade: {error}").into(),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WingetRow {
    version: String,
    available: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WingetColumns {
    id: usize,
    version: usize,
    available: Option<usize>,
    source: Option<usize>,
}

impl WingetColumns {
    fn located_in(listing: &str) -> Option<Self> {
        listing.lines().find_map(|line| {
            let characters_before = |byte: usize| line[..byte].chars().count();
            let name = line.find("Name")?;
            let id = line[name..].find("Id").map(|offset| name + offset)?;
            let version = line[id..].find("Version").map(|offset| id + offset)?;
            let available = line[version..]
                .find("Available")
                .map(|offset| version + offset);
            let source = line[version..]
                .find("Source")
                .map(|offset| version + offset);
            Some(Self {
                id: characters_before(id),
                version: characters_before(version),
                available: available.map(characters_before),
                source: source.map(characters_before),
            })
        })
    }

    fn cell(line: &str, first: usize, past_last: Option<usize>) -> String {
        let characters = line.chars().skip(first);
        let cell: String = match past_last {
            Some(past_last) => characters.take(past_last.saturating_sub(first)).collect(),
            None => characters.collect(),
        };
        cell.trim().to_owned()
    }
}

const TRUNCATION_MARKER: char = '…';

// ADR 0010
fn winget_row(listing: &str, id: &str) -> Result<Option<WingetRow>, UnreadableReason> {
    let Some(columns) = WingetColumns::located_in(listing) else {
        return Err("winget's listing has no Id column, so it could not be read".into());
    };

    let mut found = None;
    for line in listing.lines() {
        let cell = WingetColumns::cell(line, columns.id, Some(columns.version));

        if cell.contains(TRUNCATION_MARKER) {
            return Err("winget cut an Id short, so its listing could not be read".into());
        }
        if cell == id && found.is_none() {
            let available = columns
                .available
                .map(|available| WingetColumns::cell(line, available, columns.source));
            found = Some(WingetRow {
                version: WingetColumns::cell(
                    line,
                    columns.version,
                    columns.available.or(columns.source),
                ),
                available: available.filter(|offered| !offered.is_empty()),
            });
        }
    }

    Ok(found)
}

fn assess_uv_tool(package: &UvToolPackage, readings: &SourceReadings) -> Assessment {
    let installed = match readings.installed_uv_tool(&package.name) {
        Ok(Some(installed)) => installed,
        Ok(None) => return Assessment::Drifted("uv has not installed it".into()),
        Err(impediment) => return Assessment::Unassessable(impediment),
    };

    match &package.version {
        PackageCurrency::Exactly(declared) if installed == *declared => Assessment::Converged,
        PackageCurrency::Exactly(declared) => Assessment::Drifted(
            format!("{installed} is installed, and the version declared is {declared}").into(),
        ),
        PackageCurrency::Latest => match readings.newer_uv_tool(&package.name) {
            Ok(None) => Assessment::Converged,
            Ok(Some(latest)) => Assessment::Drifted(
                format!(
                    "{installed} is installed, and the newest version that resolves is {latest}"
                )
                .into(),
            ),
            Err(impediment) => Assessment::Unassessable(impediment),
        },
    }
}

const LATEST_MARKER: &str = "[latest: ";

// 2026-09-25: `uv tool list` names each tool on a line of its own as `name vX.Y.Z`, with its
// executables beneath as `- executable` lines, and `--outdated` appends `[latest: A.B.C]` to each
// tool it lists. uv 0.10.12 on Windows 11.
fn listed_uv_tool(
    listing: &str,
    name: &UvToolName,
) -> Result<Option<(UvToolVersion, Option<UvToolVersion>)>, UnreadableReason> {
    let Some(line) = listing
        .lines()
        .filter(|line| !line.trim_start().starts_with('-'))
        .find(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|listed| name.is_listed_as(listed))
        })
    else {
        return Ok(None);
    };

    let Some(installed) = line
        .split_whitespace()
        .nth(1)
        .and_then(|word| word.strip_prefix('v'))
        .filter(|version| !version.is_empty())
    else {
        return Err(
            format!("uv listed {name} without the version it is installed at: {line}").into(),
        );
    };

    let latest = match line.split_once(LATEST_MARKER) {
        None => None,
        Some((_, remainder)) => match remainder.split_once(']') {
            Some((latest, _)) if !latest.trim().is_empty() => {
                Some(UvToolVersion::from(latest.trim()))
            }
            Some(_) | None => {
                return Err(format!(
                    "uv listed {name} as behind a version it did not name: {line}"
                )
                .into());
            }
        },
    };

    Ok(Some((UvToolVersion::from(installed), latest)))
}

fn assess_cargo_package(
    package: &CargoPackage,
    resource: &ResolvedResource,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Assessment {
    match &package.source {
        CargoSource::Workspace { repository } => assess_workspace_member(
            &package.crate_name,
            &resource.clone_directory(repository),
            machine,
            readings,
        ),
        CargoSource::Registry { .. } | CargoSource::Path { .. } => {
            assess_declared_cargo_package(package, machine, readings)
        }
    }
}

// ADR 0040
fn assess_workspace_member(
    crate_name: &CrateName,
    clone_directory: &Path,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Assessment {
    let reading = match readings.resolved_workspace(clone_directory) {
        Ok(reading) => reading,
        Err(impediment) => return Assessment::Unassessable(impediment),
    };
    if !reading.members.contains_key(crate_name) {
        return Assessment::Drifted("the workspace no longer holds it".into());
    }
    if let Err(impediment) = readings.install_records() {
        return Assessment::Unassessable(impediment);
    }

    let (own_copies, search_path) = match (
        readings.own_copies(clone_directory, crate_name),
        readings.search_path(),
    ) {
        (Ok(own_copies), Ok(search_path)) => (own_copies, search_path),
        (Err(impediment), _) | (_, Err(impediment)) => {
            return Assessment::Unassessable(impediment);
        }
    };

    judged(own_copies.iter().map(|(binary, own_copy)| {
        (
            binary.clone(),
            own_copy.clone(),
            resolution_of(binary, search_path, machine),
        )
    }))
}

fn assess_declared_cargo_package(
    package: &CargoPackage,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Assessment {
    let installed = match readings.cargo_crates.read() {
        Ok(listing) => listing,
        Err(impediment) => return Assessment::Unassessable(impediment),
    };

    let Some(actual) = installed_crate_source(installed, package.crate_name.as_ref()) else {
        return Assessment::Drifted("cargo has not installed it".into());
    };

    match (&package.source, &actual) {
        (
            CargoSource::Registry {
                version: PackageCurrency::Latest,
            },
            InstalledFrom::Registry { version: installed },
        ) => match readings.newest_crate(&package.crate_name) {
            Ok(newest) => {
                assess_installed_version(&WantedVersion::NewestPublished(newest.clone()), installed)
            }
            Err(impediment) => Assessment::Unassessable(impediment),
        },
        (
            CargoSource::Registry {
                version: PackageCurrency::Exactly(declared),
            },
            InstalledFrom::Registry { version: installed },
        ) => assess_installed_version(&WantedVersion::Declared(declared.clone()), installed),
        (CargoSource::Path { path }, InstalledFrom::Path(installed_path))
            if paths_are_the_same(path, installed_path, machine) =>
        {
            Assessment::Converged
        }
        (_, actual) => Assessment::Drifted(format!("installed from {actual}").into()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WantedVersion {
    Declared(CrateVersion),
    NewestPublished(CrateVersion),
}

impl WantedVersion {
    fn version(&self) -> &CrateVersion {
        match self {
            WantedVersion::Declared(version) | WantedVersion::NewestPublished(version) => version,
        }
    }
}

impl std::fmt::Display for WantedVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WantedVersion::Declared(version) => {
                write!(formatter, "the version declared is {version}")
            }
            WantedVersion::NewestPublished(version) => {
                write!(formatter, "the newest published to crates.io is {version}")
            }
        }
    }
}

fn assess_installed_version(wanted: &WantedVersion, installed: &str) -> Assessment {
    let spelled = installed.strip_prefix('v').unwrap_or(installed);

    match CrateVersion::try_from(spelled) {
        Ok(actual) if actual == *wanted.version() => Assessment::Converged,
        Ok(actual) => Assessment::Drifted(format!("{actual} is installed, and {wanted}").into()),
        Err(reason) => Assessment::Unassessable(Impediment::ActualStateUnreadable(
            format!("cargo lists it at a version this program cannot read: {reason}").into(),
        )),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum InstalledFrom {
    Registry { version: String },
    Path(String),
    Git { url: String, commit: String },
}

impl std::fmt::Display for InstalledFrom {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstalledFrom::Registry { .. } => formatter.write_str("the registry"),
            InstalledFrom::Path(path) => write!(formatter, "{path}"),
            InstalledFrom::Git { url, commit } => write!(formatter, "{url} at {commit}"),
        }
    }
}

fn installed_crate_line(line: &str) -> Option<(&str, InstalledFrom)> {
    let (name, remainder) = line.trim_end().split_once(' ')?;

    let Some((_, source)) = remainder.split_once('(') else {
        return Some((
            name,
            InstalledFrom::Registry {
                version: remainder.trim_end_matches(':').to_owned(),
            },
        ));
    };
    let source = source.trim_end_matches([')', ':']);

    match source.split_once('#') {
        Some((location, commit)) => Some((
            name,
            InstalledFrom::Git {
                url: location
                    .split_once('?')
                    .map_or(location, |(url, _)| url)
                    .to_owned(),
                commit: commit.to_owned(),
            },
        )),
        None => Some((name, InstalledFrom::Path(source.to_owned()))),
    }
}

fn installed_crate_source(listing: &str, crate_name: &str) -> Option<InstalledFrom> {
    listing
        .lines()
        .filter(|line| !line.starts_with(char::is_whitespace))
        .filter_map(installed_crate_line)
        .find(|(name, _)| *name == crate_name)
        .map(|(_, source)| source)
}

fn paths_are_the_same(declared: &Path, installed: &str, machine: &impl ReadMachine) -> bool {
    let installed_path = Path::new(installed);
    match (
        machine.canonical_path(declared),
        machine.canonical_path(installed_path),
    ) {
        (Some(declared), Some(installed)) => declared == installed,
        _ => declared == installed_path,
    }
}

fn assess_variable(variable: &Variable, machine: &impl ReadMachine) -> Assessment {
    match machine.read_environment_variable(&variable.name) {
        Ok(Some(set)) if set == variable.value => Assessment::Converged,
        Ok(Some(set)) => Assessment::Drifted(format!("is set to {set}").into()),
        Ok(None) => Assessment::Drifted("is not set".into()),
        Err(error) => Assessment::Unassessable(Impediment::ActualStateUnreadable(
            format!("{error:#}").into(),
        )),
    }
}

fn assess_search_path_entry(
    entry: &SearchPathEntry,
    resource: &ResolvedResource,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Assessment {
    let reading = match readings.search_path() {
        Ok(reading) => reading,
        Err(impediment) => return Assessment::Unassessable(impediment),
    };
    let directory = search_path_directory(entry, resource, machine);

    match reading.carries(&directory) {
        true => Assessment::Converged,
        false => Assessment::Drifted(format!("{} is not on it", directory.display()).into()),
    }
}

fn assess_symlink(
    symlink: &Symlink,
    resource: &ResolvedResource,
    machine: &impl ReadMachine,
) -> Assessment {
    let (link_path, source_path) = symlink_location(symlink, resource, machine);

    match machine.link_target(&link_path) {
        None if machine.path_exists(&link_path) => {
            Assessment::Drifted(format!("{} is not a link", link_path.display()).into())
        }
        None => Assessment::Drifted(format!("{} is missing", link_path.display()).into()),
        Some(target) if target == source_path => Assessment::Converged,
        Some(target) => Assessment::Drifted(
            format!(
                "links to {} instead of {}",
                target.display(),
                source_path.display()
            )
            .into(),
        ),
    }
}

fn assess_machine_manifest(manifest: &RenderedManifest, machine: &impl ReadMachine) -> Assessment {
    match machine.text_file_at(&MachineManifest::path_within(machine.home_directory())) {
        Ok(None) => Assessment::Drifted("the machine holds no manifest".into()),
        Ok(Some(held)) if held == manifest.document() => Assessment::Converged,
        Ok(Some(_)) => Assessment::Drifted("the manifest says something else".into()),
        Err(error) => Assessment::Unassessable(Impediment::ActualStateUnreadable(
            format!("{error:#}").into(),
        )),
    }
}

fn assess_claude_mcp_server(server: &ClaudeMcpServer, machine: &impl ReadMachine) -> Assessment {
    let invocation = ReadInvocation::ClaudeMcpServer {
        name: server.name.clone(),
    };
    let output = match machine.read(&invocation) {
        Ok(output) => output,
        Err(error) => {
            return Assessment::Unassessable(Impediment::ActualStateUnreadable(
                format!("claude could not be read: {error}").into(),
            ));
        }
    };

    if !output.exited.succeeded() {
        return claude_refusal(&output);
    }

    match first_difference(server, &output.standard_output) {
        None => Assessment::Converged,
        Some(difference) => Assessment::Drifted(difference.into()),
    }
}

// 2026-09-19: `claude mcp get <name>` exits 1 with an empty standard output and a standard
// error opening with this phrase when it holds no server under that name. Claude Code 2.1.278
// on Windows 11.
const NO_SUCH_SERVER: &str = "No MCP server named";

fn claude_refusal(output: &CommandOutput) -> Assessment {
    let standard_error = &output.standard_error;
    match standard_error.trim_start().starts_with(NO_SUCH_SERVER) {
        true => Assessment::Drifted("claude holds no such server".into()),
        false => Assessment::Unassessable(Impediment::ActualStateUnreadable(
            format!(
                "claude could not be read, {}: {}",
                output.exited,
                standard_error.trim()
            )
            .into(),
        )),
    }
}

fn first_difference(server: &ClaudeMcpServer, reported: &str) -> Option<String> {
    let field = |label: &str| {
        reported.lines().find_map(|line| {
            line.trim()
                .strip_prefix(&format!("{label}: "))
                .map(str::to_owned)
        })
    };

    match field("Command") {
        Some(command) if command == server.command => {}
        Some(command) => return Some(format!("registered to run {command}")),
        None => return Some("claude reports no command for it".to_owned()),
    }

    // 2026-09-19: `claude mcp get` leaves out the line for a field that does not apply rather
    // than printing it empty — the output for an http server carries no Command and no Args
    // line at all. Claude Code 2.1.278 on Windows 11.
    match field("Args") {
        Some(registered) if registered == server.args.join(" ") => {}
        Some(registered) => return Some(format!("registered with the arguments {registered}")),
        None if server.args.is_empty() => {}
        None => return Some("registered with no arguments".to_owned()),
    }

    server
        .environment
        .iter()
        .find(|(key, value)| {
            !reported
                .lines()
                .any(|line| line.trim() == format!("{key}={value}"))
        })
        .map(|(key, _)| format!("registered without {key} set as declared"))
}

fn assess_command(command: &Command, machine: &impl ReadMachine) -> Assessment {
    let Some(check) = &command.presence_check else {
        return Assessment::Drifted("declares no presence check, so it runs every time".into());
    };

    match machine.check_presence(check) {
        Ok(Some(_)) => Assessment::Converged,
        Ok(None) => Assessment::Drifted(format!("not yet done — {check}").into()),
        Err(error) => Assessment::Unassessable(Impediment::ActualStateUnreadable(
            format!("presence could not be read: {error}").into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        configuration::{McpScope, McpServerName},
        machine::Exited,
    };

    // 2026-07-31: taken verbatim from `cargo install --list`.
    const LISTING: &str = concat!(
        "ci-checks v0.1.0 (C:\\Repositories\\Personal\\dotfiles\\tools\\ci-checks):\n",
        "    ci-checks.exe\n",
        "committed v1.1.11:\n",
        "    committed.exe\n",
        "stop-gate v0.1.0 (https://github.com/RobinCombrink/dotfiles",
        "?rev=2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c#2ae2ffff):\n",
        "    stop-gate.exe\n",
    );

    #[test]
    fn a_crate_installed_from_the_registry_is_reported_as_coming_from_it_at_its_version() {
        assert_eq!(
            installed_crate_source(LISTING, "committed"),
            Some(InstalledFrom::Registry {
                version: "v1.1.11".to_owned()
            })
        );
    }

    fn pinned(version: &str) -> WantedVersion {
        WantedVersion::Declared(CrateVersion::try_from(version).unwrap())
    }

    #[test]
    fn a_crate_installed_at_the_version_declared_is_converged() {
        assert_eq!(
            assess_installed_version(&pinned("27.1.0"), "v27.1.0"),
            Assessment::Converged
        );
    }

    #[test]
    fn a_crate_installed_at_another_version_than_declared_is_drifted() {
        assert_eq!(
            assess_installed_version(&pinned("27.1.0"), "v27.0.0"),
            Assessment::Drifted("27.0.0 is installed, and the version declared is 27.1.0".into())
        );
    }

    #[test]
    fn a_crate_behind_the_newest_published_is_drifted_naming_both_versions() {
        let newest = WantedVersion::NewestPublished(CrateVersion::try_from("15.1.0").unwrap());

        assert_eq!(
            assess_installed_version(&newest, "v14.1.1"),
            Assessment::Drifted(
                "14.1.1 is installed, and the newest published to crates.io is 15.1.0".into()
            )
        );
    }

    #[test]
    fn a_crate_listed_at_a_version_that_cannot_be_read_is_unassessable_rather_than_drifted() {
        let assessment = assess_installed_version(&pinned("27.1.0"), "vtwenty-seven");

        let Assessment::Unassessable(_) = assessment else {
            panic!("expected an unassessable crate, got {assessment:?}");
        };
    }

    #[test]
    fn a_crate_installed_from_a_path_is_reported_with_that_path() {
        assert_eq!(
            installed_crate_source(LISTING, "ci-checks"),
            Some(InstalledFrom::Path(
                "C:\\Repositories\\Personal\\dotfiles\\tools\\ci-checks".to_owned()
            ))
        );
    }

    #[test]
    fn a_crate_installed_from_git_is_reported_with_the_commit_it_resolved_to() {
        assert_eq!(
            installed_crate_source(LISTING, "stop-gate"),
            Some(InstalledFrom::Git {
                url: "https://github.com/RobinCombrink/dotfiles".to_owned(),
                commit: "2ae2ffff".to_owned(),
            })
        );
    }

    #[test]
    fn a_crate_cargo_has_not_installed_is_absent_from_the_listing() {
        assert_eq!(installed_crate_source(LISTING, "ripgrep"), None);
    }

    #[test]
    fn the_binaries_a_crate_installed_are_not_mistaken_for_crates_of_their_own() {
        assert_eq!(installed_crate_source(LISTING, "committed.exe"), None);
    }

    // 2026-08-02: taken verbatim from `winget list`, narrowed to three packages.
    const PACKAGES: &str = concat!(
        "Name                 Id                        Version    Available  Source\n",
        "-----------------------------------------------------------------------------\n",
        "Battle for Wesnoth   Wesnoth.BattleForWesnoth  Unknown    1.18.4     winget\n",
        "Bitwarden            Bitwarden.Bitwarden       2026.3.1   2026.7.0   winget\n",
        "AMD Software         ARP\\Machine\\X64\\AMD Cat   26.7.1                     \n",
    );

    fn winget_lists_package(listing: &str, id: &str) -> Result<bool, UnreadableReason> {
        winget_row(listing, id).map(|row| row.is_some())
    }

    #[test]
    fn a_package_winget_lists_is_found() {
        assert_eq!(
            winget_lists_package(PACKAGES, "Bitwarden.Bitwarden"),
            Ok(true)
        );
    }

    #[test]
    fn a_package_winget_lists_is_read_at_its_version_and_the_one_winget_offers() {
        assert_eq!(
            winget_row(PACKAGES, "Bitwarden.Bitwarden"),
            Ok(Some(WingetRow {
                version: "2026.3.1".to_owned(),
                available: Some("2026.7.0".to_owned()),
            }))
        );
    }

    #[test]
    fn a_package_winget_offers_nothing_newer_for_is_read_without_an_available_version() {
        assert_eq!(
            winget_row(PACKAGES, "ARP\\Machine\\X64\\AMD Cat"),
            Ok(Some(WingetRow {
                version: "26.7.1".to_owned(),
                available: None,
            }))
        );
    }

    // 2026-10-06: taken verbatim from `winget upgrade --accept-source-agreements
    // --disable-interactivity`, narrowed to two packages and the lines closing the table.
    const UPGRADES: &str = concat!(
        "Name                                                         Id                                     Version              Available           Source\n",
        "---------------------------------------------------------------------------------------------------------------------------------------------------\n",
        "Lefthook                                                     evilmartians.lefthook                  2.1.10               2.1.17              winget\n",
        "shfmt                                                        mvdan.shfmt                            3.13.1               3.14.1              winget\n",
        "34 upgrades available.\n",
        "2 package(s) have version numbers that cannot be determined. Use --include-unknown to see all results.\n",
    );

    #[test]
    fn a_package_winget_would_upgrade_is_read_with_the_version_it_offers() {
        assert_eq!(
            winget_row(UPGRADES, "mvdan.shfmt"),
            Ok(Some(WingetRow {
                version: "3.13.1".to_owned(),
                available: Some("3.14.1".to_owned()),
            }))
        );
    }

    #[test]
    fn the_lines_closing_an_upgrade_listing_are_not_read_as_packages() {
        assert_eq!(winget_row(UPGRADES, "jqlang.jq"), Ok(None));
    }

    fn declared(version: &str) -> WingetVersion {
        WingetVersion::from(version)
    }

    #[test]
    fn a_package_at_the_version_declared_is_converged() {
        assert_eq!(
            assess_winget_version(&declared("2025.1.2.11"), "2025.1.2.11"),
            Assessment::Converged
        );
    }

    #[test]
    fn a_package_newer_than_the_version_declared_is_drifted() {
        assert_eq!(
            assess_winget_version(&declared("3.13.1"), "3.14.1"),
            Assessment::Drifted("3.14.1 is installed, and the version declared is 3.13.1".into())
        );
    }

    #[test]
    fn a_package_winget_knows_no_one_version_of_is_unassessable_rather_than_drifted() {
        let Assessment::Unassessable(_) =
            assess_winget_version(&declared("17.14.41"), "< 17.14.37")
        else {
            panic!("a version winget gives only a bound for was read as one version");
        };
    }

    #[test]
    fn a_package_winget_does_not_list_is_not_found() {
        assert_eq!(
            winget_lists_package(PACKAGES, "Microsoft.PowerShell"),
            Ok(false)
        );
    }

    #[test]
    fn an_identifier_holding_spaces_is_read_whole_rather_than_cut_at_the_first_one() {
        assert_eq!(
            winget_lists_package(PACKAGES, "ARP\\Machine\\X64\\AMD Cat"),
            Ok(true)
        );
    }

    #[test]
    fn a_package_named_after_another_packages_identifier_is_not_mistaken_for_it() {
        let listing = concat!(
            "Name                  Id             Version  Available  Source\n",
            "---------------------------------------------------------------\n",
            "Bitwarden.Bitwarden   Some.Other.Id  1.0.0                winget\n",
        );

        assert_eq!(
            winget_lists_package(listing, "Bitwarden.Bitwarden"),
            Ok(false)
        );
    }

    #[test]
    fn a_name_that_is_not_ascii_does_not_shift_the_column_the_identifier_is_read_from() {
        let listing = concat!(
            "Name          Id                   Version  Available  Source\n",
            "-------------------------------------------------------------\n",
            "Café Münchén  Bitwarden.Bitwarden  1.0.0               winget\n",
        );

        assert_eq!(
            winget_lists_package(listing, "Bitwarden.Bitwarden"),
            Ok(true)
        );
    }

    #[test]
    fn a_listing_that_cut_an_identifier_short_is_refused_rather_than_read_as_an_absent_package() {
        let listing = concat!(
            "Name        Id             Version  Available  Source\n",
            "-----------------------------------------------------\n",
            "Bitwarden   Bitwarden.Bi…  2026.3.1            winget\n",
        );

        assert!(winget_lists_package(listing, "Bitwarden.Bitwarden").is_err());
    }

    #[test]
    fn a_listing_whose_columns_cannot_be_located_is_refused() {
        assert!(winget_lists_package("no columns here\n", "Bitwarden.Bitwarden").is_err());
    }

    // 2026-09-13: taken verbatim from `claude mcp get probe-server` against a stdio server
    // registered for the measurement, on Claude Code 2.1.270 under Windows 11.
    const REGISTERED: &str = concat!(
        "probe-server:\n",
        "  Scope: Local config (private to you in this project)\n",
        "  Status: \u{2718} Failed to connect\n",
        "  Issue: CONNECTION_CLOSED: Connection closed\n",
        "  Type: stdio\n",
        "  Command: my-program\n",
        "  Args: start --port 1234\n",
        "  Environment:\n",
        "    ALPHA=one\n",
        "    BETA=two\n",
        "\n",
        "To remove this server, run: claude mcp remove probe-server -s local\n",
    );

    fn probe_server() -> ClaudeMcpServer {
        ClaudeMcpServer {
            name: McpServerName::from("probe-server"),
            scope: McpScope::Local,
            command: "my-program".to_owned(),
            args: vec!["start".to_owned(), "--port".to_owned(), "1234".to_owned()],
            environment: BTreeMap::from([
                ("ALPHA".to_owned(), "one".to_owned()),
                ("BETA".to_owned(), "two".to_owned()),
            ]),
        }
    }

    #[test]
    fn a_registration_holding_everything_that_was_declared_has_not_drifted() {
        assert_eq!(first_difference(&probe_server(), REGISTERED), None);
    }

    #[test]
    fn a_registration_running_another_program_drifts_and_names_the_one_it_runs() {
        let declared = ClaudeMcpServer {
            command: "another-program".to_owned(),
            ..probe_server()
        };

        assert_eq!(
            first_difference(&declared, REGISTERED),
            Some("registered to run my-program".to_owned())
        );
    }

    #[test]
    fn a_registration_started_with_other_arguments_drifts_and_names_the_ones_it_holds() {
        let declared = ClaudeMcpServer {
            args: vec!["start".to_owned(), "--port".to_owned(), "9999".to_owned()],
            ..probe_server()
        };

        assert_eq!(
            first_difference(&declared, REGISTERED),
            Some("registered with the arguments start --port 1234".to_owned())
        );
    }

    #[test]
    fn a_registration_missing_a_declared_environment_entry_drifts_and_names_the_variable() {
        let mut environment = probe_server().environment;
        environment.insert("GAMMA".to_owned(), "three".to_owned());
        let declared = ClaudeMcpServer {
            environment,
            ..probe_server()
        };

        assert_eq!(
            first_difference(&declared, REGISTERED),
            Some("registered without GAMMA set as declared".to_owned())
        );
    }

    #[test]
    fn a_registration_holding_a_declared_variable_at_another_value_drifts() {
        let mut environment = probe_server().environment;
        environment.insert("ALPHA".to_owned(), "something else".to_owned());
        let declared = ClaudeMcpServer {
            environment,
            ..probe_server()
        };

        assert_eq!(
            first_difference(&declared, REGISTERED),
            Some("registered without ALPHA set as declared".to_owned())
        );
    }

    #[test]
    fn a_registration_reporting_no_arguments_for_a_server_declaring_some_names_that_rather_than_nothing()
     {
        let reported = "probe-server:\n  Type: stdio\n  Command: my-program\n";

        assert_eq!(
            first_difference(&probe_server(), reported),
            Some("registered with no arguments".to_owned())
        );
    }

    #[test]
    fn a_registration_reporting_no_arguments_for_a_server_declaring_none_has_not_drifted() {
        let declared = ClaudeMcpServer {
            args: Vec::new(),
            environment: BTreeMap::new(),
            ..probe_server()
        };
        let reported = "probe-server:\n  Type: stdio\n  Command: my-program\n";

        assert_eq!(first_difference(&declared, reported), None);
    }

    #[test]
    fn a_report_naming_no_command_at_all_drifts_rather_than_reading_as_no_command_declared() {
        let reported = "probe-server:\n  Scope: Local config\n  Type: http\n";

        assert_eq!(
            first_difference(&probe_server(), reported),
            Some("claude reports no command for it".to_owned())
        );
    }

    fn claude_exiting_one_saying(standard_error: &str) -> CommandOutput {
        CommandOutput {
            exited: Exited::Code(1),
            standard_output: String::new(),
            standard_error: standard_error.to_owned(),
        }
    }

    #[test]
    fn a_claude_that_reports_no_server_under_the_name_has_drifted() {
        let refusal = "No MCP server named \"probe-server\". Configured servers: serena, github";

        assert_eq!(
            claude_refusal(&claude_exiting_one_saying(refusal)),
            Assessment::Drifted("claude holds no such server".into())
        );
    }

    #[test]
    fn a_claude_that_failed_for_any_other_reason_is_unassessable_rather_than_drifted() {
        assert_eq!(
            claude_refusal(&claude_exiting_one_saying(
                "Invalid API key · Please run /login"
            )),
            Assessment::Unassessable(Impediment::ActualStateUnreadable(
                "claude could not be read, exited with 1: Invalid API key · Please run /login"
                    .into()
            ))
        );
    }

    // 2026-09-25: taken verbatim from `uv tool list` and `uv tool list --outdated` under uv
    // 0.10.12 on Windows 11, with a second tool added to the first.
    const UV_TOOLS: &str = concat!(
        "cowsay v6.1\n",
        "- cowsay\n",
        "serena-agent v1.5.3\n",
        "- serena\n",
        "- serena-agent\n",
        "- serena-hooks\n",
    );
    const UV_OUTDATED_TOOLS: &str = concat!(
        "serena-agent v1.5.3 [latest: 1.7.0]\n",
        "- serena\n",
        "- serena-agent\n",
        "- serena-hooks\n",
    );

    fn serena() -> UvToolName {
        UvToolName::from("serena-agent")
    }

    #[test]
    fn a_tool_uv_lists_is_read_at_the_version_it_is_installed_at() {
        assert_eq!(
            listed_uv_tool(UV_TOOLS, &serena()),
            Ok(Some((UvToolVersion::from("1.5.3"), None)))
        );
    }

    #[test]
    fn a_tool_uv_does_not_list_is_not_installed() {
        assert_eq!(
            listed_uv_tool(UV_TOOLS, &UvToolName::from("ruff")),
            Ok(None)
        );
    }

    #[test]
    fn an_executable_named_after_a_tool_is_not_mistaken_for_the_tool() {
        let listing = "serena-agent v1.5.3\n- serena\n";

        assert_eq!(
            listed_uv_tool(listing, &UvToolName::from("serena")),
            Ok(None)
        );
    }

    #[test]
    fn a_tool_uv_lists_as_behind_is_read_with_the_newest_version_that_resolves() {
        assert_eq!(
            listed_uv_tool(UV_OUTDATED_TOOLS, &serena()),
            Ok(Some((
                UvToolVersion::from("1.5.3"),
                Some(UvToolVersion::from("1.7.0"))
            )))
        );
    }

    #[test]
    fn a_tool_is_found_under_the_name_uv_normalises_it_to() {
        assert!(
            listed_uv_tool(UV_TOOLS, &UvToolName::from("Serena_Agent"))
                .expect("a readable listing")
                .is_some()
        );
    }

    #[test]
    fn a_tool_listed_without_a_version_is_refused_rather_than_read_as_absent() {
        assert!(listed_uv_tool("serena-agent\n- serena\n", &serena()).is_err());
    }

    #[test]
    fn a_tool_listed_as_behind_nothing_it_names_is_refused_rather_than_read_as_current() {
        assert!(listed_uv_tool("serena-agent v1.5.3 [latest: ]\n", &serena()).is_err());
    }
}

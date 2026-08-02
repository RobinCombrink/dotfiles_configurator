use {
    crate::{
        configuration::{
            Application, CargoPackage, CargoSource, ClaudeMcpServer, Command, CrateName,
            DesiredState, GitHubRepository, Package, Registration, Resource, Symlink,
            WingetPackage,
        },
        convergence::{Assessment, DriftReason, Requirement},
        machine::{
            ReadInvocation, ReadMachine,
            workspace_reading::{Revision, WorkspaceReading},
        },
    },
    std::collections::BTreeMap,
};

/// Every source that answers about a whole set of resources at once, read once for one change
/// set. The cost of a change set therefore scales with the number of sources it touches rather
/// than the number of resources declared against them. See ADR 0010.
///
/// A source whose tool is absent, or which nothing declared needs, is not read at all — so a
/// configuration declaring no cargo package never launches cargo. Neither absence is reachable
/// from an assessment, because a resource's requirements are read before its source is consulted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReadings {
    winget_packages: Option<Result<String, DriftReason>>,
    cargo_crates: Option<Result<String, DriftReason>>,
    workspaces: BTreeMap<GitHubRepository, Result<Option<WorkspaceReading>, DriftReason>>,
}

impl SourceReadings {
    pub fn read_for(desired_state: &DesiredState, machine: &impl ReadMachine) -> Self {
        let mut winget_is_needed = false;
        let mut cargo_is_needed = !desired_state.workspaces.is_empty();
        for resource in &desired_state.resources {
            if let Resource::Package(package) = resource {
                match package {
                    Package::Winget(_) => winget_is_needed = true,
                    Package::Cargo(_) => cargo_is_needed = true,
                }
            }
        }

        let cargo_crates = read_listing(
            cargo_is_needed,
            ReadInvocation::CargoInstalledCrates,
            machine,
        );
        let installed = match &cargo_crates {
            Some(Ok(listing)) => installed_revisions(listing),
            Some(Err(_)) | None => BTreeMap::new(),
        };

        let mut workspaces = BTreeMap::new();
        for workspace in &desired_state.workspaces {
            let repository_path = machine
                .repositories_directory()
                .join(workspace.repository.repository.as_ref());
            let reading = machine
                .read_cargo_workspace(&repository_path, &installed)
                .map_err(|error| DriftReason::from(format!("{error:#}")));
            workspaces.insert(workspace.repository.clone(), reading);
        }

        Self {
            winget_packages: read_listing(
                winget_is_needed,
                ReadInvocation::WingetInstalledPackages,
                machine,
            ),
            cargo_crates,
            workspaces,
        }
    }

    pub fn workspace(
        &self,
        repository: &GitHubRepository,
    ) -> Option<&Result<Option<WorkspaceReading>, DriftReason>> {
        self.workspaces.get(repository)
    }

    pub fn workspace_revision(&self, repository: &GitHubRepository) -> Option<&Revision> {
        match self.workspaces.get(repository) {
            Some(Ok(Some(reading))) => Some(&reading.revision),
            Some(Ok(None)) | Some(Err(_)) | None => None,
        }
    }
}

fn installed_revisions(listing: &str) -> BTreeMap<CrateName, Revision> {
    listing
        .lines()
        .filter(|line| !line.starts_with(char::is_whitespace))
        .filter_map(installed_crate_line)
        .filter_map(|(name, source)| match source {
            InstalledFrom::Git { commit, .. } => {
                Some((CrateName::from(name), Revision::from(commit)))
            }
            InstalledFrom::Registry | InstalledFrom::Path(_) => None,
        })
        .collect()
}

/// A listing a tool could not produce is one failure, not one per resource that needed it, so the
/// reason is held and reported against each of them.
fn read_listing(
    is_needed: bool,
    invocation: ReadInvocation,
    machine: &impl ReadMachine,
) -> Option<Result<String, DriftReason>> {
    let tool = invocation.tool();
    if !is_needed || !machine.tool_is_present(tool) {
        return None;
    }

    Some(match machine.read(&invocation) {
        Ok(output) if output.succeeded => Ok(output.standard_output),
        Ok(output) => {
            Err(format!("{tool} could not be read: {}", output.standard_error.trim()).into())
        }
        Err(error) => Err(format!("{tool} could not be read: {error}").into()),
    })
}

/// Reads the actual state of one resource and compares it against what was declared.
///
/// Requirements are read from the machine first: a resource whose requirements are absent is
/// neither converged nor failed, it is unassessable and says so.
pub fn assess(
    resource: &Resource,
    machine: &impl ReadMachine,
    readings: &SourceReadings,
) -> Assessment {
    if let Some(unmet) = first_unmet_requirement(resource, machine) {
        return Assessment::Unassessable(unmet);
    }

    match resource {
        Resource::Repository(repository) => assess_repository(repository, machine),
        Resource::Application(application) => assess_application(application, machine),
        Resource::Package(Package::Winget(package)) => assess_winget_package(package, readings),
        Resource::Package(Package::Cargo(package)) => assess_cargo_package(package, readings),
        Resource::Symlink(symlink) => assess_symlink(symlink, machine),
        Resource::Registration(Registration::ClaudeMcpServer(server)) => {
            assess_claude_mcp_server(server, machine)
        }
        Resource::Command(command) => assess_command(command, machine),
    }
}

fn first_unmet_requirement(resource: &Resource, machine: &impl ReadMachine) -> Option<Requirement> {
    resource
        .requirements()
        .into_iter()
        .find(|requirement| !requirement_is_met(*requirement, machine))
}

fn requirement_is_met(requirement: Requirement, machine: &impl ReadMachine) -> bool {
    match requirement {
        Requirement::Tool(tool) => machine.tool_is_present(tool),
        Requirement::DotfilesRepository => {
            machine.path_exists(&machine.dotfiles_repository_path().join(".git"))
        }
    }
}

fn assess_repository(repository: &GitHubRepository, machine: &impl ReadMachine) -> Assessment {
    let clone_directory = machine
        .repositories_directory()
        .join(repository.repository.as_ref());
    match machine.path_exists(&clone_directory.join(".git")) {
        true => Assessment::Converged,
        false => {
            Assessment::Drifted(format!("{} holds no clone", clone_directory.display()).into())
        }
    }
}

fn assess_application(application: &Application, machine: &impl ReadMachine) -> Assessment {
    match machine.check_presence(&application.presence_check) {
        Ok(true) => Assessment::Converged,
        Ok(false) => {
            Assessment::Drifted(format!("not installed — {}", application.presence_check).into())
        }
        Err(error) => Assessment::Drifted(format!("presence could not be read: {error}").into()),
    }
}

fn assess_winget_package(package: &WingetPackage, readings: &SourceReadings) -> Assessment {
    let listing = match &readings.winget_packages {
        Some(Ok(listing)) => listing,
        Some(Err(reason)) => return Assessment::Drifted(reason.clone()),
        None => return Assessment::Drifted("winget was not read for this change set".into()),
    };

    match winget_lists_package(listing, package.id.to_string().as_str()) {
        Ok(true) => Assessment::Converged,
        Ok(false) => Assessment::Drifted("winget reports it as not installed".into()),
        Err(reason) => Assessment::Drifted(reason),
    }
}

/// The character winget puts at the end of a value it had to cut short.
const TRUNCATION_MARKER: char = '…';

/// winget publishes no machine-readable listing, so the `Id` column is located from the header row
/// and read out of every row at that position. Columns are sized to the data whenever winget's
/// output is redirected, which is the only way this crate runs it, so a value is not expected to
/// be cut short — a listing that cuts one short is reported as unreadable rather than allowed to
/// read as an absent package.
///
/// The header is matched on its English labels. A listing whose columns cannot be located is
/// unreadable, which is why a machine that reports them in another language fails loudly here
/// instead of reporting every declared package as missing. See ADR 0010.
fn winget_lists_package(listing: &str, id: &str) -> Result<bool, DriftReason> {
    let Some((first_column, last_column)) = winget_id_column(listing) else {
        return Err("winget's listing has no Id column, so it could not be read".into());
    };

    let mut lists_it = false;
    for line in listing.lines() {
        let cell: String = line
            .chars()
            .skip(first_column)
            .take(last_column - first_column)
            .collect();
        let cell = cell.trim();

        if cell.contains(TRUNCATION_MARKER) {
            return Err("winget cut an Id short, so its listing could not be read".into());
        }
        if cell == id {
            lists_it = true;
        }
    }

    Ok(lists_it)
}

/// Where the `Id` column starts and ends, counted in characters rather than bytes so that a
/// package whose name is not ASCII does not shift every column to its right.
fn winget_id_column(listing: &str) -> Option<(usize, usize)> {
    listing.lines().find_map(|line| {
        let characters_before = |byte: usize| line[..byte].chars().count();
        let name = line.find("Name")?;
        let id = line[name..].find("Id").map(|offset| name + offset)?;
        let version = line[id..].find("Version").map(|offset| id + offset)?;
        Some((characters_before(id), characters_before(version)))
    })
}

fn assess_cargo_package(package: &CargoPackage, readings: &SourceReadings) -> Assessment {
    match &package.source {
        CargoSource::Workspace { repository } => {
            assess_workspace_member(&package.crate_name, repository, readings)
        }
        CargoSource::Registry | CargoSource::Path { .. } => {
            assess_declared_cargo_package(package, readings)
        }
    }
}

fn assess_workspace_member(
    crate_name: &CrateName,
    repository: &GitHubRepository,
    readings: &SourceReadings,
) -> Assessment {
    let reading = match readings.workspace(repository) {
        Some(Ok(Some(reading))) => reading,
        Some(Ok(None)) => {
            return Assessment::Drifted("its repository has not been cloned".into());
        }
        Some(Err(reason)) => return Assessment::Drifted(reason.clone()),
        None => {
            return Assessment::Drifted("its workspace was not read for this change set".into());
        }
    };

    let Some(member) = reading.members.get(crate_name) else {
        return Assessment::Drifted("the workspace no longer holds it".into());
    };
    let Some(installed) = &member.installed else {
        return Assessment::Drifted("cargo has not installed it".into());
    };

    match member.desired.difference_from(installed) {
        None => Assessment::Converged,
        Some(difference) => Assessment::Drifted(difference.into()),
    }
}

fn assess_declared_cargo_package(package: &CargoPackage, readings: &SourceReadings) -> Assessment {
    let installed = match &readings.cargo_crates {
        Some(Ok(listing)) => listing,
        Some(Err(reason)) => return Assessment::Drifted(reason.clone()),
        None => return Assessment::Drifted("cargo was not read for this change set".into()),
    };

    let Some(actual) = installed_crate_source(installed, package.crate_name.as_ref()) else {
        return Assessment::Drifted("cargo has not installed it".into());
    };

    match (&package.source, &actual) {
        (CargoSource::Registry, InstalledFrom::Registry) => Assessment::Converged,
        (CargoSource::Path { path }, InstalledFrom::Path(installed_path))
            if paths_are_the_same(path, installed_path) =>
        {
            Assessment::Converged
        }
        (_, actual) => Assessment::Drifted(format!("installed from {actual}").into()),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum InstalledFrom {
    Registry,
    Path(String),
    /// The commit is what the install actually resolved to, which is the fact drift is read
    /// against; `cargo install --list` abbreviates it.
    Git {
        url: String,
        commit: String,
    },
}

impl std::fmt::Display for InstalledFrom {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstalledFrom::Registry => formatter.write_str("the registry"),
            InstalledFrom::Path(path) => write!(formatter, "{path}"),
            InstalledFrom::Git { url, commit } => write!(formatter, "{url} at {commit}"),
        }
    }
}

/// `cargo install --list` names each crate on a line of its own, indenting the binaries it
/// installed underneath. A registry install is bare — `committed v1.1.11:` — while anything else
/// carries its source in parentheses: `ci-checks v0.1.0 (C:\path\to\crate):` for a path, and
/// `stop-gate v0.1.0 (https://host/owner/repo?rev=<asked>#<resolved>):` for a git revision.
fn installed_crate_line(line: &str) -> Option<(&str, InstalledFrom)> {
    let (name, remainder) = line.trim_end().split_once(' ')?;

    let Some((_, source)) = remainder.split_once('(') else {
        return Some((name, InstalledFrom::Registry));
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

fn paths_are_the_same(declared: &std::path::Path, installed: &str) -> bool {
    let installed = std::path::Path::new(installed);
    match (declared.canonicalize(), installed.canonicalize()) {
        (Ok(declared), Ok(installed)) => declared == installed,
        _ => declared == installed,
    }
}

fn assess_symlink(symlink: &Symlink, machine: &impl ReadMachine) -> Assessment {
    let link_path = machine.resolve_against_home(&symlink.link_path);
    let source_path = machine
        .dotfiles_repository_path()
        .join(&symlink.source_path);

    match machine.link_target(&link_path) {
        None if machine.path_exists(&link_path) => {
            Assessment::Drifted(format!("{} is not a link", link_path.display()).into())
        }
        None => Assessment::Drifted(format!("{} is missing", link_path.display()).into()),
        Some(target) if target == source_path => Assessment::Converged,
        Some(target) => {
            Assessment::Drifted(format!("links to {} instead", target.display()).into())
        }
    }
}

fn assess_claude_mcp_server(server: &ClaudeMcpServer, machine: &impl ReadMachine) -> Assessment {
    let invocation = ReadInvocation::ClaudeMcpServer {
        name: server.name.clone(),
    };
    let output = match machine.read(&invocation) {
        Ok(output) => output,
        Err(error) => {
            return Assessment::Drifted(format!("claude could not be read: {error}").into());
        }
    };

    if !output.succeeded {
        return Assessment::Drifted("claude holds no such server".into());
    }

    match first_difference(server, &output.standard_output) {
        None => Assessment::Converged,
        Some(difference) => Assessment::Drifted(difference.into()),
    }
}

/// `claude mcp get <name>` reports the server as indented `Label: value` lines, with each
/// environment entry on a line of its own beneath `Environment:`.
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

    let registered_args = field("Args").unwrap_or_default();
    if registered_args != server.args.join(" ") {
        return Some(format!("registered with the arguments {registered_args}"));
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
        Ok(true) => Assessment::Converged,
        Ok(false) => Assessment::Drifted(format!("not yet done — {check}").into()),
        Err(error) => Assessment::Drifted(format!("presence could not be read: {error}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Taken verbatim from `cargo install --list`, so the parser is exercised against the shape
    /// cargo actually emits rather than one assumed for it.
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
    fn a_crate_installed_from_the_registry_is_reported_as_coming_from_it() {
        assert_eq!(
            installed_crate_source(LISTING, "committed"),
            Some(InstalledFrom::Registry)
        );
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

    /// Taken verbatim from `winget list`, narrowed to three packages, so the parser is exercised
    /// against the shape winget actually emits rather than one assumed for it.
    const PACKAGES: &str = concat!(
        "Name                 Id                        Version    Available  Source\n",
        "-----------------------------------------------------------------------------\n",
        "Battle for Wesnoth   Wesnoth.BattleForWesnoth  Unknown    1.18.4     winget\n",
        "Bitwarden            Bitwarden.Bitwarden       2026.3.1   2026.7.0   winget\n",
        "AMD Software         ARP\\Machine\\X64\\AMD Cat   26.7.1                     \n",
    );

    #[test]
    fn a_package_winget_lists_is_found() {
        assert_eq!(
            winget_lists_package(PACKAGES, "Bitwarden.Bitwarden"),
            Ok(true)
        );
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

    #[test]
    fn the_commit_a_crate_resolved_to_is_read_out_of_the_listing() {
        let listing = concat!(
            "committed v1.1.11:\n",
            "    committed.exe\n",
            "stop-gate v0.1.0 (https://github.com/Alice/dotfiles?rev=426d343#2ae2ffff):\n",
            "    stop-gate.exe\n",
        );

        let revisions = installed_revisions(listing);

        assert_eq!(
            revisions.get(&CrateName::from("stop-gate")),
            Some(&Revision::from("2ae2ffff"))
        );
    }

    #[test]
    fn a_crate_installed_from_the_registry_names_no_commit_to_compare_against() {
        let listing = "committed v1.1.11:\n    committed.exe\n";

        assert!(installed_revisions(listing).is_empty());
    }
}

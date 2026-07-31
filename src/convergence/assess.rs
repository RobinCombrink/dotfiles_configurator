use crate::{
    configuration::{
        Application, CargoPackage, CargoSource, ClaudeMcpServer, Command, Package, Registration,
        RepositoryName, Resource, Symlink, WingetPackage,
    },
    convergence::{Assessment, Requirement},
    machine::{ReadInvocation, ReadMachine},
};

/// Reads the actual state of one resource and compares it against what was declared.
///
/// Requirements are read from the machine first: a resource whose requirements are absent is
/// neither converged nor failed, it is unassessable and says so.
pub fn assess(resource: &Resource, machine: &impl ReadMachine) -> Assessment {
    if let Some(unmet) = first_unmet_requirement(resource, machine) {
        return Assessment::Unassessable(unmet);
    }

    match resource {
        Resource::Repository(repository) => assess_repository(repository, machine),
        Resource::Application(application) => assess_application(application, machine),
        Resource::Package(Package::Winget(package)) => assess_winget_package(package, machine),
        Resource::Package(Package::Cargo(package)) => assess_cargo_package(package, machine),
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

fn assess_repository(repository: &RepositoryName, machine: &impl ReadMachine) -> Assessment {
    let clone_directory = machine.repositories_directory().join(&repository.repo);
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
            Assessment::Drifted(format!("not installed: {}", application.presence_check).into())
        }
        Err(error) => Assessment::Drifted(format!("presence could not be read: {error}").into()),
    }
}

fn assess_winget_package(package: &WingetPackage, machine: &impl ReadMachine) -> Assessment {
    let invocation = ReadInvocation::WingetPackage {
        id: package.id.clone(),
    };
    match machine.read(&invocation) {
        Ok(output) if output.succeeded => Assessment::Converged,
        Ok(_) => Assessment::Drifted("winget reports it as not installed".into()),
        Err(error) => Assessment::Drifted(format!("winget could not be read: {error}").into()),
    }
}

fn assess_cargo_package(package: &CargoPackage, machine: &impl ReadMachine) -> Assessment {
    let installed = match machine.read(&ReadInvocation::CargoInstalledCrates) {
        Ok(output) => output.standard_output,
        Err(error) => {
            return Assessment::Drifted(format!("cargo could not be read: {error}").into());
        }
    };

    let Some(actual) = installed_crate_source(&installed, &package.crate_name) else {
        return Assessment::Drifted("cargo has not installed it".into());
    };

    match (&package.source, &actual) {
        (CargoSource::Registry, InstalledFrom::Registry) => Assessment::Converged,
        (CargoSource::Path { path }, InstalledFrom::Path(installed_path))
            if paths_are_the_same(path, installed_path) =>
        {
            Assessment::Converged
        }
        (CargoSource::Git { revision, .. }, InstalledFrom::Git { commit, .. })
            if revisions_agree(revision, commit) =>
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
fn installed_crate_source(listing: &str, crate_name: &str) -> Option<InstalledFrom> {
    listing
        .lines()
        .filter(|line| !line.starts_with(char::is_whitespace))
        .find_map(|line| {
            let (name, remainder) = line.trim_end().split_once(' ')?;
            if name != crate_name {
                return None;
            }

            let Some((_, source)) = remainder.split_once('(') else {
                return Some(InstalledFrom::Registry);
            };
            let source = source.trim_end_matches([')', ':']);

            match source.split_once('#') {
                Some((location, commit)) => Some(InstalledFrom::Git {
                    url: location
                        .split_once('?')
                        .map_or(location, |(url, _)| url)
                        .to_owned(),
                    commit: commit.to_owned(),
                }),
                None => Some(InstalledFrom::Path(source.to_owned())),
            }
        })
}

/// The listing abbreviates the resolved commit, so a declared revision agrees with it when either
/// is a prefix of the other.
fn revisions_agree(declared: &str, installed: &str) -> bool {
    !declared.is_empty() && (declared.starts_with(installed) || installed.starts_with(declared))
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
        Ok(false) => Assessment::Drifted(format!("not yet done: {check}").into()),
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

    #[test]
    fn a_full_revision_agrees_with_the_abbreviated_commit_the_listing_reports() {
        assert!(revisions_agree(
            "2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c",
            "2ae2ffff"
        ));
    }

    #[test]
    fn a_revision_that_resolved_to_a_different_commit_does_not_agree() {
        assert!(!revisions_agree(
            "2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c",
            "9f31a0c2"
        ));
    }
}

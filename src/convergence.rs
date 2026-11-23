use {
    crate::{
        configuration::{
            AnsweringCandidate, CrateVersion, DotnetToolVersion, GitHubRepository, LinkSource,
            Migration, Notice, PresenceCheck, Requirement, Resource, Symlink, UvToolVersion,
            VariableValue, WingetVersion,
        },
        desired_state::{
            ChangeSetResource, DesiredState, Precedence, ResolvedResource, ResolvedWorkspace,
        },
        machine::dart_reading::DartDrift,
        reporting::RunReport,
        version::Version,
    },
    assess::{InstalledFrom, ServerDifference, WantedVersion},
    attribution::{Decided, DeclaredClone, decided, unclaimed_binaries},
    member_currency::{BinaryDrift, SearchPathFinding},
    std::{collections::BTreeSet, fmt::Display, path::PathBuf},
};

pub mod apply;
pub mod assess;
pub mod attribution;
mod build_stage;
pub mod converge;
pub mod lane;
pub mod member_currency;
pub mod resolve;
pub mod source_reading;
pub mod withdrawal;

pub use {
    apply::{ApplyOutcome, Enactment},
    assess::{SourceReadings, assess},
    converge::install_release,
    lane::Lane,
    resolve::{Resolution, resolve},
    source_reading::{ReadSource, SourceReading, UnreadableReason},
    withdrawal::{Withdrawal, dangling_links},
};

#[derive(Debug, Clone)]
pub enum Assessment {
    Converged,
    // ADR 0032
    Found(Finding),
    Drifted(DriftReason),
    Unassessable(Impediment),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    AnsweredBy(AnsweringCandidate),
    OnSearchPath(Vec<SearchPathFinding>),
}

impl Display for Finding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Finding::AnsweredBy(candidate) => Display::fmt(candidate, formatter),
            Finding::OnSearchPath(findings) => write_joined(formatter, findings),
        }
    }
}

fn write_joined(
    formatter: &mut std::fmt::Formatter<'_>,
    items: &[impl Display],
) -> std::fmt::Result {
    for (position, item) in items.iter().enumerate() {
        if position > 0 {
            formatter.write_str("; ")?;
        }
        Display::fmt(item, formatter)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub enum Impediment {
    // ADR 0004
    Absent(Requirement),
    ActualStateUnreadable(UnreadableReason),
}

impl Display for Impediment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Impediment::Absent(requirement) => Display::fmt(requirement, formatter),
            Impediment::ActualStateUnreadable(reason) => Display::fmt(reason, formatter),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DriftReason {
    NoClone {
        clone_directory: PathBuf,
    },
    ShallowClone {
        clone_directory: PathBuf,
    },
    NotInstalled {
        presence_check: PresenceCheck,
    },
    NotYetDone {
        presence_check: PresenceCheck,
    },
    NoPresenceCheck,
    ReleaseNotInstalled {
        installed_path: PathBuf,
        repository: GitHubRepository,
        released: Version,
    },
    BehindRelease {
        installed: Version,
        repository: GitHubRepository,
        released: Version,
    },
    WingetHoldsNothing,
    WingetOffersNewer {
        installed: String,
        offered: WingetVersion,
    },
    WingetAtAnotherVersion {
        installed: String,
        declared: WingetVersion,
    },
    UvHoldsNothing,
    UvAtAnotherVersion {
        installed: UvToolVersion,
        declared: UvToolVersion,
    },
    UvBehindNewest {
        installed: UvToolVersion,
        newest: UvToolVersion,
    },
    DotnetHoldsNothing,
    DotnetAtAnotherVersion {
        installed: DotnetToolVersion,
        declared: DotnetToolVersion,
    },
    DotnetBehindNewest {
        installed: DotnetToolVersion,
        newest: DotnetToolVersion,
    },
    CargoHoldsNothing,
    CargoAtAnotherVersion {
        installed: CrateVersion,
        wanted: WantedVersion,
    },
    CargoInstalledFrom(InstalledFrom),
    Dart(DartDrift),
    VariableSetTo(VariableValue),
    VariableUnset,
    NotOnSearchPath {
        directory: PathBuf,
    },
    #[cfg(target_family = "windows")]
    NotFirstOnSearchPath {
        directory: PathBuf,
    },
    NotALink {
        link_path: PathBuf,
    },
    LinkMissing {
        link_path: PathBuf,
    },
    LinksElsewhere {
        target: PathBuf,
        source_path: PathBuf,
    },
    NoManifest,
    ManifestDiffers,
    NoSuchServer,
    Server(ServerDifference),
    OwnCopies(Vec<BinaryDrift>),
}

impl Display for DriftReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriftReason::NoClone { clone_directory } => {
                write!(formatter, "{} holds no clone", clone_directory.display())
            }
            DriftReason::ShallowClone { clone_directory } => write!(
                formatter,
                "{} holds a shallow clone rather than the whole history",
                clone_directory.display()
            ),
            DriftReason::NotInstalled { presence_check } => {
                write!(formatter, "not installed — {presence_check}")
            }
            DriftReason::NotYetDone { presence_check } => {
                write!(formatter, "not yet done — {presence_check}")
            }
            DriftReason::NoPresenceCheck => {
                formatter.write_str("declares no presence check, so it runs every time")
            }
            DriftReason::ReleaseNotInstalled {
                installed_path,
                repository,
                released,
            } => write!(
                formatter,
                "not installed — nothing at {}, and the latest release of {repository} is \
                 {released}",
                installed_path.display()
            ),
            DriftReason::BehindRelease {
                installed,
                repository,
                released,
            } => write!(
                formatter,
                "{installed} is installed, and the latest release of {repository} is {released}"
            ),
            DriftReason::WingetHoldsNothing => {
                formatter.write_str("winget reports it as not installed")
            }
            DriftReason::WingetOffersNewer { installed, offered } => {
                write!(
                    formatter,
                    "{installed} is installed, and winget offers {offered}"
                )
            }
            DriftReason::WingetAtAnotherVersion {
                installed,
                declared,
            } => write!(
                formatter,
                "{installed} is installed, and the version declared is {declared}"
            ),
            DriftReason::UvHoldsNothing => formatter.write_str("uv has not installed it"),
            DriftReason::UvAtAnotherVersion {
                installed,
                declared,
            } => write!(
                formatter,
                "{installed} is installed, and the version declared is {declared}"
            ),
            DriftReason::UvBehindNewest { installed, newest } => write!(
                formatter,
                "{installed} is installed, and the newest version that resolves is {newest}"
            ),
            DriftReason::DotnetHoldsNothing => {
                formatter.write_str("dotnet has not installed it as a global tool")
            }
            DriftReason::DotnetAtAnotherVersion {
                installed,
                declared,
            } => write!(
                formatter,
                "{installed} is installed, and the version declared is {declared}"
            ),
            DriftReason::DotnetBehindNewest { installed, newest } => write!(
                formatter,
                "{installed} is installed, and the newest version NuGet publishes is {newest}"
            ),
            DriftReason::CargoHoldsNothing => formatter.write_str("cargo has not installed it"),
            DriftReason::CargoAtAnotherVersion { installed, wanted } => {
                write!(formatter, "{installed} is installed, and {wanted}")
            }
            DriftReason::CargoInstalledFrom(installed_from) => {
                write!(formatter, "installed from {installed_from}")
            }
            DriftReason::Dart(drift) => Display::fmt(drift, formatter),
            DriftReason::VariableSetTo(value) => write!(formatter, "is set to {value}"),
            DriftReason::VariableUnset => formatter.write_str("is not set"),
            DriftReason::NotOnSearchPath { directory } => {
                write!(formatter, "{} is not on it", directory.display())
            }
            #[cfg(target_family = "windows")]
            DriftReason::NotFirstOnSearchPath { directory } => {
                write!(
                    formatter,
                    "{} comes after another entry",
                    directory.display()
                )
            }
            DriftReason::NotALink { link_path } => {
                write!(formatter, "{} is not a link", link_path.display())
            }
            DriftReason::LinkMissing { link_path } => {
                write!(formatter, "{} is missing", link_path.display())
            }
            DriftReason::LinksElsewhere {
                target,
                source_path,
            } => write!(
                formatter,
                "links to {} instead of {}",
                target.display(),
                source_path.display()
            ),
            DriftReason::NoManifest => formatter.write_str("the machine holds no manifest"),
            DriftReason::ManifestDiffers => formatter.write_str("the manifest says something else"),
            DriftReason::NoSuchServer => formatter.write_str("claude holds no such server"),
            DriftReason::Server(difference) => Display::fmt(difference, formatter),
            DriftReason::OwnCopies(binaries) => write_joined(formatter, binaries),
        }
    }
}

// ADR 0025
pub(crate) fn search_path_directory(
    entry: &crate::configuration::SearchPathEntry,
    resource: &ResolvedResource,
    machine: &impl crate::machine::ReadMachine,
) -> std::path::PathBuf {
    match &entry.directory {
        crate::configuration::SearchPathDirectory::ToolBinaries => machine.binaries_directory(),
        crate::configuration::SearchPathDirectory::Repository { repository, path } => {
            resource.clone_directory(repository).join(path)
        }
        crate::configuration::SearchPathDirectory::Home { path } => {
            machine.resolve_against_home(path)
        }
    }
}

pub(crate) fn symlink_location(
    symlink: &Symlink,
    resource: &ResolvedResource,
    machine: &impl crate::machine::ReadMachine,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let source_path = match &symlink.source {
        LinkSource::DotfilesRepository(source_path) => resource.files_root().join(source_path),
        LinkSource::DeclaredRepository(within) => resource
            .clone_directory(&within.repository)
            .join(within.path.as_path()),
    };
    (
        machine.resolve_against_home(&symlink.link_path),
        source_path,
    )
}

#[derive(Debug, Clone)]
pub struct Change {
    pub resource: ChangeSetResource,
    pub reason: DriftReason,
}

#[derive(Debug, Clone)]
pub struct Blocked {
    pub resource: ChangeSetResource,
    pub impediment: Impediment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub resource: ChangeSetResource,
    pub finding: Finding,
}

#[derive(Debug, Clone)]
pub struct ChangeSet {
    pub changes: Vec<Change>,
    pub withdrawals: Vec<Withdrawal>,
    pub blocked: Vec<Blocked>,
    pub uncloned: Vec<ResolvedWorkspace>,
    pub converged: Vec<ChangeSetResource>,
    pub found: Vec<Found>,
    pub notices: Vec<Notice>,
    pub migrations: Vec<Migration>,
}

impl ChangeSet {
    pub fn is_converged(&self) -> bool {
        self.changes.is_empty()
            && self.withdrawals.is_empty()
            && self.blocked.is_empty()
            && self.uncloned.is_empty()
    }

    // ADR 0013
    pub fn would_enact_something(&self) -> bool {
        !self.changes.is_empty() || !self.withdrawals.is_empty() || !self.migrations.is_empty()
    }
}

// ADR 0004
// ADR 0010
pub async fn plan(
    desired_state: &DesiredState,
    machine: &impl crate::machine::ReadMachine,
    report: &RunReport,
) -> anyhow::Result<(ChangeSet, SourceReadings)> {
    let readings = {
        let _doing = report.doing("reading what the machine already has");
        SourceReadings::read_for(desired_state, machine).await
    };
    let Resolution {
        resources,
        uncloned,
    } = resolve(desired_state, &readings, machine.home_directory())?;

    let mut assessed: Vec<(Precedence, usize, ChangeSetResource, Assessment)> = resources
        .iter()
        .enumerate()
        .map(|(position, resource)| {
            let _doing = report.doing(format!("reading {resource}"));
            (
                resource.precedence(),
                position,
                resource.clone(),
                assess(resource, machine, &readings),
            )
        })
        .collect();

    assessed.sort_by_key(|(precedence, position, _, _)| (*precedence, *position));

    let mut changes = Vec::new();
    let mut blocked = Vec::new();
    let mut converged = Vec::new();
    let mut found = Vec::new();
    for (_, _, resource, assessment) in assessed {
        match assessment {
            Assessment::Converged => converged.push(resource),
            Assessment::Found(finding) => {
                converged.push(resource.clone());
                found.push(Found { resource, finding });
            }
            Assessment::Drifted(reason) => changes.push(Change { resource, reason }),
            Assessment::Unassessable(impediment) => blocked.push(Blocked {
                resource,
                impediment,
            }),
        }
    }

    let mut notices: Vec<Notice> = desired_state
        .notices
        .iter()
        .map(|notice| Notice::Declared(notice.declared().clone()))
        .collect();
    notices.extend(desired_state.announcements.iter().cloned());
    notices.extend(
        machine
            .superseded_images()
            .into_iter()
            .map(Notice::SupersededImage),
    );
    let Decided {
        withdrawals,
        notices: unattributed,
    } = planned_withdrawals(desired_state, &readings, machine);
    notices.extend(unattributed);

    Ok((
        ChangeSet {
            changes,
            withdrawals: [withdrawals, planned_unlinks(desired_state, machine)].concat(),
            blocked,
            uncloned,
            converged,
            found,
            notices,
            migrations: desired_state.migrations.clone(),
        },
        readings,
    ))
}

// ADR 0046
fn planned_withdrawals(
    desired_state: &DesiredState,
    readings: &SourceReadings,
    machine: &impl crate::machine::ReadMachine,
) -> Decided {
    let Ok(listing) = readings.cargo_listing() else {
        return Decided::default();
    };
    let mut clones = Vec::new();
    let mut declared = BTreeSet::new();
    for workspace in &desired_state.workspaces {
        let repository = &workspace.declared().repository;
        let clone_directory = workspace.clone_directory(repository);
        match readings.workspace(&clone_directory) {
            Ok(Some(reading)) => {
                declared.extend(
                    reading
                        .members
                        .values()
                        .flat_map(|member| member.binaries.iter().cloned()),
                );
                clones.push(DeclaredClone::Read {
                    workspace: repository.clone(),
                    clone_directory,
                });
            }
            Ok(None) => {}
            Err(_) => clones.push(DeclaredClone::Unread),
        }
    }

    let repositories: Vec<GitHubRepository> = desired_state
        .workspaces
        .iter()
        .map(|workspace| workspace.declared().repository.clone())
        .collect();
    let mut decided = decided(
        unclaimed_binaries(listing, &clones, &declared, machine),
        &declared,
    );
    decided.withdrawals.extend(withdrawal::untracked(
        withdrawal::install_records(listing),
        &repositories,
    ));
    decided
}

// ADR 0005
fn planned_unlinks(
    desired_state: &DesiredState,
    machine: &impl crate::machine::ReadMachine,
) -> Vec<Withdrawal> {
    let declared_links: Vec<PathBuf> = desired_state
        .resources
        .iter()
        .filter_map(|resource| match resource.declared() {
            Resource::Symlink(symlink) => Some(symlink_location(symlink, resource, machine).0),
            _ => None,
        })
        .collect();
    let repositories: BTreeSet<PathBuf> =
        desired_state
            .resources
            .iter()
            .map(|resource| resource.files_root().to_path_buf())
            .chain(desired_state.resources.iter().filter_map(
                |resource| match resource.declared() {
                    Resource::Symlink(Symlink {
                        source: LinkSource::DeclaredRepository(within),
                        ..
                    }) => Some(resource.clone_directory(&within.repository)),
                    _ => None,
                },
            ))
            .chain(
                desired_state
                    .workspaces
                    .iter()
                    .map(|workspace| workspace.files_root().to_path_buf()),
            )
            .chain(
                desired_state
                    .notices
                    .iter()
                    .map(|notice| notice.files_root().to_path_buf()),
            )
            .collect();

    dangling_links(
        declared_links.iter().map(PathBuf::as_path),
        &repositories,
        machine,
    )
}

impl Display for ChangeSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for change in &self.changes {
            writeln!(
                formatter,
                "  change  {} ({})",
                change.resource, change.reason
            )?;
        }
        for withdrawal in &self.withdrawals {
            writeln!(formatter, "  remove  {withdrawal}")?;
        }
        for blocked in &self.blocked {
            writeln!(
                formatter,
                "  blocked {} ({})",
                blocked.resource, blocked.impediment
            )?;
        }
        for workspace in &self.uncloned {
            writeln!(
                formatter,
                "  uncloned {workspace} (its members are unknown until it is cloned)"
            )?;
        }
        for found in &self.found {
            writeln!(
                formatter,
                "  found   {} ({})",
                found.resource, found.finding
            )?;
        }
        for migration in &self.migrations {
            writeln!(formatter, "  migrate {migration}")?;
        }
        for notice in &self.notices {
            writeln!(formatter, "  notice  {notice}")?;
        }
        write!(
            formatter,
            "\n{} to change, {} to remove, {} blocked, {} uncloned, {} already converged, {} to \
             migrate",
            self.changes.len(),
            self.withdrawals.len(),
            self.blocked.len(),
            self.uncloned.len(),
            self.converged.len(),
            self.migrations.len()
        )
    }
}

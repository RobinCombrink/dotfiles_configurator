use {
    crate::{
        configuration::{
            CrateVersion, GitHubRepository, Migration, Notice, PresenceCheck, Requirement, Symlink,
            UvToolVersion, VariableValue, WingetVersion,
        },
        desired_state::{
            ChangeSetResource, DesiredState, Precedence, ResolvedResource, ResolvedWorkspace,
        },
        machine::dart_reading::DartDrift,
        reporting::RunReport,
        version::Version,
    },
    assess::{InstalledFrom, ServerDifference, WantedVersion},
    member_currency::BinaryDrift,
    std::{fmt::Display, path::PathBuf},
};

pub mod apply;
pub mod assess;
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
    withdrawal::{Withdrawal, withdrawals},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    Converged,
    // ADR 0032
    Found(Finding),
    Drifted(DriftReason),
    Unassessable(Impediment),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding(String);

impl From<String> for Finding {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Display for Finding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
            DriftReason::OwnCopies(binaries) => {
                for (position, binary) in binaries.iter().enumerate() {
                    if position > 0 {
                        formatter.write_str("; ")?;
                    }
                    Display::fmt(binary, formatter)?;
                }
                Ok(())
            }
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
    (
        machine.resolve_against_home(&symlink.link_path),
        resource.files_root().join(&symlink.source_path),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub resource: ChangeSetResource,
    pub reason: DriftReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
    pub resource: ChangeSetResource,
    pub impediment: Impediment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub resource: ChangeSetResource,
    pub finding: Finding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

    Ok((
        ChangeSet {
            changes,
            withdrawals: planned_withdrawals(desired_state, &readings),
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

// ADR 0041
fn planned_withdrawals(desired_state: &DesiredState, readings: &SourceReadings) -> Vec<Withdrawal> {
    let Ok(records) = readings.install_records() else {
        return Vec::new();
    };
    let workspaces = desired_state.workspaces.iter().filter_map(|workspace| {
        let repository = &workspace.declared().repository;
        let reading = readings
            .workspace(&workspace.clone_directory(repository))
            .ok()??;
        Some((repository, reading))
    });

    withdrawals(workspaces, &records)
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

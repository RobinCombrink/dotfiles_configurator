use {
    crate::{
        configuration::{BinaryName, CrateName, GitHubRepository, path_folding},
        convergence::Lane,
        machine::{ReadMachine, RecordUntracking, WriteMachine, workspace_reading::Revision},
        reporting::{Entry, EntryOutcome, RunReport},
    },
    anyhow::Result,
    std::{
        collections::BTreeSet,
        fmt::Display,
        path::{Path, PathBuf},
    },
    url::Url,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstallRecord {
    pub crate_name: CrateName,
    pub version: String,
    pub source: String,
    pub binary_files: Vec<String>,
}

impl InstallRecord {
    // 2026-10-05: `cargo uninstall` tells two records of one package apart only by the whole
    // specification `git+<source as cargo lists it>#<name>@<version>`; the bare `<name>@<version>`
    // is refused as ambiguous. cargo 1.99.0 on Windows 11.
    pub fn specification(&self) -> String {
        format!("git+{}#{}@{}", self.source, self.crate_name, self.version)
    }

    pub fn was_installed_from(&self, repository: &GitHubRepository) -> bool {
        let Ok(url) = Url::parse(&self.source) else {
            return false;
        };
        let path = url.path().trim_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);

        url.host_str() == Some("github.com")
            && path.eq_ignore_ascii_case(&format!("{}/{}", repository.owner, repository.repository))
    }
}

// 2026-10-05: `cargo install --list` names each record on a line of its own as
// `<name> v<version> (<source>#<short commit>):` for one installed from git, with each binary file
// it installed indented beneath. cargo 1.99.0 on Windows 11.
pub fn install_records(listing: &str) -> Vec<InstallRecord> {
    let mut records: Vec<InstallRecord> = Vec::new();
    let mut in_a_git_record = false;
    for line in listing.lines() {
        if line.starts_with(char::is_whitespace) {
            if let (true, Some(record)) = (in_a_git_record, records.last_mut()) {
                record.binary_files.push(line.trim().to_owned());
            }
            continue;
        }

        let record = git_record_line(line);
        in_a_git_record = record.is_some();
        records.extend(record);
    }
    records
}

fn git_record_line(line: &str) -> Option<InstallRecord> {
    let (name, remainder) = line.trim_end().split_once(' ')?;
    let (version, source) = remainder.split_once(" (")?;
    let source = source.strip_suffix("):")?;
    let (source, _) = source.rsplit_once('#')?;

    Some(InstallRecord {
        crate_name: CrateName::from(name),
        version: version.strip_prefix('v').unwrap_or(version).to_owned(),
        source: source.to_owned(),
        binary_files: Vec::new(),
    })
}

pub fn untracked(
    records: Vec<InstallRecord>,
    repositories: &[GitHubRepository],
) -> Vec<Withdrawal> {
    records
        .into_iter()
        .filter_map(|record| {
            let workspace = repositories
                .iter()
                .find(|repository| record.was_installed_from(repository))?
                .clone();
            Some(Withdrawal::Untrack { record, workspace })
        })
        .collect()
}

pub fn claimed_binary_files(listing: &str) -> BTreeSet<String> {
    listing
        .lines()
        .filter(|line| line.starts_with(char::is_whitespace))
        .map(|line| line.trim().to_owned())
        .filter(|file| !file.is_empty())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Withdrawal {
    Remove {
        binary: BinaryName,
        workspace: GitHubRepository,
        commit: Revision,
    },
    Untrack {
        record: InstallRecord,
        workspace: GitHubRepository,
    },
    // ADR 0005
    Unlink {
        link_path: PathBuf,
        target: PathBuf,
        repository: PathBuf,
    },
}

impl Withdrawal {
    pub fn lane(&self) -> Lane {
        match self {
            Withdrawal::Remove { .. } | Withdrawal::Untrack { .. } => Lane::Cargo,
            Withdrawal::Unlink { .. } => Lane::Instant,
        }
    }

    fn entry(&self) -> Entry {
        match self {
            Withdrawal::Remove { binary, .. } => Entry::removal(self.lane(), binary),
            Withdrawal::Untrack { record, .. } => Entry::removal(
                self.lane(),
                format!("cargo's record of {}", record.crate_name),
            ),
            Withdrawal::Unlink { link_path, .. } => {
                Entry::removal(self.lane(), link_path.display())
            }
        }
    }
}

impl Display for Withdrawal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Withdrawal::Remove {
                binary,
                workspace,
                commit,
            } => write!(
                formatter,
                "{binary}, built from {commit} of {workspace}, which no member of a declared \
                 workspace declares"
            ),
            Withdrawal::Untrack { record, workspace } => write!(
                formatter,
                "cargo's record of {} {} from {workspace}, keeping its binaries, which answer \
                 for themselves by the commit they report",
                record.crate_name, record.version
            ),
            Withdrawal::Unlink {
                link_path,
                target,
                repository,
            } => write!(
                formatter,
                "the link {}, which nothing declares and whose target {} is gone from {}",
                link_path.display(),
                target.display(),
                repository.display()
            ),
        }
    }
}

pub fn dangling_links<'link>(
    declared_links: impl IntoIterator<Item = &'link Path>,
    repositories: &BTreeSet<PathBuf>,
    machine: &impl ReadMachine,
) -> Vec<Withdrawal> {
    let declared_links: Vec<&Path> = declared_links.into_iter().collect();
    let declared: BTreeSet<String> = declared_links
        .iter()
        .map(|link_path| path_folding::comparable(link_path))
        .collect();
    let directories: BTreeSet<&Path> = declared_links
        .iter()
        .filter_map(|link_path| link_path.parent())
        .collect();

    let mut withdrawals: Vec<Withdrawal> = directories
        .into_iter()
        .filter_map(|directory| machine.directory_entries(directory).ok())
        .flatten()
        .filter(|candidate| !declared.contains(&path_folding::comparable(candidate)))
        .filter_map(|link_path| {
            let target = machine.link_target(&link_path)?;
            let repository = repositories
                .iter()
                .find(|repository| target.starts_with(repository))?
                .clone();
            match machine.path_exists(&target) {
                true => None,
                false => Some(Withdrawal::Unlink {
                    link_path,
                    target,
                    repository,
                }),
            }
        })
        .collect();
    withdrawals.sort();
    withdrawals.dedup();
    withdrawals
}

#[derive(Debug, Default)]
pub struct Withdrawn {
    pub removed: Vec<Withdrawal>,
    pub attempted: BTreeSet<Withdrawal>,
}

impl Withdrawn {
    pub fn and(mut self, other: Withdrawn) -> Withdrawn {
        self.removed.extend(other.removed);
        self.attempted.extend(other.attempted);
        self
    }
}

pub async fn withdraw(
    withdrawals: Vec<&Withdrawal>,
    machine: &impl WriteMachine,
    report: &RunReport,
) -> Withdrawn {
    let mut withdrawn = Withdrawn::default();
    for withdrawal in withdrawals {
        let entry = withdrawal.entry();
        let removed = report
            .converging(&entry, Box::pin(remove(withdrawal, machine)))
            .await;

        match removed {
            Ok(()) => {
                report.note(&format!("removed {withdrawal}"));
                report.entry_finished(&entry, EntryOutcome::Converged);
                withdrawn.removed.push(withdrawal.clone());
            }
            Err(error) => {
                let reason = format!("{error:#}");
                report.note(&format!("FAILED to remove {withdrawal}: {reason}"));
                report.entry_finished(&entry, EntryOutcome::Failed { reason });
            }
        }
        withdrawn.attempted.insert(withdrawal.clone());
    }
    withdrawn
}

async fn remove(withdrawal: &Withdrawal, machine: &impl WriteMachine) -> Result<()> {
    match withdrawal {
        Withdrawal::Remove { binary, .. } => {
            machine.remove_binary(&machine.cargo_binaries_directory().join(binary.file_name()))
        }
        Withdrawal::Untrack { record, .. } => {
            machine
                .untrack_cargo_record(&RecordUntracking {
                    specification: record.specification(),
                    binary_files: record.binary_files.clone(),
                })
                .await
        }
        Withdrawal::Unlink { link_path, .. } => machine.remove_link(link_path),
    }
}

#[cfg(test)]
mod tests;

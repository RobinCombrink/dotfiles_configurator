use {
    crate::{
        configuration::{BinaryName, CrateName, GitHubRepository},
        machine::{WriteInvocation, WriteMachine, workspace_reading::WorkspaceReading},
        reporting::{Entry, EntryOutcome, RunReport},
    },
    anyhow::{Result, bail},
    std::{collections::BTreeSet, fmt::Display},
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

    fn was_installed_from(&self, repository: &GitHubRepository) -> bool {
        let Ok(url) = Url::parse(&self.source) else {
            return false;
        };
        let path = url.path().trim_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);

        url.host_str() == Some("github.com")
            && path.eq_ignore_ascii_case(&format!("{}/{}", repository.owner, repository.repository))
    }

    fn binary_named(&self, file: &str) -> BinaryName {
        BinaryName::from(
            file.strip_suffix(std::env::consts::EXE_SUFFIX)
                .unwrap_or(file),
        )
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

// ADR 0041
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Withdrawal {
    Remove {
        record: InstallRecord,
        binary: BinaryName,
    },
}

impl Display for Withdrawal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Withdrawal::Remove { record, binary } => write!(
                formatter,
                "{binary}, which {} no longer declares (installed from {})",
                record.crate_name, record.source
            ),
        }
    }
}

pub fn withdrawals<'reading>(
    workspaces: impl IntoIterator<Item = (&'reading GitHubRepository, &'reading WorkspaceReading)>,
    records: &[InstallRecord],
) -> Vec<Withdrawal> {
    let mut withdrawals = Vec::new();
    for (repository, reading) in workspaces {
        for record in records {
            let Some(member) = reading.members.get(&record.crate_name) else {
                continue;
            };
            if !record.was_installed_from(repository) {
                continue;
            }

            withdrawals.extend(
                record
                    .binary_files
                    .iter()
                    .map(|file| record.binary_named(file))
                    .filter(|binary| !member.binaries.contains(binary))
                    .map(|binary| Withdrawal::Remove {
                        record: record.clone(),
                        binary,
                    }),
            );
        }
    }
    withdrawals.sort();
    withdrawals.dedup();
    withdrawals
}

#[derive(Debug, Default)]
pub struct Withdrawn {
    pub removed: Vec<Withdrawal>,
    pub attempted: BTreeSet<Withdrawal>,
}

pub async fn withdraw(
    withdrawals: Vec<&Withdrawal>,
    machine: &impl WriteMachine,
    report: &RunReport,
) -> Withdrawn {
    let mut withdrawn = Withdrawn::default();
    for withdrawal in withdrawals {
        let Withdrawal::Remove { binary, .. } = withdrawal;
        let entry = Entry::removal(binary);
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
    let Withdrawal::Remove { record, binary } = withdrawal;
    let invocation = WriteInvocation::UninstallCargoBinary {
        specification: record.specification(),
        binary: binary.clone(),
    };

    let output = machine.attempt_write(&invocation).await?;
    if output.exited.succeeded() {
        return Ok(());
    }
    if !invocation.refused_as_corrupt(&output) {
        bail!(
            "cargo would not remove {binary}, {}: {}",
            output.exited,
            output.standard_error.trim()
        );
    }

    let binaries_directory = machine.cargo_binaries_directory();
    for file in &record.binary_files {
        let path = binaries_directory.join(file);
        if !machine.path_exists(&path) {
            machine.write_text_file(&path, "")?;
        }
    }

    let retried = machine.attempt_write(&invocation).await?;
    match retried.exited.succeeded() {
        true => Ok(()),
        false => bail!(
            "cargo would not remove {binary} even with every file its record names in place, {}: \
             {}",
            retried.exited,
            retried.standard_error.trim()
        ),
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            configuration::{RepositoryName, RepositoryOwner},
            machine::workspace_reading::{Fingerprint, MemberReading, ObjectHash, Revision},
        },
        std::collections::{BTreeMap, BTreeSet},
    };

    const EARLIER: &str = "0c532c68bd883d1e6b002ad5bb1f40aa2ed70f66";
    const LATER: &str = "e54928664eb4222b2ea79c7ef56126c5c58c7e01";

    fn file(binary: &str) -> String {
        BinaryName::from(binary).file_name()
    }

    fn listed(header: &str, binaries: &[&str]) -> String {
        let mut lines = format!("{header}\n");
        for binary in binaries {
            lines.push_str(&format!("    {}\n", file(binary)));
        }
        lines
    }

    fn listing_of_two_disjoint_records_of_one_member() -> String {
        [
            listed("committed v1.1.11:", &["committed"]),
            listed(
                &format!(
                    "session-mining v0.1.0 \
                     (https://Alice@github.com/Alice/dotfiles?rev={EARLIER}#0c532c68):"
                ),
                &["session-census", "reach"],
            ),
            listed(
                &format!(
                    "session-mining v0.1.0 \
                     (https://Alice@github.com/Alice/dotfiles?rev={LATER}#e5492866):"
                ),
                &["sweep", "tool-use-statistics"],
            ),
            listed(
                "stop-gate v0.1.0 (C:\\Repositories\\Alice\\dotfiles\\tools\\stop-gate):",
                &["stop-gate"],
            ),
        ]
        .concat()
    }

    fn dotfiles() -> GitHubRepository {
        GitHubRepository {
            owner: RepositoryOwner::from("Alice"),
            repository: RepositoryName::from("dotfiles"),
        }
    }

    fn workspace_declaring(crate_name: &str, binaries: &[&str]) -> WorkspaceReading {
        WorkspaceReading {
            revision: Revision::from(LATER),
            members: BTreeMap::from([(
                CrateName::from(crate_name),
                MemberReading {
                    desired: Fingerprint {
                        crate_subtree: ObjectHash::from("aaa"),
                        workspace_bindings: BTreeMap::new(),
                        lock_closure: ObjectHash::from("ccc"),
                        dependency_subtrees: BTreeMap::new(),
                    },
                    binaries: binaries
                        .iter()
                        .map(|name| BinaryName::from(*name))
                        .collect(),
                },
            )]),
        }
    }

    fn withdrawn_binaries(withdrawals: &[Withdrawal]) -> Vec<String> {
        withdrawals
            .iter()
            .map(|Withdrawal::Remove { binary, .. }| binary.to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    #[test]
    fn only_git_records_are_read_each_with_the_binaries_listed_beneath_it() {
        let records = install_records(&listing_of_two_disjoint_records_of_one_member());

        let read: Vec<(String, Vec<String>)> = records
            .iter()
            .map(|record| (record.crate_name.to_string(), record.binary_files.clone()))
            .collect();
        assert_eq!(
            read,
            vec![
                (
                    "session-mining".to_owned(),
                    vec![file("session-census"), file("reach")]
                ),
                (
                    "session-mining".to_owned(),
                    vec![file("sweep"), file("tool-use-statistics")]
                ),
            ]
        );
    }

    #[test]
    fn a_record_is_specified_by_its_whole_source_so_that_two_of_one_package_are_told_apart() {
        let records = install_records(&listing_of_two_disjoint_records_of_one_member());

        assert_eq!(
            records[0].specification(),
            format!(
                "git+https://Alice@github.com/Alice/dotfiles?rev={EARLIER}#session-mining@0.1.0"
            )
        );
    }

    #[test]
    fn the_binaries_an_earlier_record_of_a_member_holds_that_it_no_longer_declares_are_withdrawn() {
        let records = install_records(&listing_of_two_disjoint_records_of_one_member());
        let workspace = workspace_declaring(
            "session-mining",
            &["sweep", "tool-use-statistics", "session-census"],
        );

        let withdrawals = withdrawals([(&dotfiles(), &workspace)], &records);

        assert_eq!(withdrawn_binaries(&withdrawals), vec!["reach"]);
    }

    #[test]
    fn a_record_installed_from_another_repository_is_never_withdrawn() {
        let records = install_records(&listing_of_two_disjoint_records_of_one_member());
        let elsewhere = GitHubRepository {
            owner: RepositoryOwner::from("Bob"),
            repository: RepositoryName::from("dotfiles"),
        };

        let withdrawals = withdrawals(
            [(
                &elsewhere,
                &workspace_declaring("session-mining", &["sweep"]),
            )],
            &records,
        );

        assert!(withdrawals.is_empty(), "{withdrawals:?}");
    }

    #[test]
    fn a_record_of_a_crate_the_workspace_does_not_hold_is_never_withdrawn() {
        let records = install_records(&listing_of_two_disjoint_records_of_one_member());

        let withdrawals = withdrawals(
            [(
                &dotfiles(),
                &workspace_declaring("stop-gate", &["stop-gate"]),
            )],
            &records,
        );

        assert!(withdrawals.is_empty(), "{withdrawals:?}");
    }

    #[test]
    fn a_record_fetched_as_another_account_still_belongs_to_the_repository() {
        let listing = listed(
            &format!(
                "session-mining v0.1.0 \
                 (https://Carol@github.com/alice/Dotfiles?rev={EARLIER}#0c532c68):"
            ),
            &["reach"],
        );

        let withdrawals = withdrawals(
            [(
                &dotfiles(),
                &workspace_declaring("session-mining", &["sweep"]),
            )],
            &install_records(&listing),
        );

        assert_eq!(withdrawn_binaries(&withdrawals), vec!["reach"]);
    }
}

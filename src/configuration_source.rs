use {
    crate::{
        configuration::{
            Configuration, ConfigurationName, Context, GitHubAccount, GitHubRepository,
            MachineClass, MachineManifest, Migration, Notice, RepositoryName, RepositoryOwner,
            Unreadable, parse_configuration,
        },
        desired_state::{DesiredState, ResolvedConfiguration, SourceLocation},
        github::{self, GitHubAccess},
    },
    anyhow::{Context as _, Error, Result, anyhow, bail},
    std::{
        fmt::{Display, Formatter},
        fs,
        path::{Path, PathBuf},
        str::FromStr,
    },
};

pub trait WriteSource {
    fn rewrite(&self, migration: &Migration) -> Result<()>;
}

/// Where a configuration is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurationSource {
    LocalDirectory(PathBuf),
    GitHubRepository {
        repository: GitHubRepository,
        directory: String,
    },
}

impl FromStr for ConfigurationSource {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (kind, rest) = value
            .split_once(':')
            .ok_or_else(|| format!("{value:?} names no source kind; {EXPECTED_SOURCE}"))?;

        match kind {
            "local" => Ok(ConfigurationSource::LocalDirectory(PathBuf::from(rest))),
            "github" => {
                let mut segments = rest.splitn(3, '/');
                let (Some(owner), Some(repository), Some(directory)) =
                    (segments.next(), segments.next(), segments.next())
                else {
                    return Err(format!("{value:?} names no directory; {EXPECTED_SOURCE}"));
                };

                Ok(ConfigurationSource::GitHubRepository {
                    repository: GitHubRepository {
                        owner: RepositoryOwner::from(owner),
                        repository: RepositoryName::from(repository),
                    },
                    directory: directory.to_owned(),
                })
            }
            other => Err(format!("{other:?} is not a source kind; {EXPECTED_SOURCE}")),
        }
    }
}

const EXPECTED_SOURCE: &str = "expected `local:<directory>` or `github:<owner>/<repo>/<directory>`";

const CONFIGURATION_SUFFIX: &str = ".dotconfig.json";

/// A configuration as it was read, and what reading it a generation back left pending. A local
/// document is rewritten by an apply; one this program only reads is announced instead.
#[derive(Debug)]
struct LoadedConfiguration {
    name: ConfigurationName,
    configuration: Configuration,
    pending: Pending,
}

#[derive(Debug)]
enum Pending {
    Nothing,
    Rewriting(Migration),
    Announcing(Notice),
}

fn checkout_holding(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

fn is_configuration_file(path: &str) -> bool {
    path.ends_with(CONFIGURATION_SUFFIX)
}

pub async fn load_desired_state(
    sources: &[ConfigurationSource],
    machine: MachineClass,
    repositories_root: &Path,
    github: &GitHubAccess,
) -> Result<DesiredState, LoadFailure> {
    let mut per_source: Vec<(&ConfigurationSource, Vec<LoadedConfiguration>)> = Vec::new();
    let mut unreadable: Vec<Unreadable> = Vec::new();
    for source in sources {
        let mut from_this_source: Vec<LoadedConfiguration> = Vec::new();
        for attempt in source.load(github).await {
            match attempt {
                Ok(configuration) => from_this_source.push(configuration),
                Err(refusal) => unreadable.push(refusal),
            }
        }
        per_source.push((source, from_this_source));
    }

    if let Some(refusal) = Refusal::of(unreadable) {
        return Err(LoadFailure::Unreadable(refusal));
    }

    let mut read: Vec<(LoadedConfiguration, SourceLocation)> = Vec::new();
    for (source, from_this_source) in per_source {
        refuse_two_trees_for_one_source(source, &from_this_source)?;
        let location = source.files_come_from()?;
        read.extend(
            from_this_source
                .into_iter()
                .map(|loaded| (loaded, location.clone())),
        );
    }

    if read.is_empty() {
        return Err(LoadFailure::NoConfigurationFound(sources.to_vec()));
    }

    let applicable: Vec<(LoadedConfiguration, SourceLocation)> = read
        .into_iter()
        .filter(|(loaded, _)| loaded.configuration.applies_to.applies_on(machine))
        .collect();

    if applicable.is_empty() {
        return Err(LoadFailure::NoneAppliesTo(machine));
    }

    let mut migrations: Vec<Migration> = Vec::new();
    let mut announcements: Vec<Notice> = Vec::new();
    let mut resolved: Vec<ResolvedConfiguration> = Vec::new();
    for (loaded, location) in applicable {
        match loaded.pending {
            Pending::Nothing => {}
            Pending::Rewriting(migration) => migrations.push(migration),
            Pending::Announcing(notice) => announcements.push(notice),
        }
        resolved.push(ResolvedConfiguration::read(
            loaded.name,
            loaded.configuration,
            location,
            repositories_root,
        ));
    }

    let machine_manifest = MachineManifest {
        repositories_directory_path: repositories_root.join(machine.repositories_leaf()),
    };

    let home_directory = home_directory().map_err(LoadFailure::Irreconcilable)?;
    DesiredState::of(resolved, machine_manifest, &home_directory)
        .map(|desired_state| desired_state.also_reporting(migrations, announcements))
        .map_err(LoadFailure::Irreconcilable)
}

#[derive(Debug)]
pub enum LoadFailure {
    Unreadable(Refusal),
    NoConfigurationFound(Vec<ConfigurationSource>),
    NoneAppliesTo(MachineClass),
    // ADR 0025
    TwoTrees {
        source: ConfigurationSource,
        first: Context,
        second: Context,
    },
    SourceOutsideACheckout(PathBuf),
    Irreconcilable(Error),
}

impl LoadFailure {
    pub fn is_answered_by_a_newer_build(&self) -> bool {
        match self {
            LoadFailure::Unreadable(refusal) => {
                refusal.unreadable().iter().any(Unreadable::is_too_new)
            }
            LoadFailure::NoConfigurationFound(_)
            | LoadFailure::NoneAppliesTo(_)
            | LoadFailure::TwoTrees { .. }
            | LoadFailure::SourceOutsideACheckout(_)
            | LoadFailure::Irreconcilable(_) => false,
        }
    }
}

impl Display for LoadFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadFailure::Unreadable(refusal) => Display::fmt(refusal, formatter),
            LoadFailure::NoConfigurationFound(sources) => write!(
                formatter,
                "No configurations were found in any of the sources given: {}",
                sources
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            LoadFailure::NoneAppliesTo(machine) => write!(
                formatter,
                "No configuration in any of the sources given applies to {}",
                machine.described()
            ),
            LoadFailure::TwoTrees {
                source,
                first,
                second,
            } => write!(
                formatter,
                "{source} holds a configuration for {first}, which clones under {}, and one for                  {second}, which clones under {}. One source cannot be cloned into two trees.",
                first.repositories_leaf(),
                second.repositories_leaf()
            ),
            LoadFailure::SourceOutsideACheckout(directory) => write!(
                formatter,
                "{} is inside no checkout, so there is nothing to read a configuration's files                  out of. Read it from the repository it was written in instead.",
                directory.display()
            ),
            LoadFailure::Irreconcilable(fault) => write!(formatter, "{fault:#}"),
        }
    }
}

impl std::error::Error for LoadFailure {}

fn home_directory() -> Result<PathBuf> {
    std::env::home_dir()
        .ok_or_else(|| anyhow!("Could not find the home directory to resolve symlinks against"))
}

/// A source is cloned into the tree its configurations' context names, so one yielding two
/// contexts that name different trees would have to be cloned into both. See ADR 0025.
fn refuse_two_trees_for_one_source(
    source: &ConfigurationSource,
    loaded: &[LoadedConfiguration],
) -> Result<(), LoadFailure> {
    let contexts = loaded.iter().map(|loaded| loaded.configuration.applies_to);

    let Some(first) = contexts.clone().next() else {
        return Ok(());
    };
    let Some(second) = contexts
        .into_iter()
        .find(|context| context.repositories_leaf() != first.repositories_leaf())
    else {
        return Ok(());
    };

    Err(LoadFailure::TwoTrees {
        source: source.clone(),
        first,
        second,
    })
}

// ADR 0020
fn refuse_an_account_other_than_the_sources_owner(
    source: &ConfigurationName,
    declared: &GitHubAccount,
    owner: &RepositoryOwner,
) -> Result<()> {
    match declared.as_ref() == owner.as_ref() {
        true => Ok(()),
        false => bail!(
            "{source} declares the account {declared}, and it was read from a repository owned by \
             {owner}. A source is read as the account that owns it, so either the declaration \
             names {owner}, or the configuration belongs in a repository {declared} owns."
        ),
    }
}

#[derive(Debug)]
pub struct Refusal(Vec<Unreadable>);

impl Refusal {
    pub fn of(unreadable: Vec<Unreadable>) -> Option<Self> {
        match unreadable.is_empty() {
            true => None,
            false => Some(Self(unreadable)),
        }
    }

    pub fn unreadable(&self) -> &[Unreadable] {
        &self.0
    }
}

impl Display for Refusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self.0.as_slice() {
            [only] => write!(formatter, "{only}"),
            several => {
                write!(
                    formatter,
                    "{} configurations could not be read:",
                    several.len()
                )?;
                several
                    .iter()
                    .try_for_each(|refusal| write!(formatter, "\n  {refusal}"))
            }
        }
    }
}

impl std::error::Error for Refusal {}

impl Display for ConfigurationSource {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigurationSource::LocalDirectory(directory) => {
                write!(formatter, "local:{}", directory.display())
            }
            ConfigurationSource::GitHubRepository {
                repository,
                directory,
            } => write!(formatter, "github:{repository}/{directory}"),
        }
    }
}

impl ConfigurationSource {
    // ADR 0025
    fn files_come_from(&self) -> Result<SourceLocation, LoadFailure> {
        match self {
            ConfigurationSource::GitHubRepository { repository, .. } => {
                Ok(SourceLocation::Repository(repository.clone()))
            }
            ConfigurationSource::LocalDirectory(directory) => checkout_holding(directory)
                .map(SourceLocation::Checkout)
                .ok_or_else(|| LoadFailure::SourceOutsideACheckout(directory.clone())),
        }
    }

    async fn load(&self, github: &GitHubAccess) -> Vec<Result<LoadedConfiguration, Unreadable>> {
        match self {
            ConfigurationSource::LocalDirectory(directory) => Self::load_local(directory),
            ConfigurationSource::GitHubRepository {
                repository,
                directory,
            } => Self::load_from_github(repository, directory, github).await,
        }
    }

    fn load_local(directory: &Path) -> Vec<Result<LoadedConfiguration, Unreadable>> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(failure) => {
                return vec![Err(Unreadable::Malformed(
                    Error::new(failure).context(format!("Could not read {}", directory.display())),
                ))];
            }
        };

        let mut configuration_paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && is_configuration_file(&path.to_string_lossy()))
            .collect();
        configuration_paths.sort();

        configuration_paths
            .into_iter()
            .map(|path| {
                let source = ConfigurationName::from(path.as_path());
                let contents = fs::read_to_string(&path)
                    .with_context(|| format!("Could not read {source}"))
                    .map_err(Unreadable::Malformed)?;
                let reading = parse_configuration(&contents, &source)?;

                let pending = match reading.migrated_from {
                    None => Pending::Nothing,
                    Some(from) => Pending::Rewriting(
                        Migration::of(&path, &reading.configuration, from)
                            .map_err(Unreadable::Malformed)?,
                    ),
                };
                Ok(LoadedConfiguration {
                    name: source,
                    configuration: reading.configuration,
                    pending,
                })
            })
            .collect()
    }

    async fn load_from_github(
        repository: &GitHubRepository,
        directory: &str,
        github: &GitHubAccess,
    ) -> Vec<Result<LoadedConfiguration, Unreadable>> {
        // ADR 0020
        let reading_as = GitHubAccount::from(repository.owner.as_ref());
        let account = match github.account(&reading_as) {
            Ok(account) => account,
            Err(refusal) => return vec![Err(Unreadable::Malformed(refusal))],
        };

        let file_paths =
            match github::list_directory_files(repository, directory, account.client()).await {
                Ok(file_paths) => file_paths,
                Err(refusal) => return vec![Err(Unreadable::Malformed(refusal))],
            };

        let mut loaded: Vec<Result<LoadedConfiguration, Unreadable>> = Vec::new();
        for file_path in file_paths
            .iter()
            .filter(|file_path| is_configuration_file(file_path))
        {
            let source = ConfigurationName::from(format!("{repository}/{file_path}"));
            match github::get_file_contents(repository, file_path, account.client()).await {
                Err(refusal) => loaded.push(Err(Unreadable::Malformed(refusal))),
                Ok(documents) => loaded.extend(documents.into_iter().map(|contents| {
                    let reading = parse_configuration(&contents, &source)?;
                    refuse_an_account_other_than_the_sources_owner(
                        &source,
                        &reading.configuration.github_account,
                        &repository.owner,
                    )?;
                    Ok(LoadedConfiguration {
                        pending: match reading.migrated_from {
                            None => Pending::Nothing,
                            Some(from) => Pending::Announcing(Notice::SourceCannotBeRewritten {
                                source: source.clone(),
                                from,
                            }),
                        },
                        name: source.clone(),
                        configuration: reading.configuration,
                    })
                })),
            }
        }
        loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{BUILD_GENERATION, OLDEST_READABLE_GENERATION};
    use std::{env, fs::File, io::Write};

    #[test]
    fn a_local_source_resolves_its_files_root_by_walking_up_to_a_checkout() {
        let checkout = temporary_checkout("files_root");

        let location = ConfigurationSource::LocalDirectory(checkout.join("config"))
            .files_come_from()
            .unwrap();

        assert_eq!(location, SourceLocation::Checkout(checkout));
    }

    #[test]
    fn a_github_source_resolves_its_files_root_to_the_clone_of_that_repository() {
        let location = ConfigurationSource::GitHubRepository {
            repository: GitHubRepository {
                owner: RepositoryOwner::from("Alice"),
                repository: RepositoryName::from("dotfiles"),
            },
            directory: "config".to_owned(),
        }
        .files_come_from()
        .unwrap();

        assert_eq!(
            location,
            SourceLocation::Repository(GitHubRepository {
                owner: RepositoryOwner::from("Alice"),
                repository: RepositoryName::from("dotfiles"),
            })
        );
    }

    #[test]
    fn a_configuration_declaring_the_account_that_owns_its_source_is_read() {
        let checked = refuse_an_account_other_than_the_sources_owner(
            &ConfigurationName::from("Alice/dotfiles/config/everywhere.dotconfig.json"),
            &GitHubAccount::from("Alice"),
            &RepositoryOwner::from("Alice"),
        );

        assert!(checked.is_ok(), "{:?}", checked.unwrap_err());
    }

    #[test]
    fn a_configuration_declaring_an_account_other_than_its_sources_owner_names_both_in_the_refusal()
    {
        let refusal = refuse_an_account_other_than_the_sources_owner(
            &ConfigurationName::from("Employer/dotfiles/config/work.dotconfig.json"),
            &GitHubAccount::from("Alice"),
            &RepositoryOwner::from("Employer"),
        )
        .unwrap_err()
        .to_string();

        assert!(
            refusal.contains("Employer/dotfiles/config/work.dotconfig.json")
                && refusal.contains("Alice")
                && refusal.contains("Employer"),
            "{refusal}"
        );
    }

    #[test]
    fn an_account_matching_its_sources_owner_in_every_letter_but_case_is_refused() {
        let checked = refuse_an_account_other_than_the_sources_owner(
            &ConfigurationName::from("Alice/dotfiles/config/everywhere.dotconfig.json"),
            &GitHubAccount::from("alice"),
            &RepositoryOwner::from("Alice"),
        );

        assert!(checked.is_err());
    }

    fn refusing(unreadable: Vec<Unreadable>) -> LoadFailure {
        LoadFailure::Unreadable(
            Refusal::of(unreadable).expect("at least one configuration could not be read"),
        )
    }

    #[test]
    fn a_configuration_needing_a_newer_build_sends_this_one_looking_for_its_own_release() {
        let refusal = refusing(vec![Unreadable::TooNew {
            source: ConfigurationName::from("everywhere.dotconfig.json"),
            required: BUILD_GENERATION.stepped_by(1),
            available: BUILD_GENERATION,
        }]);

        assert!(refusal.is_answered_by_a_newer_build());
    }

    #[test]
    fn a_configuration_a_person_must_repair_is_not_answered_by_updating_this_build() {
        let refusal = refusing(vec![Unreadable::Malformed(anyhow!(
            "everywhere.dotconfig.json is not valid JSON"
        ))]);

        assert!(!refusal.is_answered_by_a_newer_build());
    }

    #[test]
    fn a_document_this_build_has_outgrown_is_not_answered_by_updating_this_build() {
        let refusal = refusing(vec![Unreadable::TooOld {
            source: ConfigurationName::from("everywhere.dotconfig.json"),
            stated: OLDEST_READABLE_GENERATION.stepped_by(-1),
            oldest_readable: OLDEST_READABLE_GENERATION,
        }]);

        assert!(!refusal.is_answered_by_a_newer_build());
    }

    #[test]
    fn one_configuration_needing_a_newer_build_is_enough_to_go_looking_for_one() {
        let refusal = refusing(vec![
            Unreadable::Malformed(anyhow!("personal.dotconfig.json is not valid JSON")),
            Unreadable::TooNew {
                source: ConfigurationName::from("everywhere.dotconfig.json"),
                required: BUILD_GENERATION.stepped_by(1),
                available: BUILD_GENERATION,
            },
        ]);

        assert!(refusal.is_answered_by_a_newer_build());
    }

    #[test]
    fn a_failure_that_is_not_a_refusal_to_read_sends_this_build_looking_for_nothing() {
        let failure = LoadFailure::NoConfigurationFound(vec![ConfigurationSource::LocalDirectory(
            PathBuf::from("/config"),
        )]);

        assert!(!failure.is_answered_by_a_newer_build());
    }
    #[test]
    fn the_cause_of_each_refusal_survives_being_combined_with_the_others() {
        let refusal = Refusal::of(vec![
            Unreadable::Malformed(anyhow!("personal.dotconfig.json is not valid JSON")),
            Unreadable::TooNew {
                source: ConfigurationName::from("everywhere.dotconfig.json"),
                required: BUILD_GENERATION.stepped_by(1),
                available: BUILD_GENERATION,
            },
        ])
        .unwrap();

        let [_, needing_a_newer_build] = refusal.unreadable() else {
            panic!("expected both refusals to be kept");
        };

        let Unreadable::TooNew { required, .. } = needing_a_newer_build else {
            panic!("expected the build to still be named as the fault");
        };
        assert_eq!(*required, BUILD_GENERATION.stepped_by(1));
    }

    fn temporary_checkout(name: &str) -> PathBuf {
        let checkout = env::temp_dir()
            .join("dotfiles_configuration_source_tests")
            .join(name);
        let _ = fs::remove_dir_all(&checkout);
        fs::create_dir_all(checkout.join(".git")).unwrap();
        fs::create_dir_all(checkout.join("config")).unwrap();
        checkout
    }

    fn write_configuration(checkout: &Path, file_name: &str, applies_to: &str, resources: &str) {
        let contents = format!(
            r#"{{
                "version": "{BUILD_GENERATION}",
                "applies_to": "{applies_to}",
                "github_account": "Alice",
                "resources": [{resources}]
            }}"#
        );
        let mut file = File::create(checkout.join("config").join(file_name)).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    fn command(argument: &str) -> String {
        format!(r#"{{ "kind": "command", "shell": "bash", "args": ["{argument}"] }}"#)
    }

    async fn load_from(
        checkout: &Path,
        machine: MachineClass,
    ) -> Result<DesiredState, LoadFailure> {
        load_desired_state(
            &[ConfigurationSource::LocalDirectory(checkout.join("config"))],
            machine,
            Path::new("/repositories"),
            &GitHubAccess::new(),
        )
        .await
    }

    /// Every change set carries the configurator's own currency, which these scenarios are not
    /// about.
    fn rendered(desired_state: &DesiredState) -> Vec<String> {
        let carried = &desired_state.undeclared;
        desired_state
            .resources
            .iter()
            .filter(|resource| {
                resource
                    .identity(Path::new("/home/Alice"))
                    .is_none_or(|identity| !carried.contains(&identity))
            })
            .map(ToString::to_string)
            .collect()
    }

    #[tokio::test]
    async fn a_directory_that_does_not_exist_is_reported_by_path() {
        let error = load_desired_state(
            &[ConfigurationSource::LocalDirectory(
                "/no/such/directory".into(),
            )],
            MachineClass::Personal,
            Path::new("/repositories"),
            &GitHubAccess::new(),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("no/such/directory"), "{error}");
    }

    #[tokio::test]
    async fn a_directory_inside_no_checkout_has_nothing_to_read_a_configurations_files_out_of() {
        let outside_any_checkout = env::temp_dir()
            .join("dotfiles_configuration_source_tests")
            .join("outside_any_checkout");
        let _ = fs::remove_dir_all(&outside_any_checkout);
        fs::create_dir_all(outside_any_checkout.join("config")).unwrap();
        write_configuration(
            &outside_any_checkout,
            "everywhere.dotconfig.json",
            "everywhere",
            "",
        );
        let outside_any_checkout = outside_any_checkout.join("config");

        let error = load_desired_state(
            &[ConfigurationSource::LocalDirectory(outside_any_checkout)],
            MachineClass::Personal,
            Path::new("/repositories"),
            &GitHubAccess::new(),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("no checkout"), "{error}");
    }

    #[tokio::test]
    async fn configurations_in_a_directory_are_read_in_a_stable_order() {
        let checkout = temporary_checkout("stable_order");
        write_configuration(
            &checkout,
            "b_second.dotconfig.json",
            "everywhere",
            &command("second"),
        );
        write_configuration(
            &checkout,
            "a_first.dotconfig.json",
            "personal",
            &command("first"),
        );

        let desired_state = load_from(&checkout, MachineClass::Personal).await.unwrap();

        assert_eq!(
            rendered(&desired_state),
            vec!["command first".to_owned(), "command second".to_owned()]
        );
    }

    #[tokio::test]
    async fn a_directory_holding_a_configuration_for_another_machine_contributes_none_of_it() {
        let checkout = temporary_checkout("another_machine");
        write_configuration(
            &checkout,
            "everywhere.dotconfig.json",
            "everywhere",
            &command("for every machine"),
        );
        write_configuration(
            &checkout,
            "personal.dotconfig.json",
            "personal",
            &command("for a personal machine"),
        );

        let desired_state = load_from(&checkout, MachineClass::Personal).await.unwrap();

        assert!(
            !rendered(&desired_state).contains(&"command for a work machine".to_owned()),
            "{:?}",
            rendered(&desired_state)
        );
    }

    #[tokio::test]
    async fn a_file_without_the_configuration_suffix_is_not_read_at_all() {
        let checkout = temporary_checkout("unsuffixed_file");
        write_configuration(
            &checkout,
            "everywhere.dotconfig.json",
            "everywhere",
            &command("declared"),
        );
        write_configuration(
            &checkout,
            "personal.dotconfig.json",
            "personal",
            &command("also declared"),
        );
        let mut readme = File::create(checkout.join("config").join("README.md")).unwrap();
        readme.write_all(b"These are the configurations.").unwrap();

        let desired_state = load_from(&checkout, MachineClass::Personal).await.unwrap();

        assert_eq!(
            rendered(&desired_state),
            vec![
                "command declared".to_owned(),
                "command also declared".to_owned()
            ]
        );
    }

    #[tokio::test]
    async fn a_set_holding_nothing_for_this_machines_class_is_refused() {
        let checkout = temporary_checkout("nothing_for_this_class");
        write_configuration(
            &checkout,
            "everywhere.dotconfig.json",
            "everywhere",
            &command("for every machine"),
        );

        let error = load_from(&checkout, MachineClass::Personal)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("machine's class"), "{error}");
    }

    #[tokio::test]
    async fn a_set_holding_nothing_for_every_machine_is_refused() {
        let checkout = temporary_checkout("nothing_for_every_machine");
        write_configuration(
            &checkout,
            "personal.dotconfig.json",
            "personal",
            &command("for a personal machine"),
        );

        let error = load_from(&checkout, MachineClass::Personal)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("every machine"), "{error}");
    }

    #[tokio::test]
    async fn a_source_holding_configurations_of_two_trees_is_refused_by_the_trees_it_names() {
        let checkout = temporary_checkout("two_trees");
        write_configuration(&checkout, "everywhere.dotconfig.json", "everywhere", "");
        write_configuration(&checkout, "work.dotconfig.json", "work", "");

        let error = load_from(&checkout, MachineClass::Work).await.unwrap_err();

        assert!(
            error.to_string().contains("Personal") && error.to_string().contains("Work"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_configuration_in_the_superseded_format_is_rejected_by_file_name() {
        let checkout = temporary_checkout("superseded_format");
        let mut file =
            File::create(checkout.join("config").join("everywhere.dotconfig.json")).unwrap();
        file.write_all(br#"{ "version": "0.1.0", "clone_config": {}, "items": [] }"#)
            .unwrap();

        let error = load_from(&checkout, MachineClass::Personal)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("everywhere.dotconfig.json"));
    }
}

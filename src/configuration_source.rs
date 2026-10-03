use {
    crate::{
        configuration::{
            Configuration, ConfigurationName, Context, EstateConflict, GitHubAccount,
            GitHubRepository, MachineClass, MachineManifest, Migration, Notice, RecordedRun,
            RecordedSource, RenderedManifest, RepositoryName, RepositoryOwner, Unreadable,
            parse_configuration, resolve_estates,
        },
        desired_state::{DesiredState, Irreconcilable, ResolvedConfiguration, SourceLocation},
        github::{self, GitHubAccess},
    },
    anyhow::{Result, anyhow},
    github_authentication::cli,
    std::{
        fmt::{Display, Formatter},
        fs, io,
        path::{Component, Path, PathBuf},
    },
};

pub trait WriteSource {
    fn rewrite(&self, migration: &Migration) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurationSource {
    LocalDirectory(AbsoluteDirectory),
    GitHubRepository {
        repository: GitHubRepository,
        directory: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct AbsoluteDirectory(PathBuf);

impl AbsoluteDirectory {
    pub fn of(path: PathBuf) -> Option<Self> {
        match path.is_absolute() {
            true => Some(Self(lexically_normalised(&path))),
            false => None,
        }
    }

    pub fn resolve(&self, path: &Path) -> Option<Self> {
        Self::of(self.0.join(path))
    }

    fn enclosing_checkout(&self) -> Option<Self> {
        self.0
            .ancestors()
            .find(|ancestor| ancestor.join(".git").exists())
            .map(|checkout| Self(checkout.to_path_buf()))
    }
}

fn lexically_normalised(path: &Path) -> PathBuf {
    let mut normalised = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalised.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalised.push(component)
            }
        }
    }
    normalised
}

impl AsRef<Path> for AbsoluteDirectory {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl ConfigurationSource {
    pub fn named(value: &str, working_directory: &AbsoluteDirectory) -> Result<Self, String> {
        Self::parsed(value, |directory| {
            working_directory
                .resolve(Path::new(directory))
                .ok_or_else(|| {
                    format!(
                        "{value:?} names a directory relative to a drive rather than to {}; \
                         name it in full",
                        working_directory.as_ref().display()
                    )
                })
        })
    }

    pub fn of_recorded(recorded: &RecordedSource) -> Result<Self, String> {
        let value = recorded.as_written();
        Self::parsed(value, |directory| {
            AbsoluteDirectory::of(PathBuf::from(directory))
                .ok_or_else(|| format!("{value:?} was recorded without its directory in full"))
        })
    }

    fn parsed(
        value: &str,
        local_directory: impl FnOnce(&str) -> Result<AbsoluteDirectory, String>,
    ) -> Result<Self, String> {
        let (kind, rest) = value
            .split_once(':')
            .ok_or_else(|| format!("{value:?} names no source kind; {EXPECTED_SOURCE}"))?;

        match kind {
            "local" => local_directory(rest).map(ConfigurationSource::LocalDirectory),
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

pub const DEFAULT_SOURCE: &str = "github:RobinCombrink/dotfiles/config";

const EXPECTED_SOURCE: &str = "expected `local:<directory>` or `github:<owner>/<repo>/<directory>`";

const CONFIGURATION_SUFFIX: &str = ".dotconfig.json";

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
    let mut failures: Vec<SourceFailure> = Vec::new();
    for source in sources {
        let mut from_this_source: Vec<LoadedConfiguration> = Vec::new();
        for attempt in source.load(github).await {
            match attempt {
                Ok(configuration) => from_this_source.push(configuration),
                Err(failure) => failures.push(failure),
            }
        }
        per_source.push((source, from_this_source));
    }

    if let Some(refusal) = refusal_of(failures) {
        return Err(refusal);
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

    let estates = resolve_estates(
        applicable
            .iter()
            .map(|(loaded, _)| (&loaded.name, &loaded.configuration)),
    )
    .map_err(LoadFailure::Estates)?;

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

    let machine_manifest = RenderedManifest::try_from(MachineManifest {
        repositories_directory_path: repositories_root.join(machine.repositories_leaf()),
        estates,
        recorded_run: RecordedRun {
            class: machine,
            configuration_sources: sources.iter().map(ConfigurationSource::recorded).collect(),
        },
    })
    .map_err(|failure| {
        LoadFailure::Irreconcilable(Irreconcilable::UnrenderableManifest(failure))
    })?;

    let home_directory = std::env::home_dir().ok_or_else(|| LoadFailure::Environment {
        failures: EnvironmentFailures::one(EnvironmentFailure::HomeDirectoryUnknown),
        unreadable: None,
    })?;
    DesiredState::of(resolved, machine_manifest, &home_directory)
        .map(|desired_state| desired_state.also_reporting(migrations, announcements))
        .map_err(LoadFailure::Irreconcilable)
}

#[derive(Debug)]
pub enum EnvironmentFailure {
    GitHubCli(cli::Refusal),
    GitHubClient(octocrab::Error),
    GitHubFetch {
        path: String,
        failure: octocrab::Error,
    },
    LocalRead {
        path: PathBuf,
        failure: io::Error,
    },
    HomeDirectoryUnknown,
}

impl Display for EnvironmentFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvironmentFailure::GitHubCli(refusal @ cli::Refusal::ToolAbsent) => write!(
                formatter,
                "{refusal}. Install it from https://cli.github.com, then authenticate with `gh \
                 auth login`"
            ),
            EnvironmentFailure::GitHubCli(refusal @ cli::Refusal::AccountUnheld { account }) => {
                write!(
                    formatter,
                    "{refusal}. Run `gh auth login` and sign in as {account}"
                )
            }
            EnvironmentFailure::GitHubCli(refusal @ cli::Refusal::Failed { .. }) => {
                write!(formatter, "{refusal}")
            }
            EnvironmentFailure::GitHubClient(failure) => {
                write!(formatter, "Could not prepare a client for GitHub")?;
                write_causes(formatter, failure)
            }
            EnvironmentFailure::GitHubFetch { path, failure } => {
                write!(formatter, "Could not read {path}")?;
                write_causes(formatter, failure)
            }
            EnvironmentFailure::LocalRead { path, failure } => {
                write!(formatter, "Could not read {}", path.display())?;
                write_causes(formatter, failure)
            }
            EnvironmentFailure::HomeDirectoryUnknown => {
                formatter.write_str("Could not find the home directory to resolve symlinks against")
            }
        }
    }
}

fn write_causes(
    formatter: &mut Formatter<'_>,
    failure: &(dyn std::error::Error + 'static),
) -> std::fmt::Result {
    let mut cause = Some(failure);
    while let Some(current) = cause {
        write!(formatter, ": {current}")?;
        cause = current.source();
    }
    Ok(())
}

impl std::error::Error for EnvironmentFailure {}

#[derive(Debug)]
pub struct EnvironmentFailures(Vec<EnvironmentFailure>);

impl EnvironmentFailures {
    fn of(failures: Vec<EnvironmentFailure>) -> Option<Self> {
        match failures.is_empty() {
            true => None,
            false => Some(Self(failures)),
        }
    }

    fn one(failure: EnvironmentFailure) -> Self {
        Self(vec![failure])
    }
}

impl Display for EnvironmentFailures {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let mut failures = self.0.iter();
        if let Some(first) = failures.next() {
            write!(formatter, "{first}")?;
        }
        failures.try_for_each(|failure| write!(formatter, "\n{failure}"))
    }
}

#[derive(Debug)]
enum SourceFailure {
    Environment(EnvironmentFailure),
    Unreadable(Unreadable),
}

impl From<EnvironmentFailure> for SourceFailure {
    fn from(failure: EnvironmentFailure) -> Self {
        Self::Environment(failure)
    }
}

impl From<Unreadable> for SourceFailure {
    fn from(unreadable: Unreadable) -> Self {
        Self::Unreadable(unreadable)
    }
}

fn refusal_of(failures: Vec<SourceFailure>) -> Option<LoadFailure> {
    let mut environment: Vec<EnvironmentFailure> = Vec::new();
    let mut unreadable: Vec<Unreadable> = Vec::new();
    for failure in failures {
        match failure {
            SourceFailure::Environment(failure) => environment.push(failure),
            SourceFailure::Unreadable(refusal) => unreadable.push(refusal),
        }
    }

    match (
        EnvironmentFailures::of(environment),
        Refusal::of(unreadable),
    ) {
        (None, None) => None,
        (None, Some(refusal)) => Some(LoadFailure::Unreadable(refusal)),
        (Some(failures), unreadable) => Some(LoadFailure::Environment {
            failures,
            unreadable,
        }),
    }
}

#[derive(Debug)]
pub enum LoadFailure {
    Unreadable(Refusal),
    Environment {
        failures: EnvironmentFailures,
        unreadable: Option<Refusal>,
    },
    NoConfigurationFound(Vec<ConfigurationSource>),
    NoneAppliesTo(MachineClass),
    // ADR 0025
    TwoTrees {
        source: ConfigurationSource,
        first: Context,
        second: Context,
    },
    SourceOutsideACheckout(PathBuf),
    Estates(EstateConflict),
    Irreconcilable(Irreconcilable),
}

impl LoadFailure {
    pub fn is_answered_by_a_newer_build(&self) -> bool {
        self.unreadable().iter().any(Unreadable::is_too_new)
    }

    pub fn unreadable(&self) -> &[Unreadable] {
        match self {
            LoadFailure::Unreadable(refusal)
            | LoadFailure::Environment {
                unreadable: Some(refusal),
                ..
            } => refusal.unreadable(),
            LoadFailure::Environment {
                unreadable: None, ..
            }
            | LoadFailure::NoConfigurationFound(_)
            | LoadFailure::NoneAppliesTo(_)
            | LoadFailure::TwoTrees { .. }
            | LoadFailure::SourceOutsideACheckout(_)
            | LoadFailure::Estates(_)
            | LoadFailure::Irreconcilable(_) => &[],
        }
    }
}

impl Display for LoadFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadFailure::Unreadable(refusal) => Display::fmt(refusal, formatter),
            LoadFailure::Environment {
                failures,
                unreadable,
            } => {
                write!(formatter, "{failures}")?;
                match unreadable {
                    Some(refusal) => write!(formatter, "\n{refusal}"),
                    None => Ok(()),
                }
            }
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
                "{source} holds a configuration for {first}, which clones under {}, and one for \
                 {second}, which clones under {}. One source cannot be cloned into two trees.",
                first.repositories_leaf(),
                second.repositories_leaf()
            ),
            LoadFailure::SourceOutsideACheckout(directory) => write!(
                formatter,
                "{} is inside no checkout, so there is nothing to read a configuration's files \
                 out of. Read it from the repository it was written in instead.",
                directory.display()
            ),
            LoadFailure::Estates(conflict) => Display::fmt(conflict, formatter),
            LoadFailure::Irreconcilable(reason) => Display::fmt(reason, formatter),
        }
    }
}

impl std::error::Error for LoadFailure {}

// ADR 0025
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
) -> Result<(), Unreadable> {
    match declared.as_ref() == owner.as_ref() {
        true => Ok(()),
        false => Err(Unreadable::Malformed(anyhow!(
            "{source} declares the account {declared}, and it was read from a repository owned by \
             {owner}. A source is read as the account that owns it, so either the declaration \
             names {owner}, or the configuration belongs in a repository {declared} owns."
        ))),
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
                write!(formatter, "local:{}", directory.as_ref().display())
            }
            ConfigurationSource::GitHubRepository {
                repository,
                directory,
            } => write!(formatter, "github:{repository}/{directory}"),
        }
    }
}

impl ConfigurationSource {
    pub fn recorded(&self) -> RecordedSource {
        RecordedSource::from(self.to_string())
    }
}

impl ConfigurationSource {
    // ADR 0025
    fn files_come_from(&self) -> Result<SourceLocation, LoadFailure> {
        match self {
            ConfigurationSource::GitHubRepository { repository, .. } => {
                Ok(SourceLocation::Repository(repository.clone()))
            }
            ConfigurationSource::LocalDirectory(directory) => directory
                .enclosing_checkout()
                .map(SourceLocation::Checkout)
                .ok_or_else(|| LoadFailure::SourceOutsideACheckout(directory.as_ref().into())),
        }
    }

    async fn load(&self, github: &GitHubAccess) -> Vec<Result<LoadedConfiguration, SourceFailure>> {
        match self {
            ConfigurationSource::LocalDirectory(directory) => Self::load_local(directory.as_ref()),
            ConfigurationSource::GitHubRepository {
                repository,
                directory,
            } => Self::load_from_github(repository, directory, github).await,
        }
    }

    fn load_local(directory: &Path) -> Vec<Result<LoadedConfiguration, SourceFailure>> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(failure) => {
                return vec![Err(SourceFailure::Environment(
                    EnvironmentFailure::LocalRead {
                        path: directory.to_path_buf(),
                        failure,
                    },
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
                let contents =
                    fs::read_to_string(&path).map_err(|failure| EnvironmentFailure::LocalRead {
                        path: path.clone(),
                        failure,
                    })?;
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
    ) -> Vec<Result<LoadedConfiguration, SourceFailure>> {
        // ADR 0020
        let reading_as = GitHubAccount::from(repository.owner.as_ref());
        let account = match github.account(&reading_as) {
            Ok(account) => account,
            Err(failure) => return vec![Err(SourceFailure::Environment(failure))],
        };

        let file_paths =
            match github::list_directory_files(repository, directory, account.client()).await {
                Ok(file_paths) => file_paths,
                Err(failure) => return vec![Err(SourceFailure::Environment(failure))],
            };

        let mut loaded: Vec<Result<LoadedConfiguration, SourceFailure>> = Vec::new();
        for file_path in file_paths
            .iter()
            .filter(|file_path| is_configuration_file(file_path))
        {
            let source = ConfigurationName::from(format!("{repository}/{file_path}"));
            match github::get_file_contents(repository, file_path, account.client()).await {
                Err(failure) => loaded.push(Err(SourceFailure::Environment(failure))),
                Ok(documents) => loaded.extend(documents.into_iter().map(|decoded| {
                    let contents = decoded.map_err(Unreadable::Malformed)?;
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
    use std::{env, fs::File, io::Write, process};

    #[test]
    fn a_local_source_resolves_its_files_root_by_walking_up_to_a_checkout() {
        let checkout = temporary_checkout("files_root");

        let location = ConfigurationSource::LocalDirectory(absolute(checkout.join("config")))
            .files_come_from()
            .unwrap();

        assert_eq!(location, SourceLocation::Checkout(absolute(checkout)));
    }

    #[test]
    fn a_local_source_named_up_through_a_checkout_inside_another_resolves_to_the_outer_one() {
        let outer = temporary_checkout("named_up_through_a_checkout");
        let inner = outer.join("tools").join("inner");
        fs::create_dir_all(inner.join(".git")).unwrap();
        fs::create_dir_all(outer.join("tools").join("config")).unwrap();

        let location = ConfigurationSource::named("local:../config", &absolute(inner))
            .unwrap()
            .files_come_from()
            .unwrap();

        assert_eq!(location, SourceLocation::Checkout(absolute(outer)));
    }

    #[test]
    fn a_local_source_is_read_from_the_directory_its_parent_and_current_steps_lead_to() {
        let source = ConfigurationSource::named(
            "local:./../beside/./config",
            &absolute(where_alice_runs().join("checkout")),
        )
        .unwrap();

        assert_eq!(
            directory_read(source),
            where_alice_runs().join("beside").join("config")
        );
    }

    #[test]
    fn a_local_source_stepping_above_the_root_stays_at_the_root() {
        let root = where_alice_runs()
            .ancestors()
            .last()
            .expect("an absolute path has a root")
            .to_path_buf();
        let steps_above_the_root = where_alice_runs().components().count() + 1;

        let source = ConfigurationSource::named(
            &format!("local:{}config", "../".repeat(steps_above_the_root)),
            &absolute(where_alice_runs()),
        )
        .unwrap();

        assert_eq!(directory_read(source), root.join("config"));
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

    fn absolute(path: PathBuf) -> AbsoluteDirectory {
        AbsoluteDirectory::of(path).expect("a temporary directory is absolute")
    }

    fn where_alice_runs() -> PathBuf {
        env::temp_dir().join("where_alice_runs")
    }

    fn directory_read(source: ConfigurationSource) -> PathBuf {
        let ConfigurationSource::LocalDirectory(directory) = source else {
            panic!("expected a local directory, got {source}");
        };
        directory.as_ref().to_path_buf()
    }

    #[test]
    fn a_local_source_named_relative_to_the_working_directory_is_read_from_under_it() {
        let source =
            ConfigurationSource::named("local:config", &absolute(where_alice_runs())).unwrap();

        assert_eq!(directory_read(source), where_alice_runs().join("config"));
    }

    #[test]
    fn a_local_source_named_in_full_is_read_where_it_names_whatever_the_working_directory() {
        let checkout = env::temp_dir().join("alices_checkout").join("config");

        let source = ConfigurationSource::named(
            &format!("local:{}", checkout.display()),
            &absolute(where_alice_runs()),
        )
        .unwrap();

        assert_eq!(directory_read(source), checkout);
    }

    #[cfg(windows)]
    #[test]
    fn a_local_source_named_relative_to_a_drive_is_refused() {
        let refusal = ConfigurationSource::named("local:D:config", &absolute(where_alice_runs()))
            .unwrap_err();

        assert!(refusal.contains("name it in full"), "{refusal}");
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

    async fn refused_as_unauthorised() -> octocrab::Error {
        use http_body_util::BodyExt;

        let body = http_body_util::Full::new(bytes::Bytes::from_static(
            br#"{ "message": "Bad credentials" }"#,
        ))
        .map_err(|never| match never {})
        .boxed();
        let response = http::Response::builder()
            .status(http::StatusCode::UNAUTHORIZED)
            .body(body)
            .expect("a response GitHub could have sent");

        octocrab::map_github_error(response)
            .await
            .expect_err("a 401 is a failure")
    }

    fn refusal_naming(failure: EnvironmentFailure) -> LoadFailure {
        refusal_of(vec![SourceFailure::Environment(failure)])
            .expect("a failure to read a source refuses the load")
    }

    #[tokio::test]
    async fn a_source_github_refuses_to_authenticate_is_a_failure_of_the_environment() {
        let refusal = refusal_naming(EnvironmentFailure::GitHubFetch {
            path: "Alice/dotfiles/config".to_owned(),
            failure: refused_as_unauthorised().await,
        });

        let LoadFailure::Environment { .. } = refusal else {
            panic!("expected a failure of the environment, got {refusal:?}");
        };
    }

    #[tokio::test]
    async fn a_source_github_refuses_to_authenticate_counts_as_no_unreadable_configuration() {
        let refusal = refusal_naming(EnvironmentFailure::GitHubFetch {
            path: "Alice/dotfiles/config".to_owned(),
            failure: refused_as_unauthorised().await,
        });

        assert!(refusal.unreadable().is_empty(), "{refusal}");
    }

    #[test]
    fn an_account_the_github_cli_does_not_hold_is_a_failure_of_the_environment() {
        let refusal = refusal_naming(EnvironmentFailure::GitHubCli(cli::Refusal::AccountUnheld {
            account: "Alice".to_owned(),
        }));

        let LoadFailure::Environment { .. } = refusal else {
            panic!("expected a failure of the environment, got {refusal:?}");
        };
    }

    #[test]
    fn an_account_the_github_cli_does_not_hold_counts_as_no_unreadable_configuration() {
        let refusal = refusal_naming(EnvironmentFailure::GitHubCli(cli::Refusal::AccountUnheld {
            account: "Alice".to_owned(),
        }));

        assert!(refusal.unreadable().is_empty(), "{refusal}");
    }

    #[test]
    fn a_configuration_whose_content_cannot_be_read_is_unreadable_rather_than_the_environments() {
        let refusal = refusal_of(vec![SourceFailure::Unreadable(Unreadable::Malformed(
            anyhow!("personal.dotconfig.json is not valid JSON"),
        ))])
        .expect("an unreadable configuration refuses the load");

        let LoadFailure::Unreadable(_) = refusal else {
            panic!("expected an unreadable configuration, got {refusal:?}");
        };
    }

    #[tokio::test]
    async fn a_run_failing_in_the_environment_and_on_content_reports_each_source_with_its_cause() {
        let refusal = refusal_of(vec![
            SourceFailure::Environment(EnvironmentFailure::GitHubFetch {
                path: "Employer/dotfiles/config".to_owned(),
                failure: refused_as_unauthorised().await,
            }),
            SourceFailure::Unreadable(Unreadable::Malformed(anyhow!(
                "personal.dotconfig.json is not valid JSON"
            ))),
        ])
        .expect("both failures refuse the load")
        .to_string();

        assert!(
            refusal.contains("Employer/dotfiles/config: GitHub: Bad credentials")
                && refusal.contains("personal.dotconfig.json is not valid JSON"),
            "{refusal}"
        );
    }

    #[tokio::test]
    async fn a_run_counts_only_the_configurations_whose_content_could_not_be_read() {
        let refusal = refusal_of(vec![
            SourceFailure::Environment(EnvironmentFailure::GitHubFetch {
                path: "Employer/dotfiles/config".to_owned(),
                failure: refused_as_unauthorised().await,
            }),
            SourceFailure::Unreadable(Unreadable::Malformed(anyhow!(
                "personal.dotconfig.json is not valid JSON"
            ))),
            SourceFailure::Unreadable(Unreadable::Malformed(anyhow!(
                "work.dotconfig.json is not valid JSON"
            ))),
        ])
        .expect("every failure refuses the load")
        .to_string();

        assert!(
            refusal.contains("2 configurations could not be read"),
            "{refusal}"
        );
    }

    #[test]
    fn an_absent_github_cli_is_answered_with_how_to_install_it() {
        let reported = EnvironmentFailure::GitHubCli(cli::Refusal::ToolAbsent).to_string();

        assert!(
            reported.ends_with(
                "Install it from https://cli.github.com, then authenticate with `gh auth login`"
            ),
            "{reported}"
        );
    }

    #[test]
    fn an_account_the_github_cli_does_not_hold_is_answered_with_how_to_sign_in_as_it() {
        let reported = EnvironmentFailure::GitHubCli(cli::Refusal::AccountUnheld {
            account: "Alice".to_owned(),
        })
        .to_string();

        assert!(
            reported.ends_with("Run `gh auth login` and sign in as Alice"),
            "{reported}"
        );
    }

    #[test]
    fn a_github_cli_failure_with_no_act_behind_it_is_answered_with_its_reason_alone() {
        let reported = EnvironmentFailure::GitHubCli(cli::Refusal::Failed {
            account: "Alice".to_owned(),
            reason: "the token it wrote is not valid UTF-8".to_owned(),
        })
        .to_string();

        assert!(
            reported.ends_with("the token it wrote is not valid UTF-8"),
            "{reported}"
        );
    }

    #[tokio::test]
    async fn a_directory_that_does_not_exist_is_a_failure_of_the_environment() {
        let error = load_desired_state(
            &[ConfigurationSource::LocalDirectory(absolute(
                env::temp_dir().join("no").join("such").join("directory"),
            ))],
            MachineClass::Personal,
            Path::new("/repositories"),
            &GitHubAccess::new(),
        )
        .await
        .unwrap_err();

        let LoadFailure::Environment { .. } = error else {
            panic!("expected a failure of the environment, got {error:?}");
        };
    }

    #[test]
    fn a_failure_that_is_not_a_refusal_to_read_sends_this_build_looking_for_nothing() {
        let failure = LoadFailure::NoConfigurationFound(vec![ConfigurationSource::LocalDirectory(
            absolute(env::temp_dir().join("config")),
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
            .join(process::id().to_string())
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
            &[ConfigurationSource::LocalDirectory(absolute(
                checkout.join("config"),
            ))],
            machine,
            Path::new("/repositories"),
            &GitHubAccess::new(),
        )
        .await
    }

    fn rendered_other_than_what_every_change_set_carries(
        desired_state: &DesiredState,
    ) -> Vec<String> {
        desired_state
            .other_than_what_every_change_set_carries()
            .into_iter()
            .map(ToString::to_string)
            .collect()
    }

    #[tokio::test]
    async fn a_directory_that_does_not_exist_is_reported_by_path() {
        let missing = Path::new("no").join("such").join("directory");
        let error = load_desired_state(
            &[ConfigurationSource::LocalDirectory(absolute(
                env::temp_dir().join(&missing),
            ))],
            MachineClass::Personal,
            Path::new("/repositories"),
            &GitHubAccess::new(),
        )
        .await
        .unwrap_err();

        assert!(
            error.to_string().contains(&missing.display().to_string()),
            "{error}"
        );
    }

    #[cfg(windows)]
    fn a_directory_not_spelled_in_utf8() -> PathBuf {
        use std::os::windows::ffi::OsStringExt;

        PathBuf::from(std::ffi::OsString::from_wide(&[u16::from(b'/'), 0xD800]))
    }

    #[cfg(unix)]
    fn a_directory_not_spelled_in_utf8() -> PathBuf {
        use std::os::unix::ffi::OsStringExt;

        PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xFF]))
    }

    #[tokio::test]
    async fn a_manifest_that_cannot_be_written_out_is_refused_at_load_naming_why() {
        let checkout = temporary_checkout("unrenderable_manifest");
        write_configuration(&checkout, "everywhere.dotconfig.json", "everywhere", "");
        write_configuration(&checkout, "personal.dotconfig.json", "personal", "");

        let error = load_desired_state(
            &[ConfigurationSource::LocalDirectory(absolute(
                checkout.join("config"),
            ))],
            MachineClass::Personal,
            &a_directory_not_spelled_in_utf8(),
            &GitHubAccess::new(),
        )
        .await
        .unwrap_err();

        let LoadFailure::Irreconcilable(Irreconcilable::UnrenderableManifest(_)) = &error else {
            panic!("expected the manifest to be refused, got {error:?}");
        };
        assert!(error.to_string().contains("UTF-8"), "{error}");
    }

    #[tokio::test]
    async fn a_directory_inside_no_checkout_has_nothing_to_read_a_configurations_files_out_of() {
        let outside_any_checkout = env::temp_dir()
            .join("dotfiles_configuration_source_tests")
            .join(process::id().to_string())
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
            &[ConfigurationSource::LocalDirectory(absolute(
                outside_any_checkout,
            ))],
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
            rendered_other_than_what_every_change_set_carries(&desired_state),
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
            !rendered_other_than_what_every_change_set_carries(&desired_state)
                .contains(&"command for a work machine".to_owned()),
            "{:?}",
            rendered_other_than_what_every_change_set_carries(&desired_state)
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
            rendered_other_than_what_every_change_set_carries(&desired_state),
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

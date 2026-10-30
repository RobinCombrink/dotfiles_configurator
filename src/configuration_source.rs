use {
    crate::{
        configuration::{
            BUILD_GENERATION, Configuration, ConfigurationName, Context, EstateConflict,
            EstateDeclaration, EstateFile, GitCommit, GitHubAccount, GitHubRepository,
            MachineClass, MachineManifest, Migration, Notice, ReadEstate, Reading, RecordedRun,
            RecordedSource, RenderedManifest, RepositoriesDirectory, RepositoryName,
            RepositoryOwner, SlotsPerBankDeclaredTwice, Unreadable, parse_configuration,
            resolve_estates, resolve_slots_per_bank,
        },
        desired_state::{DesiredState, Irreconcilable, ResolvedConfiguration, SourceLocation},
        github::{self, GitHubAccess},
    },
    anyhow::{Result, anyhow},
    github_authentication::cli,
    octocrab::Octocrab,
    std::{
        fmt::{Display, Formatter},
        fs, io,
        path::{Component, Path, PathBuf},
        sync::Arc,
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
struct ParsedConfiguration {
    name: ConfigurationName,
    reading: Reading,
    written: Written,
}

#[derive(Debug)]
enum Written {
    Locally(PathBuf),
    OnGitHub,
}

#[derive(Debug)]
struct LoadedConfiguration {
    name: ConfigurationName,
    configuration: Configuration,
    estate: Option<ReadEstate>,
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
    let mut per_source: Vec<(
        &ConfigurationSource,
        PinnedSource<'_>,
        Vec<ParsedConfiguration>,
    )> = Vec::new();
    let mut failures: Vec<SourceFailure> = Vec::new();
    for source in sources {
        let pinned = match source.pinned(github).await {
            Ok(pinned) => pinned,
            Err(failure) => {
                failures.push(failure);
                continue;
            }
        };
        let mut from_this_source: Vec<ParsedConfiguration> = Vec::new();
        for attempt in pinned.load(github).await {
            match attempt {
                Ok(configuration) => from_this_source.push(configuration),
                Err(failure) => failures.push(failure),
            }
        }
        per_source.push((source, pinned, from_this_source));
    }

    if let Some(refusal) = refusal_of(failures) {
        return Err(refusal);
    }

    let mut read: Vec<(ParsedConfiguration, ReadFrom)> = Vec::new();
    for (source, pinned, from_this_source) in per_source {
        refuse_two_trees_for_one_source(source, &from_this_source)?;
        let read_from = pinned.files_come_from()?;
        read.extend(
            from_this_source
                .into_iter()
                .map(|parsed| (parsed, read_from.clone())),
        );
    }

    if read.is_empty() {
        return Err(LoadFailure::NoConfigurationFound(sources.to_vec()));
    }

    let parsed_applicable: Vec<(ParsedConfiguration, ReadFrom)> = read
        .into_iter()
        .filter(|(parsed, _)| parsed.reading.configuration.applies_to.applies_on(machine))
        .collect();

    if parsed_applicable.is_empty() {
        return Err(LoadFailure::NoneAppliesTo(machine));
    }

    let mut applicable: Vec<(LoadedConfiguration, ReadFrom)> = Vec::new();
    let mut failures: Vec<SourceFailure> = Vec::new();
    for (parsed, read_from) in parsed_applicable {
        match parsed.settled(&read_from, github).await {
            Ok(loaded) => applicable.push((loaded, read_from)),
            Err(failure) => failures.push(failure),
        }
    }

    if let Some(refusal) = refusal_of(failures) {
        return Err(refusal);
    }

    let estates = resolve_estates(
        applicable
            .iter()
            .map(|(loaded, _)| (&loaded.name, &loaded.configuration, loaded.estate.as_ref())),
    )
    .map_err(LoadFailure::Estates)?;

    let slots_per_bank = resolve_slots_per_bank(
        applicable
            .iter()
            .map(|(loaded, _)| (&loaded.name, &loaded.configuration)),
    )
    .map_err(LoadFailure::SlotsPerBank)?;

    let mut migrations: Vec<Migration> = Vec::new();
    let mut announcements: Vec<Notice> = Vec::new();
    let mut resolved: Vec<ResolvedConfiguration> = Vec::new();
    for (loaded, read_from) in applicable {
        match loaded.pending {
            Pending::Nothing => {}
            Pending::Rewriting(migration) => migrations.push(migration),
            Pending::Announcing(notice) => announcements.push(notice),
        }
        resolved.push(ResolvedConfiguration::read(
            loaded.name,
            loaded.configuration,
            read_from.location(),
            repositories_root,
        ));
    }

    let repositories_directory_path = RepositoriesDirectory::try_from(
        repositories_root.join(Context::from(machine).repositories_leaf()),
    )
    .map_err(|refusal| {
        LoadFailure::Irreconcilable(Irreconcilable::RelativeRepositoriesDirectory(refusal))
    })?;

    let machine_manifest = RenderedManifest::try_from(MachineManifest {
        repositories_directory_path,
        estates,
        slots_per_bank,
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
    SlotsPerBank(SlotsPerBankDeclaredTwice),
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
            | LoadFailure::SlotsPerBank(_)
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
            LoadFailure::SlotsPerBank(conflict) => Display::fmt(conflict, formatter),
            LoadFailure::Irreconcilable(reason) => Display::fmt(reason, formatter),
        }
    }
}

impl std::error::Error for LoadFailure {}

// ADR 0025
fn refuse_two_trees_for_one_source(
    source: &ConfigurationSource,
    parsed: &[ParsedConfiguration],
) -> Result<(), LoadFailure> {
    let contexts = parsed
        .iter()
        .map(|parsed| parsed.reading.configuration.applies_to);

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
            } => write!(formatter, "github:{}", repository.path_named(directory)),
        }
    }
}

impl ConfigurationSource {
    pub fn recorded(&self) -> RecordedSource {
        RecordedSource::from(self.to_string())
    }
}

impl ConfigurationSource {
    async fn pinned(&self, github: &GitHubAccess) -> Result<PinnedSource<'_>, SourceFailure> {
        match self {
            ConfigurationSource::LocalDirectory(directory) => {
                Ok(PinnedSource::LocalDirectory(directory))
            }
            ConfigurationSource::GitHubRepository {
                repository,
                directory,
            } => {
                let account = github.account_reading(repository)?;
                let at = github::head_commit(repository, account.client()).await?;
                Ok(PinnedSource::GitHubRepository {
                    repository,
                    directory,
                    at,
                })
            }
        }
    }
}

#[derive(Debug)]
enum PinnedSource<'source> {
    LocalDirectory(&'source AbsoluteDirectory),
    GitHubRepository {
        repository: &'source GitHubRepository,
        directory: &'source str,
        at: GitCommit,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReadFrom {
    Checkout(AbsoluteDirectory),
    Repository {
        repository: GitHubRepository,
        at: GitCommit,
    },
}

impl ReadFrom {
    fn location(self) -> SourceLocation {
        match self {
            ReadFrom::Checkout(checkout) => SourceLocation::Checkout(checkout),
            ReadFrom::Repository { repository, .. } => SourceLocation::Repository(repository),
        }
    }
}

impl PinnedSource<'_> {
    // ADR 0025
    fn files_come_from(&self) -> Result<ReadFrom, LoadFailure> {
        match self {
            PinnedSource::GitHubRepository { repository, at, .. } => Ok(ReadFrom::Repository {
                repository: (*repository).clone(),
                at: at.clone(),
            }),
            PinnedSource::LocalDirectory(directory) => directory
                .enclosing_checkout()
                .map(ReadFrom::Checkout)
                .ok_or_else(|| LoadFailure::SourceOutsideACheckout(directory.as_ref().into())),
        }
    }

    async fn load(&self, github: &GitHubAccess) -> Vec<Result<ParsedConfiguration, SourceFailure>> {
        match self {
            PinnedSource::LocalDirectory(directory) => Self::load_local(directory.as_ref()),
            PinnedSource::GitHubRepository {
                repository,
                directory,
                at,
            } => Self::load_from_github(repository, directory, at, github).await,
        }
    }

    fn load_local(directory: &Path) -> Vec<Result<ParsedConfiguration, SourceFailure>> {
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

                Ok(ParsedConfiguration {
                    name: source,
                    reading,
                    written: Written::Locally(path),
                })
            })
            .collect()
    }

    async fn load_from_github(
        repository: &GitHubRepository,
        directory: &str,
        at: &GitCommit,
        github: &GitHubAccess,
    ) -> Vec<Result<ParsedConfiguration, SourceFailure>> {
        let account = match github.account_reading(repository) {
            Ok(account) => account,
            Err(failure) => return vec![Err(SourceFailure::Environment(failure))],
        };

        let file_paths =
            match github::list_directory_files(repository, directory, at, account.client()).await {
                Ok(file_paths) => file_paths,
                Err(failure) => return vec![Err(SourceFailure::Environment(failure))],
            };

        let mut loaded: Vec<Result<ParsedConfiguration, SourceFailure>> = Vec::new();
        for file_path in file_paths
            .iter()
            .filter(|file_path| is_configuration_file(file_path))
        {
            let source = ConfigurationName::from(repository.path_named(file_path));
            match github::get_file_contents(repository, file_path, at, account.client()).await {
                Err(failure) => loaded.push(Err(SourceFailure::Environment(failure))),
                Ok(documents) => loaded.extend(documents.into_iter().map(|decoded| {
                    let contents = decoded.map_err(Unreadable::Malformed)?;
                    let reading = parse_configuration(&contents, &source)?;
                    refuse_an_account_other_than_the_sources_owner(
                        &source,
                        &reading.configuration.github_account,
                        &repository.owner,
                    )?;
                    Ok(ParsedConfiguration {
                        name: source.clone(),
                        reading,
                        written: Written::OnGitHub,
                    })
                })),
            }
        }
        loaded
    }
}

impl ParsedConfiguration {
    async fn settled(
        self,
        read_from: &ReadFrom,
        github: &GitHubAccess,
    ) -> Result<LoadedConfiguration, SourceFailure> {
        let Self {
            name,
            reading,
            written,
        } = self;
        let estate = match &reading.configuration.estate {
            None => None,
            Some(declaration) => Some(read_estate(declaration, read_from, github).await?),
        };

        let pending = match reading.migration {
            None => Pending::Nothing,
            Some(migration) => {
                let from = migration.completed(estate.as_ref()).map_err(|refused| {
                    Unreadable::Malformed(anyhow::Error::new(refused).context(format!(
                        "{name} cannot be migrated to generation {BUILD_GENERATION}"
                    )))
                })?;
                match written {
                    Written::Locally(path) => Pending::Rewriting(
                        Migration::of(&path, &reading.configuration, from)
                            .map_err(Unreadable::Malformed)?,
                    ),
                    Written::OnGitHub => Pending::Announcing(Notice::SourceCannotBeRewritten {
                        source: name.clone(),
                        from,
                    }),
                }
            }
        };

        Ok(LoadedConfiguration {
            name,
            configuration: reading.configuration,
            estate,
            pending,
        })
    }
}

async fn read_estate(
    declaration: &EstateDeclaration,
    read_from: &ReadFrom,
    github: &GitHubAccess,
) -> Result<ReadEstate, SourceFailure> {
    match read_from {
        ReadFrom::Checkout(checkout) => estate_in_checkout(declaration, checkout),
        ReadFrom::Repository { repository, at } => {
            let account = github.account_reading(repository)?;
            estate_on_github(declaration, repository, at, account.client()).await
        }
    }
}

fn estate_in_checkout(
    declaration: &EstateDeclaration,
    checkout: &AbsoluteDirectory,
) -> Result<ReadEstate, SourceFailure> {
    let path = checkout
        .as_ref()
        .join(declaration.file_within_the_repository().as_path());
    let contents = fs::read_to_string(&path).map_err(|failure| EnvironmentFailure::LocalRead {
        path: path.clone(),
        failure,
    })?;

    decoded_estate(declaration, EstateFile::from(path.as_path()), &contents)
}

async fn estate_on_github(
    declaration: &EstateDeclaration,
    repository: &GitHubRepository,
    at: &GitCommit,
    client: &Arc<Octocrab>,
) -> Result<ReadEstate, SourceFailure> {
    let file_path = declaration.file_within_the_repository().to_string();
    let file = EstateFile::from(repository.path_named(&file_path));
    let documents = github::get_file_contents(repository, &file_path, at, client).await?;

    let [document] = <[_; 1]>::try_from(documents).map_err(|documents| {
        Unreadable::Malformed(anyhow!(
            "{file} names {} entries on GitHub rather than one file",
            documents.len()
        ))
    })?;
    let contents = document.map_err(Unreadable::Malformed)?;

    decoded_estate(declaration, file, &contents)
}

fn decoded_estate(
    declaration: &EstateDeclaration,
    file: EstateFile,
    contents: &str,
) -> Result<ReadEstate, SourceFailure> {
    ReadEstate::decoded(declaration.name.clone(), file, contents)
        .map_err(|refused| Unreadable::Malformed(anyhow::Error::new(refused)).into())
}

#[cfg(test)]
mod tests;

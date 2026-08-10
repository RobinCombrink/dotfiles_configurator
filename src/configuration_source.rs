use {
    crate::{
        configuration::{
            Configuration, GitHubRepository, MachineClass, RepositoryName, RepositoryOwner,
            Unreadable, parse_configuration,
        },
        desired_state::{DesiredState, ResolvedConfiguration, SourceLocation},
        github,
    },
    anyhow::{Context as _, Error, Result, anyhow, bail},
    std::{
        fmt::{Display, Formatter},
        fs,
        path::{Path, PathBuf},
        str::FromStr,
    },
};

/// Where a configuration is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurationSource {
    LocalDirectory(PathBuf),
    GitHubRepository {
        owner: RepositoryOwner,
        repository: RepositoryName,
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
                    owner: RepositoryOwner::from(owner),
                    repository: RepositoryName::from(repository),
                    directory: directory.to_owned(),
                })
            }
            other => Err(format!("{other:?} is not a source kind; {EXPECTED_SOURCE}")),
        }
    }
}

const EXPECTED_SOURCE: &str = "expected `local:<directory>` or `github:<owner>/<repo>/<directory>`";

const CONFIGURATION_SUFFIX: &str = ".dotconfig.json";

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
) -> Result<DesiredState> {
    let mut per_source: Vec<(&ConfigurationSource, Vec<(String, Configuration)>)> = Vec::new();
    let mut unreadable: Vec<Unreadable> = Vec::new();
    for source in sources {
        let mut from_this_source: Vec<(String, Configuration)> = Vec::new();
        for attempt in source.load().await {
            match attempt {
                Ok(configuration) => from_this_source.push(configuration),
                Err(refusal) => unreadable.push(refusal),
            }
        }
        per_source.push((source, from_this_source));
    }

    if let Some(refusal) = Refusal::of(unreadable) {
        return Err(refusal.into());
    }

    let mut read: Vec<(String, Configuration, SourceLocation)> = Vec::new();
    for (source, from_this_source) in per_source {
        refuse_two_trees_for_one_source(source, &from_this_source)?;
        let location = source.files_come_from()?;
        read.extend(
            from_this_source
                .into_iter()
                .map(|(name, configuration)| (name, configuration, location.clone())),
        );
    }

    if read.is_empty() {
        return Err(anyhow!(
            "No configurations were found in any of the sources given"
        ));
    }

    let applicable: Vec<(String, ResolvedConfiguration)> = read
        .into_iter()
        .filter(|(_, configuration, _)| configuration.applies_to.applies_on(machine))
        .map(|(name, configuration, location)| {
            (
                name,
                ResolvedConfiguration::read(configuration, location, repositories_root),
            )
        })
        .collect();

    if applicable.is_empty() {
        return Err(anyhow!(
            "No configuration in any of the sources given applies to {}",
            machine.described()
        ));
    }

    DesiredState::of(applicable)
}

/// A source is cloned into the tree its configurations' context names, so one yielding two
/// contexts that name different trees would have to be cloned into both. See ADR 0025.
fn refuse_two_trees_for_one_source(
    source: &ConfigurationSource,
    loaded: &[(String, Configuration)],
) -> Result<()> {
    let contexts = loaded
        .iter()
        .map(|(_, configuration)| configuration.applies_to);

    let Some(first) = contexts.clone().next() else {
        return Ok(());
    };
    let Some(second) = contexts
        .into_iter()
        .find(|context| context.repositories_leaf() != first.repositories_leaf())
    else {
        return Ok(());
    };

    bail!(
        "{source} holds a configuration for {first}, which clones under {}, and one for {second}, \
         which clones under {}. One source cannot be cloned into two trees.",
        first.repositories_leaf(),
        second.repositories_leaf()
    )
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
                owner,
                repository,
                directory,
            } => write!(formatter, "github:{owner}/{repository}/{directory}"),
        }
    }
}

impl ConfigurationSource {
    // ADR 0025
    fn files_come_from(&self) -> Result<SourceLocation> {
        match self {
            ConfigurationSource::GitHubRepository {
                owner, repository, ..
            } => Ok(SourceLocation::Repository(GitHubRepository {
                owner: owner.clone(),
                repository: repository.clone(),
            })),
            ConfigurationSource::LocalDirectory(directory) => checkout_holding(directory)
                .map(SourceLocation::Checkout)
                .ok_or_else(|| {
                    anyhow!(
                        "{} is inside no checkout, so there is nothing to read a configuration's \
                         files out of. Read it from the repository it was written in instead.",
                        directory.display()
                    )
                }),
        }
    }

    async fn load(&self) -> Vec<Result<(String, Configuration), Unreadable>> {
        match self {
            ConfigurationSource::LocalDirectory(directory) => Self::load_local(directory),
            ConfigurationSource::GitHubRepository {
                owner,
                repository,
                directory,
            } => Self::load_from_github(owner.as_ref(), repository.as_ref(), directory).await,
        }
    }

    fn load_local(directory: &Path) -> Vec<Result<(String, Configuration), Unreadable>> {
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
                let source = path.display().to_string();
                let contents = fs::read_to_string(&path)
                    .with_context(|| format!("Could not read {source}"))
                    .map_err(Unreadable::Malformed)?;
                Ok((source.clone(), parse_configuration(&contents, &source)?))
            })
            .collect()
    }

    async fn load_from_github(
        owner: &str,
        repository: &str,
        directory: &str,
    ) -> Vec<Result<(String, Configuration), Unreadable>> {
        let account = match github::AuthenticatedAccount::authenticate_as(&owner.into()) {
            Ok(account) => account,
            Err(refusal) => return vec![Err(Unreadable::Malformed(refusal))],
        };

        let file_paths = match github::list_directory_files(
            owner,
            repository,
            directory,
            account.client(),
        )
        .await
        {
            Ok(file_paths) => file_paths,
            Err(refusal) => return vec![Err(Unreadable::Malformed(refusal))],
        };

        let mut loaded: Vec<Result<(String, Configuration), Unreadable>> = Vec::new();
        for file_path in file_paths
            .iter()
            .filter(|file_path| is_configuration_file(file_path))
        {
            let source = format!("{owner}/{repository}/{file_path}");
            match github::get_file_contents(owner, repository, file_path, account.client()).await {
                Err(refusal) => loaded.push(Err(Unreadable::Malformed(refusal))),
                Ok(documents) => loaded.extend(documents.into_iter().map(|contents| {
                    Ok((source.clone(), parse_configuration(&contents, &source)?))
                })),
            }
        }
        loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{BEYOND_BUILD_GENERATION, BUILD_GENERATION};
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
            owner: RepositoryOwner::from("Alice"),
            repository: RepositoryName::from("dotfiles"),
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
    fn the_cause_of_each_refusal_survives_being_combined_with_the_others() {
        let refusal = Refusal::of(vec![
            Unreadable::Malformed(anyhow!("personal.dotconfig.json is not valid JSON")),
            Unreadable::TooNew {
                source: "everywhere.dotconfig.json".to_owned(),
                required: BEYOND_BUILD_GENERATION,
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
        assert_eq!(*required, BEYOND_BUILD_GENERATION);
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

    async fn load_from(checkout: &Path, machine: MachineClass) -> Result<DesiredState> {
        load_desired_state(
            &[ConfigurationSource::LocalDirectory(checkout.join("config"))],
            machine,
            Path::new("/repositories"),
        )
        .await
    }

    fn rendered(desired_state: &DesiredState) -> Vec<String> {
        desired_state
            .resources
            .iter()
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

use {
    crate::{
        configuration::{
            Configuration, Context, DesiredState, RepositoryName, RepositoryOwner, Unreadable,
            merge_configurations, parse_configuration,
        },
        github,
    },
    anyhow::{Context as _, Error, Result, anyhow},
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

fn is_configuration_file(path: &str) -> bool {
    path.ends_with(CONFIGURATION_SUFFIX)
}

pub async fn load_desired_state(
    sources: &[ConfigurationSource],
    context: Context,
) -> Result<DesiredState> {
    let mut loaded: Vec<(String, Configuration)> = Vec::new();
    let mut unreadable: Vec<Unreadable> = Vec::new();
    for source in sources {
        for attempt in source.load().await {
            match attempt {
                Ok(configuration) => loaded.push(configuration),
                Err(refusal) => unreadable.push(refusal),
            }
        }
    }

    if let Some(refusal) = Refusal::of(unreadable) {
        return Err(refusal.into());
    }

    if loaded.is_empty() {
        return Err(anyhow!(
            "No configurations were found in any of the sources given"
        ));
    }

    let applicable: Vec<(String, Configuration)> = loaded
        .into_iter()
        .filter(|(_, configuration)| configuration.applies_to.applies_on(context))
        .collect();

    if applicable.is_empty() {
        return Err(anyhow!(
            "No configuration in any of the sources given applies to {}",
            context.machine_described()
        ));
    }

    Ok(merge_configurations(applicable)?.with_dotfiles_repository())
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

impl ConfigurationSource {
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

    fn temporary_directory(name: &str) -> PathBuf {
        let directory = env::temp_dir()
            .join("dotfiles_configuration_source_tests")
            .join(name);
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn write_configuration(directory: &Path, file_name: &str, applies_to: &str, resources: &str) {
        let contents = format!(
            r#"{{
                "version": "{BUILD_GENERATION}",
                "applies_to": "{applies_to}",
                "machine": {{
                    "repositories_directory_path": "C:\\Repositories",
                    "github_username": "Alice",
                    "dotfiles_repository": {{ "owner": "Alice", "repository": "dotfiles" }}
                }},
                "resources": [{resources}]
            }}"#
        );
        let mut file = File::create(directory.join(file_name)).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    fn command(argument: &str) -> String {
        format!(r#"{{ "kind": "command", "shell": "bash", "args": ["{argument}"] }}"#)
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
        let source = ConfigurationSource::LocalDirectory("/no/such/directory".into());

        let error = load_desired_state(&[source], Context::Personal)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("no/such/directory"));
    }

    #[tokio::test]
    async fn configurations_in_a_directory_are_read_in_a_stable_order() {
        let directory = temporary_directory("stable_order");
        write_configuration(
            &directory,
            "b_second.dotconfig.json",
            "everywhere",
            &command("second"),
        );
        write_configuration(
            &directory,
            "a_first.dotconfig.json",
            "everywhere",
            &command("first"),
        );

        let desired_state = load_desired_state(
            &[ConfigurationSource::LocalDirectory(directory)],
            Context::Personal,
        )
        .await
        .unwrap();

        assert_eq!(
            rendered(&desired_state),
            vec![
                "repository Alice/dotfiles".to_owned(),
                "command first".to_owned(),
                "command second".to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn a_directory_holding_a_configuration_for_another_machine_contributes_none_of_it() {
        let directory = temporary_directory("another_machine");
        write_configuration(
            &directory,
            "everywhere.dotconfig.json",
            "everywhere",
            &command("for every machine"),
        );
        write_configuration(
            &directory,
            "personal.dotconfig.json",
            "personal",
            &command("for a personal machine"),
        );

        let desired_state = load_desired_state(
            &[ConfigurationSource::LocalDirectory(directory)],
            Context::Work,
        )
        .await
        .unwrap();

        assert_eq!(
            rendered(&desired_state),
            vec![
                "repository Alice/dotfiles".to_owned(),
                "command for every machine".to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn a_file_without_the_configuration_suffix_is_not_read_at_all() {
        let directory = temporary_directory("unsuffixed_file");
        write_configuration(
            &directory,
            "everywhere.dotconfig.json",
            "everywhere",
            &command("declared"),
        );
        let mut readme = File::create(directory.join("README.md")).unwrap();
        readme.write_all(b"These are the configurations.").unwrap();

        let desired_state = load_desired_state(
            &[ConfigurationSource::LocalDirectory(directory)],
            Context::Personal,
        )
        .await
        .unwrap();

        assert_eq!(
            rendered(&desired_state),
            vec![
                "repository Alice/dotfiles".to_owned(),
                "command declared".to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn a_directory_holding_nothing_for_this_machine_is_refused_naming_the_machine() {
        let directory = temporary_directory("nothing_applies");
        write_configuration(
            &directory,
            "work.dotconfig.json",
            "work",
            &command("for a work machine"),
        );

        let error = load_desired_state(
            &[ConfigurationSource::LocalDirectory(directory)],
            Context::Personal,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("personal"), "{error}");
    }

    #[tokio::test]
    async fn a_configuration_in_the_superseded_format_is_rejected_by_file_name() {
        let directory = temporary_directory("superseded_format");
        let mut file = File::create(directory.join("everywhere.dotconfig.json")).unwrap();
        file.write_all(br#"{ "version": "0.1.0", "clone_config": {}, "items": [] }"#)
            .unwrap();

        let error = load_desired_state(
            &[ConfigurationSource::LocalDirectory(directory)],
            Context::Personal,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("everywhere.dotconfig.json"));
    }
}

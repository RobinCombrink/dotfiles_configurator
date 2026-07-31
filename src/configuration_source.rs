use {
    crate::{
        configuration::{Configuration, DesiredState, merge_configurations, parse_configuration},
        github,
    },
    anyhow::{Context, Result, anyhow},
    github_authentication::authentication::{Authentication, GitHubCliAuthentication},
    std::{
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
        owner: String,
        repo: String,
        file_paths: Vec<String>,
    },
}

/// One source is written as one word, so naming a source cannot interact with naming another and
/// there is no combination of flags that means "nowhere".
///
/// `local:<directory>` or `github:<owner>/<repo>/<path>[,<path>...]`
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
                let (Some(owner), Some(repo), Some(file_paths)) =
                    (segments.next(), segments.next(), segments.next())
                else {
                    return Err(format!("{value:?} names no file paths; {EXPECTED_SOURCE}"));
                };

                Ok(ConfigurationSource::GitHubRepository {
                    owner: owner.to_owned(),
                    repo: repo.to_owned(),
                    file_paths: file_paths.split(',').map(str::to_owned).collect(),
                })
            }
            other => Err(format!("{other:?} is not a source kind; {EXPECTED_SOURCE}")),
        }
    }
}

const EXPECTED_SOURCE: &str =
    "expected `local:<directory>` or `github:<owner>/<repo>/<path>[,<path>...]`";

/// Reads every named source and merges what they declare into one desired state.
pub async fn load_desired_state(sources: &[ConfigurationSource]) -> Result<DesiredState> {
    let mut loaded: Vec<(String, Configuration)> = Vec::new();
    for source in sources {
        loaded.extend(source.load().await?);
    }

    if loaded.is_empty() {
        return Err(anyhow!(
            "No configurations were found in any of the sources given"
        ));
    }

    Ok(merge_configurations(loaded)?.with_dotfiles_repository())
}

impl ConfigurationSource {
    async fn load(&self) -> Result<Vec<(String, Configuration)>> {
        match self {
            ConfigurationSource::LocalDirectory(directory) => Self::load_local(directory),
            ConfigurationSource::GitHubRepository {
                owner,
                repo,
                file_paths,
            } => Self::load_from_github(owner, repo, file_paths).await,
        }
    }

    fn load_local(directory: &Path) -> Result<Vec<(String, Configuration)>> {
        let entries = fs::read_dir(directory)
            .with_context(|| format!("Could not read {}", directory.display()))?;

        let mut configuration_paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        configuration_paths.sort();

        configuration_paths
            .into_iter()
            .map(|path| {
                let source = path.display().to_string();
                let contents = fs::read_to_string(&path)
                    .with_context(|| format!("Could not read {source}"))?;
                Ok((source.clone(), parse_configuration(&contents, &source)?))
            })
            .collect()
    }

    async fn load_from_github(
        owner: &str,
        repo: &str,
        file_paths: &[String],
    ) -> Result<Vec<(String, Configuration)>> {
        let authentication = GitHubCliAuthentication::new(owner.to_owned())?;
        let octocrab = github::create_octocrab(authentication.get_token())?;

        let mut loaded = Vec::new();
        for file_path in file_paths {
            let source = format!("{owner}/{repo}/{file_path}");
            for contents in github::get_file_contents(owner, repo, file_path, &octocrab).await? {
                loaded.push((source.clone(), parse_configuration(&contents, &source)?));
            }
        }
        Ok(loaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs::File, io::Write};

    fn temporary_directory(name: &str) -> PathBuf {
        let directory = env::temp_dir()
            .join("dotfiles_configuration_source_tests")
            .join(name);
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn write_configuration(directory: &Path, file_name: &str, resources: &str) {
        let contents = format!(
            r#"{{
                "version": "2",
                "machine": {{
                    "repositories_directory_path": "C:\\Repositories",
                    "github_username": "Alice",
                    "dotfiles_repository": {{ "owner": "Alice", "repo": "dotfiles" }}
                }},
                "resources": [{resources}]
            }}"#
        );
        let mut file = File::create(directory.join(file_name)).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    #[tokio::test]
    async fn a_directory_that_does_not_exist_is_reported_by_path() {
        let source = ConfigurationSource::LocalDirectory("/no/such/directory".into());

        let error = load_desired_state(&[source]).await.unwrap_err();

        assert!(error.to_string().contains("no/such/directory"));
    }

    #[tokio::test]
    async fn configurations_in_a_directory_are_read_in_a_stable_order() {
        let directory = temporary_directory("stable_order");
        write_configuration(
            &directory,
            "b_second.dotconfig.json",
            r#"{ "kind": "command", "shell": "bash", "args": ["second"] }"#,
        );
        write_configuration(
            &directory,
            "a_first.dotconfig.json",
            r#"{ "kind": "command", "shell": "bash", "args": ["first"] }"#,
        );

        let desired_state = load_desired_state(&[ConfigurationSource::LocalDirectory(directory)])
            .await
            .unwrap();

        let rendered: Vec<String> = desired_state
            .resources
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            rendered,
            vec![
                "repository Alice/dotfiles".to_owned(),
                "command first".to_owned(),
                "command second".to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn a_configuration_in_the_superseded_format_is_rejected_by_file_name() {
        let directory = temporary_directory("superseded_format");
        let mut file = File::create(directory.join("everywhere.dotconfig.json")).unwrap();
        file.write_all(br#"{ "version": "0.1.0", "clone_config": {}, "items": [] }"#)
            .unwrap();

        let error = load_desired_state(&[ConfigurationSource::LocalDirectory(directory)])
            .await
            .unwrap_err();

        assert!(error.to_string().contains("everywhere.dotconfig.json"));
    }
}

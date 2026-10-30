use {
    crate::{
        configuration::{GitCommit, GitHubAccount, GitHubRepository},
        configuration_source::EnvironmentFailure,
    },
    anyhow::{Result, anyhow},
    github_authentication::{GitHubToken, cli},
    octocrab::{Octocrab, models::repos::Content},
    serde::Deserialize,
    std::{
        collections::BTreeMap,
        sync::{Arc, Mutex, PoisonError},
    },
};

pub struct AuthenticatedAccount {
    token: GitHubToken,
    client: Arc<Octocrab>,
}

impl AuthenticatedAccount {
    fn authenticate_as(account: &GitHubAccount) -> Result<Self, EnvironmentFailure> {
        let token = cli::token_for(account.as_ref()).map_err(EnvironmentFailure::GitHubCli)?;
        let client = Octocrab::builder()
            .personal_token(token.secret().clone())
            .build()
            .map_err(EnvironmentFailure::GitHubClient)?;

        Ok(Self {
            token,
            client: Arc::new(client),
        })
    }

    pub fn client(&self) -> &Arc<Octocrab> {
        &self.client
    }

    pub fn token(&self) -> &GitHubToken {
        &self.token
    }
}

#[derive(Default)]
pub struct GitHubAccess {
    authenticated_accounts: Mutex<BTreeMap<GitHubAccount, Arc<AuthenticatedAccount>>>,
}

impl GitHubAccess {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn account(
        &self,
        account: &GitHubAccount,
    ) -> Result<Arc<AuthenticatedAccount>, EnvironmentFailure> {
        let mut held = self
            .authenticated_accounts
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        if let Some(authenticated) = held.get(account) {
            return Ok(Arc::clone(authenticated));
        }

        let authenticated = Arc::new(AuthenticatedAccount::authenticate_as(account)?);
        held.insert(account.clone(), Arc::clone(&authenticated));
        Ok(authenticated)
    }

    // ADR 0020
    pub fn account_reading(
        &self,
        repository: &GitHubRepository,
    ) -> Result<Arc<AuthenticatedAccount>, EnvironmentFailure> {
        self.account(&GitHubAccount::from(repository.owner.as_ref()))
    }
}

#[derive(Deserialize)]
struct HeadCommit {
    sha: NamedCommit,
}

#[derive(Deserialize)]
#[serde(try_from = "String")]
struct NamedCommit(GitCommit);

impl TryFrom<String> for NamedCommit {
    type Error = String;

    fn try_from(sha: String) -> Result<Self, Self::Error> {
        GitCommit::try_from(sha.as_str()).map(Self)
    }
}

pub async fn head_commit(
    repository: &GitHubRepository,
    octocrab: &Arc<Octocrab>,
) -> Result<GitCommit, EnvironmentFailure> {
    let route = format!(
        "/repos/{}/{}/commits/HEAD",
        repository.owner, repository.repository
    );
    let head: HeadCommit = octocrab.get(route, None::<&()>).await.map_err(|failure| {
        EnvironmentFailure::GitHubFetch {
            path: format!("the head commit of {repository}"),
            failure,
        }
    })?;

    Ok(head.sha.0)
}

pub async fn get_file_contents(
    repository: &GitHubRepository,
    file_path: &str,
    at: &GitCommit,
    octocrab: &Arc<Octocrab>,
) -> Result<Vec<Result<String>>, EnvironmentFailure> {
    let contents = octocrab
        .repos(repository.owner.as_ref(), repository.repository.as_ref())
        .get_content()
        .path(file_path.to_owned())
        .r#ref(at.as_ref())
        .send()
        .await
        .map_err(|failure| EnvironmentFailure::GitHubFetch {
            path: repository.path_named(file_path),
            failure,
        })?;

    Ok(contents
        .items
        .iter()
        .map(|item| decoded_content_of(item, repository))
        .collect())
}

fn decoded_content_of(item: &Content, repository: &GitHubRepository) -> Result<String> {
    item.decoded_content().ok_or_else(|| {
        anyhow!(
            "Could not read {}: GitHub reported it as a {} and carried no content for it",
            repository.path_named(&item.path),
            item.r#type
        )
    })
}

pub async fn list_directory_files(
    repository: &GitHubRepository,
    directory: &str,
    at: &GitCommit,
    octocrab: &Arc<Octocrab>,
) -> Result<Vec<String>, EnvironmentFailure> {
    let contents = octocrab
        .repos(repository.owner.as_ref(), repository.repository.as_ref())
        .get_content()
        .path(directory.to_owned())
        .r#ref(at.as_ref())
        .send()
        .await
        .map_err(|failure| EnvironmentFailure::GitHubFetch {
            path: repository.path_named(directory),
            failure,
        })?;

    let mut file_paths: Vec<String> = contents
        .items
        .into_iter()
        .filter(|item| item.r#type == "file")
        .map(|item| item.path)
        .collect();
    file_paths.sort();
    Ok(file_paths)
}

#[cfg(test)]
pub(crate) fn content_github_could_send(
    path: &str,
    kind: &str,
    encoded: Option<&str>,
) -> serde_json::Value {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut document = serde_json::json!({
        "name": name,
        "path": path,
        "sha": "8d3dfe1",
        "size": 7,
        "url": "https://api.github.invalid/contents",
        "type": kind,
        "_links": { "self": "https://api.github.invalid/contents" }
    });
    if let Some(encoded) = encoded {
        document["content"] = serde_json::Value::String(encoded.to_owned());
    }
    document
}

#[cfg(test)]
mod tests;

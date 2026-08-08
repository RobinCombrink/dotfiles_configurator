use {
    anyhow::{Context, Result},
    github_authentication::authentication::{Authentication, GitHubCliAuthentication},
    octocrab::Octocrab,
    secrecy::SecretString,
    std::sync::Arc,
};

pub struct AuthenticatedAccount {
    token: SecretString,
    client: Arc<Octocrab>,
}

impl AuthenticatedAccount {
    pub fn authenticate_as(account: &str) -> Result<Self> {
        let authentication =
            GitHubCliAuthentication::new(account.to_owned()).with_context(|| {
                format!("Could not authenticate as {account} through the GitHub CLI")
            })?;
        let token = authentication.get_token();
        let client = Octocrab::builder().personal_token(token.clone()).build()?;

        Ok(Self {
            token,
            client: Arc::new(client),
        })
    }

    pub fn client(&self) -> &Arc<Octocrab> {
        &self.client
    }

    pub fn token(&self) -> &SecretString {
        &self.token
    }
}

/// Reads the decoded contents of a file held in a GitHub repository.
pub async fn get_file_contents(
    owner: &str,
    repo: &str,
    file_path: &str,
    octocrab: &Arc<Octocrab>,
) -> Result<Vec<String>> {
    let contents = octocrab
        .repos(owner.to_owned(), repo.to_owned())
        .get_content()
        .path(file_path.to_owned())
        .send()
        .await
        .with_context(|| format!("Could not read {owner}/{repo}/{file_path}"))?;

    Ok(contents
        .items
        .iter()
        .filter_map(|item| item.decoded_content())
        .collect())
}

pub async fn list_directory_files(
    owner: &str,
    repository: &str,
    directory: &str,
    octocrab: &Arc<Octocrab>,
) -> Result<Vec<String>> {
    let contents = octocrab
        .repos(owner.to_owned(), repository.to_owned())
        .get_content()
        .path(directory.to_owned())
        .send()
        .await
        .with_context(|| format!("Could not read {owner}/{repository}/{directory}"))?;

    let mut file_paths: Vec<String> = contents
        .items
        .into_iter()
        .filter(|item| item.r#type == "file")
        .map(|item| item.path)
        .collect();
    file_paths.sort();
    Ok(file_paths)
}

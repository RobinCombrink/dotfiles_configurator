use {
    anyhow::{Context, Result},
    github_authentication::authentication::{Authentication, GitHubCliAuthentication},
    octocrab::Octocrab,
    secrecy::SecretString,
    std::sync::Arc,
};

pub fn create_octocrab(token: SecretString) -> Result<Arc<Octocrab>> {
    let instance = Octocrab::builder().personal_token(token).build()?;

    Ok(Arc::new(instance))
}

pub fn authenticated_client(username: &str) -> Result<Arc<Octocrab>> {
    let authentication = GitHubCliAuthentication::new(username.to_owned())
        .with_context(|| format!("Could not authenticate as {username} through the GitHub CLI"))?;

    create_octocrab(authentication.get_token())
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

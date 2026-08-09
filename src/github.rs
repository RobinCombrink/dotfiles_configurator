use {
    crate::configuration::GitHubAccount,
    anyhow::{Context, Result, anyhow},
    github_authentication::{GitHubToken, cli},
    octocrab::Octocrab,
    std::sync::Arc,
};

pub struct AuthenticatedAccount {
    token: GitHubToken,
    client: Arc<Octocrab>,
}

impl AuthenticatedAccount {
    pub fn authenticate_as(account: &GitHubAccount) -> Result<Self> {
        let token =
            cli::token_for(account.as_ref()).map_err(|refusal| match remedy_for(&refusal) {
                Some(remedy) => anyhow!("{refusal}. {remedy}"),
                None => anyhow!("{refusal}"),
            })?;
        let client = Octocrab::builder()
            .personal_token(token.secret().clone())
            .build()?;

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

fn remedy_for(refusal: &cli::Refusal) -> Option<String> {
    match refusal {
        cli::Refusal::ToolAbsent => Some(
            "Install it from https://cli.github.com, then authenticate with `gh auth login`"
                .to_owned(),
        ),
        cli::Refusal::AccountUnheld { account } => {
            Some(format!("Run `gh auth login` and sign in as {account}"))
        }
        cli::Refusal::Failed { .. } => None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_tool_is_answered_with_how_to_install_it() {
        let remedy = remedy_for(&cli::Refusal::ToolAbsent);

        assert_eq!(
            remedy.as_deref(),
            Some("Install it from https://cli.github.com, then authenticate with `gh auth login`")
        );
    }

    #[test]
    fn an_account_the_tool_does_not_hold_is_answered_with_how_to_sign_in_as_it() {
        let remedy = remedy_for(&cli::Refusal::AccountUnheld {
            account: "Alice".to_owned(),
        });

        assert_eq!(
            remedy.as_deref(),
            Some("Run `gh auth login` and sign in as Alice")
        );
    }

    #[test]
    fn a_failure_with_no_act_behind_it_is_answered_with_no_remedy() {
        let remedy = remedy_for(&cli::Refusal::Failed {
            account: "Alice".to_owned(),
            reason: "the token it wrote is not valid UTF-8".to_owned(),
        });

        assert_eq!(remedy, None);
    }
}

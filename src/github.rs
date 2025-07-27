use {
    anyhow::{Context, Error, Result},
    common::configuration::Configuration,
    octocrab::Octocrab,
    secrecy::SecretString,
    std::sync::Arc,
};

pub fn create_octocrab(token: SecretString) -> Result<Arc<Octocrab>> {
    let instance = Octocrab::builder().personal_token(token).build()?;

    Ok(Arc::new(instance))
}

pub async fn get_configs_from_github(
    owner: &str,
    repo: &str,
    file_path: impl Into<String>,
    octocrab: &Arc<Octocrab>,
) -> Result<Vec<Result<Configuration, Error>>> {
    let file_path: String = file_path.into();
    let config_file_info = octocrab
        .repos(owner.to_owned(), repo.to_owned())
        .get_content()
        .path(file_path.clone())
        .send()
        .await
        .with_context(|| format!("{owner}/{repo}/{file_path}"))?;

    let configs = config_file_info
        .items
        .iter()
        .map(|item| item.decoded_content())
        .filter_map(|content| content)
        .map(
            |content| match serde_json::from_str::<Configuration>(content.as_str()) {
                Ok(config) => Ok(config),
                Err(err) => Err(err.into()),
            },
        )
        .collect();
    Ok(configs)
}

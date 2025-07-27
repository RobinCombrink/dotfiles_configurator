use {
    anyhow::{Context, Error, Result, anyhow},
    common::configuration::Configuration,
    git2::{Cred, FetchOptions, RemoteCallbacks},
    indicatif::ProgressBar,
    log::{error, info, trace},
    octocrab::Octocrab,
    secrecy::{ExposeSecret, SecretString},
    std::{
        io,
        process::Command,
        sync::{Arc, LazyLock},
    },
};

static GITHUB_CLI_PRESENT: LazyLock<bool> =
    LazyLock::new(|| is_github_cli_on_path().unwrap_or_else(|_| false));

fn is_github_cli_on_path() -> Result<bool> {
    match Command::new("gh").output() {
        Ok(_) => {
            trace!("Github cli present: gh");
            Ok(true)
        }
        Err(e) => {
            if let io::ErrorKind::NotFound = e.kind() {
                Ok(false)
            } else {
                error!("An unknown error has occured: {}", e);
                Err(e).with_context(||format!("An unknown error has occured while checking if the `gh` command was available"))
            }
        }
    }
}

pub fn initialise_octocrab(token: SecretString) -> Result<Arc<Octocrab>> {
    if let false = *GITHUB_CLI_PRESENT {
        return Err(anyhow!(
            "`gh` was not found! Install github cli and/or add it to your path"
        ));
    };

    let instance = Octocrab::builder()
        .personal_token(token)
        .build()
        .expect("Invalid token");

    let octocrab = octocrab::initialise(instance);
    info!("Octocrab initialized");

    Ok(octocrab)
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

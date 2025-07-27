use {
    crate::impls::ExecutorSync,
    anyhow::{anyhow, Context, Error, Result},
    common::configuration::{Configuration, ShellCommand},
    git2::{Cred, FetchOptions, RemoteCallbacks},
    indicatif::ProgressBar,
    log::{error, info, trace},
    octocrab::Octocrab,
    secrecy::{ExposeSecret, SecretString},
    std::{
        io,
        process::{exit, Command},
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

fn switch_github_cli_user(user: &str) -> Result<()> {
    let command = ShellCommand::new(
        vec![
            "gh".into(),
            "auth".into(),
            "switch".into(),
            "--user".into(),
            format!("{user}"),
        ],
        false,
        false,
    );
    command.execute_sync()?;
    Ok(())
}

pub(crate) fn get_github_token_for_user(username: &str) -> Result<SecretString> {
    switch_github_cli_user(username)?;
    Ok(get_github_token())
}

pub(crate) fn get_github_token() -> SecretString {
    let command = ShellCommand::new(
        vec!["gh".to_string(), "auth".to_string(), "token".to_string()],
        true,
        false,
    );

    let token = match command.execute_sync() {
        Ok(token) => token.trim().to_owned().into(),
        Err(err) => {
            eprintln!("Could not get token: {:#?}", err);
            exit(1)
        }
    };
    token
}

pub(crate) fn create_repository_fetch_options<'token>(
    token: &'token SecretString,
    default_username: &'token str,
    progress_bar: ProgressBar,
) -> FetchOptions<'token> {
    let mut callbacks = RemoteCallbacks::new();

    let mut last_logged_progress = 0;
    callbacks.transfer_progress(move |progress| {
        let progress_percent =
            ((progress.received_objects() as f64 / progress.total_objects() as f64) * 100 as f64)
                .ceil() as u64;
        let should_update_position = progress_percent != last_logged_progress
            && (progress_percent == 0 || progress_percent % 5 == 0);

        if should_update_position {
            progress_bar.set_position(progress_percent);
            last_logged_progress = progress_percent;
        }
        if progress_percent >= 100 {
            progress_bar.finish_using_style();
        }
        true
    });

    callbacks.credentials(move |_url, username_from_url, _allowed_types| {
        Cred::userpass_plaintext(
            username_from_url.unwrap_or_else(|| &default_username),
            token.expose_secret(),
        )
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    fetch_options.depth(1);

    return fetch_options;
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

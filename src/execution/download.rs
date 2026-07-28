use {
    crate::configuration::{ApplicationDetails, AssetFind, RepositoryDetails, ShellCommand},
    crate::execution::Executor,
    anyhow::{Context, Error, Result, anyhow},
    futures::StreamExt,
    indicatif::ProgressBar,
    log::trace,
    reqwest::{Client, header},
    std::{
        borrow::Cow,
        fs::{self, create_dir_all},
        path::PathBuf,
    },
    url::Url,
};

pub trait Downloader {
    async fn download_self(
        &self,
        client: Client,
        download_directory: PathBuf,
        progress_bar: ProgressBar,
    ) -> Result<()>;

    async fn download(
        client: Client,
        url: &Url,
        file_path: &PathBuf,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        let debug = false;

        let name = match file_path.file_name() {
            Some(name) => name.to_owned(),
            None => "temp_download".to_owned().into(),
        };

        if debug && let Ok(true) = file_path.try_exists() {
            fs::remove_file(file_path)
                .with_context(|| format!("Could not remove file: {:#?}", file_path))?;
        }

        let total_size = {
            let resp = client.head(url.to_owned()).send().await?;
            if resp.status().is_success() {
                resp.headers()
                    .get(header::CONTENT_LENGTH)
                    .and_then(|ct_len| ct_len.to_str().ok())
                    .and_then(|ct_len| ct_len.parse().ok())
                    .unwrap_or(0)
            } else {
                return Err(anyhow!(format!(
                    "Couldn't download URL: {}.\nResponse: {:?}\nBody: {}",
                    url,
                    resp.status(),
                    resp.text().await.with_context(
                        || "Couldn't get response body text after error status code"
                    )?,
                )));
            }
        };

        let mut request = client.get(url.as_str());
        progress_bar.set_message(format!("Downloading {:#?} into {:#?}", name, file_path));
        progress_bar.set_length(total_size);

        if file_path.exists() {
            let size = file_path.metadata()?.len() - 1;
            request = request.header(header::RANGE, format!("bytes={}-", size));
            progress_bar.inc(size);
        }

        let mut source = request
            .send()
            .await
            .with_context(|| format!("Something went wrong beginning file download: {}", url))?
            .bytes_stream();

        if let Some(parent_directory_path) = file_path.parent() {
            create_dir_all(parent_directory_path)?;
        }
        let destination_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .with_context(|| format!("Could not open file: {:#?}", file_path))?;

        let mut tmp_file = progress_bar
            .wrap_async_write(tokio::fs::File::from(destination_file))
            .with_finish(indicatif::ProgressFinish::WithMessage(Cow::from(format!(
                "Downloaded {:#?} into {:#?}",
                name, file_path
            ))));

        while let Some(item) = source.next().await {
            tokio::io::copy(&mut item?.as_ref(), &mut tmp_file).await?;
        }
        Self::run(file_path).await
    }

    async fn run(to_run_path: &PathBuf) -> Result<()> {
        let to_run_path = to_run_path
            .to_str()
            .with_context(|| format!("Could not convert {:#?} to a utf-8 string", to_run_path))?;
        let command = ShellCommand::new(vec![to_run_path.to_string()], false, true);
        (match command.execute().await {
            Ok(ok) => Ok(ok),
            Err(err) => Err(Into::<Error>::into(err)),
        })
        .with_context(|| format!("Could not run downloaded application at {:#?}", to_run_path))
    }
}

impl Downloader for ApplicationDetails {
    async fn download_self(
        &self,
        client: Client,
        download_directory: PathBuf,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        Self::download(
            client,
            &self.uri,
            &download_directory.join(&self.name),
            progress_bar,
        )
        .await
    }
}

impl Downloader for RepositoryDetails {
    async fn download_self(
        &self,
        client: Client,
        download_directory: PathBuf,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        let release = octocrab::instance()
            .repos(&self.owner, &self.repo)
            .releases()
            .get_latest()
            .await?;

        trace!("RepositoryDetails: {:#?}", self);

        let asset = &release
            .assets
            .iter()
            .find(|asset| {
                trace!("Asset name: {:#?}", asset.name);
                if let Some(asset_find) = &self.asset_find {
                    trace!("{:#?}", asset_find);
                    match asset_find {
                        AssetFind::Contains { asset_contains } => {
                            asset.name.contains(asset_contains)
                        }
                        AssetFind::Exact { asset_exact } => {
                            asset.name == *asset_exact
                                || asset
                                    .label
                                    .to_owned()
                                    .is_some_and(|label| label == *asset_exact)
                        }
                        AssetFind::EndsWith { asset_ends_with } => {
                            asset.name.ends_with(asset_ends_with)
                        }
                    }
                } else {
                    false
                }
            })
            .unwrap();

        Self::download(
            client,
            &asset.browser_download_url.to_owned(),
            &download_directory.join(&asset.name),
            progress_bar,
        )
        .await
    }
}

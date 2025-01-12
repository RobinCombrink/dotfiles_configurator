use anyhow::{anyhow, Result};
use futures::StreamExt;
use indicatif::ProgressBar;
use reqwest::{header, Client};
use std::{
    borrow::Cow,
    fs::{self},
    path::PathBuf,
    process::Command,
};
use url::Url;

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
        let name = match file_path.file_name() {
            Some(name) => name.to_owned(),
            None => "temp_download".to_owned().into(),
        };

        if let Ok(exists) = file_path.try_exists() {
            if exists {
                fs::remove_file(file_path)?;
            }
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
                    "Couldn't download URL: {}. Error: {:?}",
                    url,
                    resp.status(),
                ))
                .into());
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

        let mut source = request.send().await?.bytes_stream();

        let destination_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)?;

        let mut tmp_file = progress_bar
            .wrap_async_write(tokio::fs::File::from(destination_file))
            .with_finish(indicatif::ProgressFinish::WithMessage(Cow::from(format!(
                "Downloaded {:#?} into {:#?}",
                name, file_path
            ))));

        while let Some(item) = source.next().await {
            tokio::io::copy(&mut item?.as_ref(), &mut tmp_file).await?;
        }
        Self::run(file_path)
    }

    fn run(to_run_path: &PathBuf) -> Result<()> {
        match Command::new("cmd")
            .args(&["/C", to_run_path.to_str().unwrap()])
            .spawn()
        {
            Ok(_) => Ok(()),
            Err(err) => Err(anyhow!(
                "There was a problem installing the downloaded application at {:#?}\n{err}",
                to_run_path
            )),
        }
    }
}

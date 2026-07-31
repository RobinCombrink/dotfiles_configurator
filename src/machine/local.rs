use {
    crate::{
        configuration::{
            Application, ApplicationSource, AssetPattern, MachineSettings, PresenceCheck, Shell,
        },
        github,
        machine::{
            CommandOutput, ReadInvocation, ReadMachine, Tool, WriteInvocation, WriteMachine,
        },
    },
    anyhow::{Context, Result, anyhow, bail},
    futures::StreamExt,
    git2::{Cred, FetchOptions, RemoteCallbacks, build::RepoBuilder},
    github_authentication::authentication::{Authentication, GitHubCliAuthentication},
    log::info,
    octocrab::Octocrab,
    reqwest::{Client, header},
    secrecy::ExposeSecret,
    std::{
        env, fs,
        path::{Path, PathBuf},
        process::{Command as ProcessCommand, Stdio},
        sync::Arc,
    },
    url::Url,
};

/// The machine this process is running on.
pub struct LocalMachine {
    home_directory: PathBuf,
    repositories_directory: PathBuf,
    dotfiles_repository_path: PathBuf,
    download_directory: PathBuf,
    authentication: GitHubCliAuthentication,
    octocrab: Arc<Octocrab>,
    http_client: Client,
}

impl LocalMachine {
    pub fn new(settings: &MachineSettings) -> Result<Self> {
        let authentication = GitHubCliAuthentication::new(settings.github_username.clone())
            .with_context(|| {
                format!(
                    "Could not authenticate as {} through the GitHub CLI",
                    settings.github_username
                )
            })?;
        let octocrab = github::create_octocrab(authentication.get_token())?;

        Ok(Self {
            home_directory: env::home_dir()
                .ok_or_else(|| anyhow!("Could not find the home directory"))?,
            repositories_directory: settings.repositories_directory_path.clone(),
            dotfiles_repository_path: settings.dotfiles_repository_path(),
            download_directory: dirs::download_dir()
                .ok_or_else(|| anyhow!("Could not find the download directory"))?,
            authentication,
            octocrab,
            http_client: Client::default(),
        })
    }

    fn run(
        &self,
        program: &str,
        arguments: &[String],
        utf16_output: bool,
    ) -> Result<CommandOutput> {
        info!("Reading: {program} {}", arguments.join(" "));
        let output = ProcessCommand::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("Could not run {program}"))?;

        Ok(CommandOutput {
            succeeded: output.status.success(),
            standard_output: decode_output(&output.stdout, utf16_output),
            standard_error: decode_output(&output.stderr, utf16_output),
        })
    }

    async fn download(&self, url: &Url, destination: &Path) -> Result<()> {
        if let Some(parent_directory) = destination.parent() {
            fs::create_dir_all(parent_directory)?;
        }

        // Downloaded whole into a partial file and renamed on completion, so an interrupted run
        // leaves nothing that a later run could mistake for a finished download.
        let partial_path = destination.with_extension("partial");
        let _ = fs::remove_file(&partial_path);

        let response = self
            .http_client
            .get(url.as_str())
            .send()
            .await
            .with_context(|| format!("Could not start downloading {url}"))?;

        if !response.status().is_success() {
            bail!("Downloading {url} failed with status {}", response.status());
        }

        let total_bytes = response
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|length| length.to_str().ok())
            .and_then(|length| length.parse::<u64>().ok());
        if let Some(total_bytes) = total_bytes {
            info!("Downloading {total_bytes} bytes from {url}");
        }

        let mut partial_file = tokio::fs::File::create(&partial_path)
            .await
            .with_context(|| format!("Could not create {}", partial_path.display()))?;
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            tokio::io::copy(&mut chunk?.as_ref(), &mut partial_file).await?;
        }
        drop(partial_file);

        fs::rename(&partial_path, destination).with_context(|| {
            format!(
                "Could not move {} into place at {}",
                partial_path.display(),
                destination.display()
            )
        })
    }

    async fn release_asset_url(
        &self,
        owner: &str,
        repo: &str,
        asset: &AssetPattern,
    ) -> Result<(Url, String)> {
        let release = self
            .octocrab
            .repos(owner, repo)
            .releases()
            .get_latest()
            .await
            .with_context(|| format!("Could not read the latest release of {owner}/{repo}"))?;

        let matched = release
            .assets
            .iter()
            .find(|candidate| asset.matches(&candidate.name))
            .ok_or_else(|| {
                anyhow!(
                    "No asset of the latest {owner}/{repo} release matches {asset:?}. Assets: {}",
                    release
                        .assets
                        .iter()
                        .map(|candidate| candidate.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

        Ok((matched.browser_download_url.clone(), matched.name.clone()))
    }

    fn run_installer(&self, installer_path: &Path) -> Result<()> {
        info!("Running installer {}", installer_path.display());
        let status = ProcessCommand::new(installer_path)
            .stdin(Stdio::null())
            .status()
            .with_context(|| format!("Could not run {}", installer_path.display()))?;

        match status.success() {
            true => Ok(()),
            false => bail!(
                "{} exited with {:?}",
                installer_path.display(),
                status.code()
            ),
        }
    }

    fn fetch_options<'token>(
        &'token self,
        token: &'token secrecy::SecretString,
        owner: &'token str,
    ) -> FetchOptions<'token> {
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(move |_url, username_from_url, _allowed| {
            Cred::userpass_plaintext(username_from_url.unwrap_or(owner), token.expose_secret())
        });

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);
        fetch_options.depth(1);
        fetch_options
    }
}

/// `wsl.exe` emits UTF-16LE whenever its output is redirected. That decodes as *valid* UTF-8 with
/// a NUL between every character, so a naive match silently never succeeds; stripping the NULs is
/// enough to compare against, and is what was agreed rather than an encoding library.
fn decode_output(bytes: &[u8], utf16_output: bool) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    match utf16_output {
        true => decoded.replace('\0', ""),
        false => decoded.into_owned(),
    }
}

/// A declared check names any program it likes, and some Windows programs — `wsl.exe` among them
/// — emit UTF-16LE the moment their output is redirected. That decodes as *valid* UTF-8 with a
/// NUL between every character, so a match against it silently never succeeds. Dropping the NULs
/// before matching costs nothing on output that has none.
fn matchable(output: &str) -> std::borrow::Cow<'_, str> {
    match output.contains('\0') {
        true => std::borrow::Cow::Owned(output.replace('\0', "")),
        false => std::borrow::Cow::Borrowed(output),
    }
}

fn program_is_on_path(program: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    let extensions: Vec<String> = match env::var("PATHEXT") {
        Ok(pathext) => std::iter::once(String::new())
            .chain(pathext.split(';').map(str::to_lowercase))
            .collect(),
        Err(_) => vec![String::new()],
    };

    env::split_paths(&path).any(|directory| {
        extensions
            .iter()
            .any(|extension| directory.join(format!("{program}{extension}")).is_file())
    })
}

impl ReadMachine for LocalMachine {
    fn home_directory(&self) -> &Path {
        &self.home_directory
    }

    fn repositories_directory(&self) -> &Path {
        &self.repositories_directory
    }

    fn dotfiles_repository_path(&self) -> &Path {
        &self.dotfiles_repository_path
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.symlink_metadata().is_ok()
    }

    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        fs::read_link(path).ok()
    }

    fn tool_is_present(&self, tool: Tool) -> bool {
        program_is_on_path(tool.program())
    }

    fn read(&self, invocation: &ReadInvocation) -> Result<CommandOutput> {
        self.run(
            invocation.tool().program(),
            &invocation.arguments(),
            invocation.output_is_utf16(),
        )
    }

    fn check_presence(&self, check: &PresenceCheck) -> Result<bool> {
        match check {
            PresenceCheck::PathExists { path } => {
                Ok(self.path_exists(&self.resolve_against_home(path)))
            }
            PresenceCheck::CommandOnPath { command } => Ok(program_is_on_path(command)),
            PresenceCheck::CommandOutputContains {
                shell,
                args,
                contains,
            } => {
                let output = self.run_declared_command(*shell, args)?;
                Ok(matchable(&output.standard_output).contains(contains))
            }
        }
    }
}

impl WriteMachine for LocalMachine {
    fn create_link(&self, link_path: &Path, target_path: &Path) -> Result<()> {
        if let Some(parent_directory) = link_path.parent() {
            fs::create_dir_all(parent_directory).with_context(|| {
                format!(
                    "Could not create the directory holding {}",
                    link_path.display()
                )
            })?;
        }

        if self.path_exists(link_path) {
            remove_existing(link_path)?;
        }

        create_link(link_path, target_path).with_context(|| {
            format!(
                "Could not link {} to {}",
                link_path.display(),
                target_path.display()
            )
        })
    }

    async fn clone_repository(&self, owner: &str, repo: &str) -> Result<()> {
        let token = self.authentication.get_token();
        let repository = self
            .octocrab
            .repos(owner, repo)
            .get()
            .await
            .with_context(|| format!("Could not read the details of {owner}/{repo}"))?;

        fs::create_dir_all(&self.repositories_directory).with_context(|| {
            format!("Could not create {}", self.repositories_directory.display())
        })?;
        let directory_path = self.repositories_directory.join(repo);

        let url = repository
            .html_url
            .ok_or_else(|| anyhow!("{owner}/{repo} has no html url"))?;

        RepoBuilder::new()
            .fetch_options(self.fetch_options(&token, owner))
            .clone(url.as_str(), &directory_path)
            .map(|_| ())
            .map_err(|error| {
                let _ = fs::remove_dir_all(&directory_path);
                anyhow!(error)
            })
            .with_context(|| format!("Could not clone {url} into {}", directory_path.display()))
    }

    async fn install_application(&self, application: &Application) -> Result<()> {
        let (url, file_name) = match &application.source {
            ApplicationSource::Uri {
                uri,
                installer_file_name,
            } => (uri.clone(), installer_file_name.clone()),
            ApplicationSource::GitHubRelease { owner, repo, asset } => {
                self.release_asset_url(owner, repo, asset).await?
            }
        };

        let installer_path = self.download_directory.join(file_name);
        self.download(&url, &installer_path).await?;
        self.run_installer(&installer_path)
    }

    fn write(&self, invocation: &WriteInvocation) -> Result<CommandOutput> {
        let output = self.run(invocation.tool().program(), &invocation.arguments(), false)?;
        match output.succeeded {
            true => Ok(output),
            false => bail!(
                "{} {} failed:\n{}\n{}",
                invocation.tool(),
                invocation.arguments().join(" "),
                output.standard_output.trim(),
                output.standard_error.trim()
            ),
        }
    }

    fn run_declared_command(&self, shell: Shell, args: &[String]) -> Result<CommandOutput> {
        let (program, arguments) = shell_invocation(shell, args);
        self.run(&program, &arguments, false)
    }
}

/// Renders a declared command for the shell that runs it. Every shell is given the command as a
/// single string through its own "run this" flag, which is what makes quoting the shell's problem
/// rather than something each call site re-derives.
fn shell_invocation(shell: Shell, args: &[String]) -> (String, Vec<String>) {
    let command = args.join(" ");
    match shell {
        Shell::Bash => ("bash".to_owned(), vec!["-c".to_owned(), command]),
        Shell::CommandPrompt => ("cmd".to_owned(), vec!["/C".to_owned(), command]),
        Shell::PowerShell => (
            "pwsh".to_owned(),
            vec!["-NoProfile".to_owned(), "-Command".to_owned(), command],
        ),
        Shell::Wsl => (
            "wsl".to_owned(),
            vec!["--".to_owned(), "bash".to_owned(), "-c".to_owned(), command],
        ),
    }
}

fn remove_existing(path: &Path) -> Result<()> {
    let metadata = path.symlink_metadata()?;
    match metadata.is_dir() {
        true => fs::remove_dir_all(path),
        false => fs::remove_file(path),
    }
    .with_context(|| format!("Could not remove what is already at {}", path.display()))
}

#[cfg(target_family = "windows")]
fn create_link(link_path: &Path, target_path: &Path) -> Result<()> {
    match target_path.is_dir() {
        true => std::os::windows::fs::symlink_dir(target_path, link_path),
        false => std::os::windows::fs::symlink_file(target_path, link_path),
    }
    .map_err(Into::into)
}

#[cfg(target_family = "unix")]
fn create_link(link_path: &Path, target_path: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target_path, link_path).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirected_wsl_output_is_matchable_once_its_nul_padding_is_stripped() {
        let utf16_ubuntu: Vec<u8> = "Ubuntu".encode_utf16().flat_map(u16::to_le_bytes).collect();

        let decoded = decode_output(&utf16_ubuntu, true);

        assert!(decoded.contains("Ubuntu"));
    }

    #[test]
    fn output_from_a_tool_that_does_not_redirect_as_utf16_is_left_alone() {
        let decoded = decode_output(b"committed v1.1.11:", false);

        assert_eq!(decoded, "committed v1.1.11:");
    }

    #[test]
    fn a_declared_check_matches_output_a_windows_program_redirected_as_utf16() {
        let redirected = String::from_utf8_lossy(
            &"Ubuntu (Default)"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<u8>>(),
        )
        .into_owned();

        assert!(matchable(&redirected).contains("Ubuntu (Default)"));
    }

    #[test]
    fn a_declared_command_reaches_its_shell_as_one_string_so_the_shell_does_the_quoting() {
        let (program, arguments) = shell_invocation(
            Shell::CommandPrompt,
            &["echo".to_owned(), "hello world".to_owned()],
        );

        assert_eq!(program, "cmd");
        assert_eq!(arguments, vec!["/C", "echo hello world"]);
    }

    #[test]
    fn a_wsl_command_is_run_through_wsl_rather_than_through_the_command_prompt() {
        let (program, arguments) = shell_invocation(Shell::Wsl, &["ls".to_owned()]);

        assert_eq!(program, "wsl");
        assert_eq!(arguments, vec!["--", "bash", "-c", "ls"]);
    }
}

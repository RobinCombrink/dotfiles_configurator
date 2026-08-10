use {
    crate::{
        configuration::{
            ApplicationSource, ArchiveEntry, AssetPattern, CrateName, GitHubAccount,
            GitHubRepository, Installer, PresenceCheck, ReleasedBinary, Shell,
        },
        github::AuthenticatedAccount,
        machine::{
            CommandOutput, DisplacingInvocation, Placement, ReadInvocation, ReadMachine,
            SUPERSEDED_SUFFIX, Tool, WriteInvocation, WriteMachine,
            release_reading::{ReleaseAsset, ReleaseReading},
            superseded_name,
            workspace_reading::{Revision, WorkspaceReading},
        },
        reporting::RunReport,
        version::Version,
    },
    anyhow::{Context, Result, anyhow, bail},
    futures::StreamExt,
    git2::{Cred, FetchOptions, RemoteCallbacks, build::RepoBuilder},
    indicatif::ProgressBar,
    reqwest::{Client, header},
    secrecy::ExposeSecret,
    std::{
        collections::BTreeMap,
        env, fs,
        io::{BufRead, BufReader, Read},
        path::{Path, PathBuf},
        process::{Command as ProcessCommand, Stdio},
        sync::{Arc, Mutex, PoisonError},
        thread,
    },
    url::Url,
};

pub mod workspace;

/// The machine this process is running on.
pub struct LocalMachine<'report> {
    home_directory: PathBuf,
    download_directory: PathBuf,
    cargo_binaries_directory: PathBuf,
    authenticated_accounts: Mutex<BTreeMap<GitHubAccount, Arc<AuthenticatedAccount>>>,
    http_client: Client,
    report: &'report RunReport,
}

impl<'report> LocalMachine<'report> {
    pub fn new(report: &'report RunReport) -> Result<Self> {
        let home_directory =
            env::home_dir().ok_or_else(|| anyhow!("Could not find the home directory"))?;

        Ok(Self {
            download_directory: dirs::download_dir()
                .ok_or_else(|| anyhow!("Could not find the download directory"))?,
            cargo_binaries_directory: cargo_binaries_directory(&home_directory),
            home_directory,
            authenticated_accounts: Mutex::new(BTreeMap::new()),
            http_client: Client::default(),
            report,
        })
    }

    fn run(
        &self,
        tool: Tool,
        arguments: &[String],
        environment: &[(&str, &str)],
    ) -> Result<CommandOutput> {
        stream(
            Path::new(tool.program()),
            arguments,
            environment,
            self.report,
        )
    }

    fn authenticated_as(&self, account: &GitHubAccount) -> Result<Arc<AuthenticatedAccount>> {
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

        let name = destination
            .file_name()
            .unwrap_or(destination.as_os_str())
            .to_string_lossy()
            .into_owned();
        let progress = self
            .report
            .progress_bar(total_bytes, format!("downloading {name}"));

        let mut partial_file = tokio::fs::File::create(&partial_path)
            .await
            .with_context(|| format!("Could not create {}", partial_path.display()))?;
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            progress.inc(chunk.len() as u64);
            tokio::io::copy(&mut chunk.as_ref(), &mut partial_file).await?;
        }
        drop(partial_file);
        progress.finish_with_message(format!("downloaded {name}"));

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
        account: &GitHubAccount,
    ) -> Result<(Url, String)> {
        let release = self
            .authenticated_as(account)?
            .client()
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
        let output = stream(installer_path, &[], &[], self.report)?;

        match output.succeeded {
            true => Ok(()),
            false => bail!(
                "{} failed:\n{}\n{}",
                installer_path.display(),
                output.standard_output.trim(),
                output.standard_error.trim()
            ),
        }
    }

    fn fetch_options<'token>(
        &'token self,
        token: &'token secrecy::SecretString,
        account: &'token str,
        progress: &'token ProgressBar,
    ) -> FetchOptions<'token> {
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(move |_url, username_from_url, _allowed| {
            Cred::userpass_plaintext(username_from_url.unwrap_or(account), token.expose_secret())
        });
        callbacks.transfer_progress(move |transfer| {
            progress.set_length(transfer.total_objects() as u64);
            progress.set_position(transfer.received_objects() as u64);
            true
        });

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);
        fetch_options.depth(1);
        fetch_options
    }
}

fn rendered_invocation(program: &Path, arguments: &[String]) -> String {
    match arguments.is_empty() {
        true => program.display().to_string(),
        false => format!("{} {}", program.display(), arguments.join(" ")),
    }
}

fn capture(program: &Path, arguments: &[String], report: &RunReport) -> Result<CommandOutput> {
    report.note(&rendered_invocation(program, arguments));

    let output = ProcessCommand::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("Could not run {}", program.display()))?;

    let standard_output = decode_output(&output.stdout);
    let standard_error = decode_output(&output.stderr);
    report.captured_output(&standard_output);
    report.captured_output(&standard_error);

    Ok(CommandOutput {
        succeeded: output.status.success(),
        standard_output,
        standard_error,
    })
}

// ADR 0013
fn stream(
    program: &Path,
    arguments: &[String],
    environment: &[(&str, &str)],
    report: &RunReport,
) -> Result<CommandOutput> {
    report.announce(&rendered_invocation(program, arguments));

    let mut child = ProcessCommand::new(program)
        .args(arguments)
        .envs(environment.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Could not run {}", program.display()))?;

    let piped_output = child.stdout.take().ok_or_else(|| {
        anyhow!(
            "{} gave nothing to read on standard output",
            program.display()
        )
    })?;
    let piped_error = child.stderr.take().ok_or_else(|| {
        anyhow!(
            "{} gave nothing to read on standard error",
            program.display()
        )
    })?;

    let (standard_output, standard_error) = thread::scope(|scope| {
        let reading_output = scope.spawn(|| drain(piped_output, report));
        let reading_error = scope.spawn(|| drain(piped_error, report));
        (
            reading_output.join().unwrap_or_default(),
            reading_error.join().unwrap_or_default(),
        )
    });

    let status = child
        .wait()
        .with_context(|| format!("Could not wait for {}", program.display()))?;

    Ok(CommandOutput {
        succeeded: status.success(),
        standard_output,
        standard_error,
    })
}

fn drain(source: impl Read, report: &RunReport) -> String {
    let mut reader = BufReader::new(source);
    let mut collected = String::new();
    let mut raw_line = Vec::new();

    loop {
        raw_line.clear();
        match reader.read_until(b'\n', &mut raw_line) {
            Ok(0) => return collected,
            Err(error) => {
                report.note(&format!("stopped reading this child's output: {error}"));
                return collected;
            }
            Ok(_) => {}
        }

        let line = decode_output(&raw_line);
        report.child_line(line.trim_end_matches(['\r', '\n']));
        collected.push_str(&line);
    }
}

/// Some Windows programs — `wsl.exe` among them — emit UTF-16LE the moment their output is
/// redirected. That decodes as *valid* UTF-8 with a NUL between every character, so a match
/// against it silently never succeeds.
///
/// Dropping the NULs is what was agreed rather than an encoding library, and doing it to every
/// program's output costs nothing on the ones that have none. Deciding it per program would be a
/// flag that has to be set correctly for each, and a declared check can name any program at all.
fn decode_output(bytes: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    match decoded.contains('\0') {
        true => decoded.replace('\0', ""),
        false => decoded.into_owned(),
    }
}

fn displace(destination: &Path) -> Result<PathBuf> {
    let superseded = superseded_name(destination);
    let _ = fs::remove_file(&superseded);
    fs::rename(destination, &superseded)?;

    Ok(superseded)
}

fn restore(superseded: &Path, destination: &Path) -> Result<()> {
    fs::rename(superseded, destination).with_context(|| {
        format!(
            "Could not put {} back at {}",
            superseded.display(),
            destination.display()
        )
    })
}

fn restoring(superseded: &Path, destination: &Path, failure: anyhow::Error) -> anyhow::Error {
    match restore(superseded, destination) {
        Ok(()) => failure,
        Err(unrestored) => failure.context(format!("{unrestored:#}")),
    }
}

fn refused(tool: Tool, arguments: &[String], output: &CommandOutput) -> anyhow::Error {
    anyhow!(
        "{tool} {} failed:\n{}\n{}",
        arguments.join(" "),
        output.standard_output.trim(),
        output.standard_error.trim()
    )
}

fn cargo_binaries_directory(home_directory: &Path) -> PathBuf {
    match env::var_os("CARGO_HOME") {
        Some(cargo_home) => PathBuf::from(cargo_home).join("bin"),
        None => home_directory.join(".cargo").join("bin"),
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

impl ReadMachine for LocalMachine<'_> {
    fn home_directory(&self) -> &Path {
        &self.home_directory
    }

    fn superseded_images(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.cargo_binaries_directory) else {
            return Vec::new();
        };

        let mut images: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.to_string_lossy().ends_with(SUPERSEDED_SUFFIX) && path.is_file())
            .collect();
        images.sort();

        images
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
        capture(
            Path::new(invocation.tool().program()),
            &invocation.arguments(),
            self.report,
        )
    }

    fn read_cargo_workspace(
        &self,
        repository_path: &Path,
        installed: &BTreeMap<CrateName, Revision>,
    ) -> Result<Option<WorkspaceReading>> {
        workspace::read(repository_path, installed, &self.cargo_binaries_directory)
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
                let (program, arguments) = shell_invocation(*shell, args);
                let output = capture(Path::new(&program), &arguments, self.report)?;
                Ok(output.standard_output.contains(contains))
            }
        }
    }

    async fn latest_release(
        &self,
        repository: &GitHubRepository,
        account: &GitHubAccount,
    ) -> Result<ReleaseReading> {
        let owner = repository.owner.as_ref();
        let name = repository.repository.as_ref();
        let release = self
            .authenticated_as(account)?
            .client()
            .repos(owner, name)
            .releases()
            .get_latest()
            .await
            .with_context(|| format!("Could not read the latest release of {repository}"))?;

        let version = Version::try_from(release.tag_name.as_str())
            .map_err(|fault| anyhow!("the latest release of {repository} is tagged {fault}"))?;

        Ok(ReleaseReading {
            version,
            assets: release
                .assets
                .into_iter()
                .map(|asset| ReleaseAsset {
                    name: asset.name,
                    download_url: asset.browser_download_url,
                })
                .collect(),
        })
    }

    fn report_version(&self, binary_path: &Path, arguments: &[String]) -> Result<CommandOutput> {
        capture(binary_path, arguments, self.report)
    }
}

impl WriteMachine for LocalMachine<'_> {
    fn create_link(&self, link_path: &Path, target_path: &Path) -> Result<()> {
        if let Some(parent_directory) = link_path.parent() {
            fs::create_dir_all(parent_directory).with_context(|| {
                format!(
                    "Could not create the directory holding {}",
                    link_path.display()
                )
            })?;
        }

        replace_existing_link(link_path)?;

        create_link(link_path, target_path).with_context(|| {
            format!(
                "Could not link {} to {}",
                link_path.display(),
                target_path.display()
            )
        })
    }

    async fn clone_repository(
        &self,
        repository: &GitHubRepository,
        clone_directory: &Path,
        account: &GitHubAccount,
    ) -> Result<()> {
        let authenticated = self.authenticated_as(account)?;
        let owner = repository.owner.as_ref();
        let name = repository.repository.as_ref();
        let details = authenticated
            .client()
            .repos(owner, name)
            .get()
            .await
            .with_context(|| format!("Could not read the details of {repository}"))?;

        if let Some(parent_directory) = clone_directory.parent() {
            fs::create_dir_all(parent_directory)
                .with_context(|| format!("Could not create {}", parent_directory.display()))?;
        }
        let directory_path = clone_directory.to_path_buf();

        let url = details
            .html_url
            .ok_or_else(|| anyhow!("{repository} has no html url"))?;

        let progress = self
            .report
            .progress_bar(None, format!("cloning {repository}"));
        let cloned = RepoBuilder::new()
            .fetch_options(self.fetch_options(
                authenticated.token().secret(),
                account.as_ref(),
                &progress,
            ))
            .clone(url.as_str(), &directory_path)
            .map(|_| ())
            .map_err(|error| {
                let _ = fs::remove_dir_all(&directory_path);
                anyhow!(error)
            })
            .with_context(|| format!("Could not clone {url} into {}", directory_path.display()));

        progress.finish_with_message(format!("cloned {repository}"));
        cloned
    }

    async fn install_application(
        &self,
        installer: &Installer,
        account: &GitHubAccount,
    ) -> Result<()> {
        let (url, file_name) = match &installer.source {
            ApplicationSource::Uri {
                uri,
                installer_file_name,
            } => (uri.clone(), installer_file_name.clone()),
            ApplicationSource::GitHubRelease {
                owner,
                repository,
                asset,
            } => {
                self.release_asset_url(owner.as_ref(), repository.as_ref(), asset, account)
                    .await?
            }
        };

        let installer_path = self.download_directory.join(file_name);
        self.download(&url, &installer_path).await?;
        self.run_installer(&installer_path)
    }

    async fn install_released_binary(
        &self,
        binary: &ReleasedBinary,
        asset: &ReleaseAsset,
    ) -> Result<Placement> {
        let archive_path = self.download_directory.join(&asset.name);
        self.download(&asset.download_url, &archive_path).await?;

        let contents = read_archive_entry(&archive_path, &binary.entry)?;
        let installed_path = self
            .binaries_directory()
            .join(binary.installed_name().as_ref());

        fs::create_dir_all(self.binaries_directory())
            .with_context(|| format!("Could not create {}", self.binaries_directory().display()))?;

        // ADR 0022
        let Err(refused) = place_executable(&installed_path, &contents) else {
            return Ok(Placement::Placed);
        };
        let Ok(superseded) = displace(&installed_path) else {
            return Ok(Placement::Held(installed_path));
        };

        place_executable(&installed_path, &contents)
            .map(|()| Placement::Placed)
            .map_err(|failure| restoring(&superseded, &installed_path, failure))
            .with_context(|| {
                format!(
                    "{} could not be written: {refused:#}",
                    installed_path.display()
                )
            })
    }

    fn write(&self, invocation: &WriteInvocation) -> Result<CommandOutput> {
        let arguments = invocation.arguments();
        let output = self.run(invocation.tool(), &arguments, &[])?;
        match output.succeeded {
            true => Ok(output),
            false => Err(refused(invocation.tool(), &arguments, &output)),
        }
    }

    fn write_displacing(&self, invocation: &DisplacingInvocation) -> Result<Placement> {
        let tool = invocation.tool();
        let arguments = invocation.arguments();
        let environment = invocation.environment();

        let output = self.run(tool, &arguments, &environment)?;
        if output.succeeded {
            return Ok(Placement::Placed);
        }

        let Some(destination) = invocation.refused_destination(&output) else {
            return Err(refused(tool, &arguments, &output));
        };

        let Ok(superseded) = displace(&destination) else {
            return Ok(Placement::Held(destination));
        };
        self.report
            .note(&format!("displaced {}", destination.display()));

        match self.run(tool, &arguments, &environment) {
            Ok(retried) if retried.succeeded => Ok(Placement::Placed),
            Ok(retried) => Err(restoring(
                &superseded,
                &destination,
                refused(tool, &arguments, &retried),
            )),
            Err(error) => Err(restoring(&superseded, &destination, error)),
        }
    }

    fn sweep_superseded_images(&self) {
        for image in self.superseded_images() {
            if fs::remove_file(&image).is_ok() {
                self.report.note(&format!("removed {}", image.display()));
            }
        }
    }

    fn run_declared_command(&self, shell: Shell, args: &[String]) -> Result<CommandOutput> {
        let (program, arguments) = shell_invocation(shell, args);
        stream(Path::new(&program), &arguments, &[], self.report)
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

/// Clears the way for a link, and only ever by removing another link.
///
/// A link is this tool's own work and removing one destroys nothing, so a link pointing somewhere
/// else is replaced. Anything else at that path was put there by a person: convergence makes
/// declared things true and never makes undeclared things false, so a real file or directory in
/// the way is reported rather than deleted. See ADR 0005.
fn replace_existing_link(link_path: &Path) -> Result<()> {
    let Ok(metadata) = link_path.symlink_metadata() else {
        return Ok(());
    };

    if !metadata.file_type().is_symlink() {
        bail!(
            "{} already exists and is not a link. Move it aside to let the dotfiles repository \
             own it; this tool will not delete something it did not create.",
            link_path.display()
        );
    }

    // A directory symlink is removed with `remove_dir`, which unlinks it without touching
    // whatever it points at.
    match metadata.is_dir() {
        true => fs::remove_dir(link_path),
        false => fs::remove_file(link_path),
    }
    .with_context(|| format!("Could not remove the link at {}", link_path.display()))
}

fn read_archive_entry(archive_path: &Path, entry: &ArchiveEntry) -> Result<Vec<u8>> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("Could not open {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Could not read {} as a zip archive", archive_path.display()))?;

    let held: Vec<String> = archive.file_names().map(str::to_owned).collect();
    let mut held_entry = archive.by_name(&entry.to_string()).map_err(|_| {
        anyhow!(
            "{} holds no entry {entry}. It holds {}",
            archive_path.display(),
            held.join(", ")
        )
    })?;

    let mut contents = Vec::new();
    held_entry
        .read_to_end(&mut contents)
        .with_context(|| format!("Could not read {entry} out of {}", archive_path.display()))?;
    Ok(contents)
}

fn place_executable(destination: &Path, contents: &[u8]) -> Result<()> {
    fs::write(destination, contents)
        .with_context(|| format!("Could not write {}", destination.display()))?;
    make_executable(destination)
}

#[cfg(target_family = "windows")]
fn make_executable(_destination: &Path) -> Result<()> {
    Ok(())
}

#[cfg(target_family = "unix")]
fn make_executable(destination: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(destination)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(destination, permissions)
        .with_context(|| format!("Could not make {} executable", destination.display()))
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
    use {super::*, crate::reporting::RunKind};

    fn a_binary_at(directory: &Path, name: &str, contents: &str) -> PathBuf {
        let path = directory.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn what_is_left_behind_keeps_its_extension_so_the_search_path_cannot_invoke_it() {
        let superseded = superseded_name(Path::new("C:\\cargo\\bin\\claude-session.exe"));

        assert_eq!(
            superseded,
            PathBuf::from("C:\\cargo\\bin\\claude-session.exe.superseded")
        );
    }

    #[test]
    fn displacing_a_binary_frees_its_name_and_keeps_the_image_beside_it() {
        let directory = tempfile::tempdir().unwrap();
        let destination = a_binary_at(directory.path(), "claude-session.exe", "the old image");

        let superseded = displace(&destination).unwrap();

        assert!(!destination.exists(), "the name should be free to write");
        assert_eq!(fs::read_to_string(&superseded).unwrap(), "the old image");
    }

    #[test]
    fn displacing_a_binary_reaps_the_image_an_earlier_run_left_under_that_exact_name() {
        let directory = tempfile::tempdir().unwrap();
        let destination = a_binary_at(directory.path(), "claude-session.exe", "the current image");
        a_binary_at(
            directory.path(),
            "claude-session.exe.superseded",
            "an image from an earlier run",
        );

        let superseded = displace(&destination).unwrap();

        assert_eq!(
            fs::read_to_string(&superseded).unwrap(),
            "the current image"
        );
    }

    #[test]
    fn restoring_puts_the_moved_image_back_under_the_name_it_had() {
        let directory = tempfile::tempdir().unwrap();
        let destination = a_binary_at(directory.path(), "claude-session.exe", "the old image");
        let superseded = displace(&destination).unwrap();

        restore(&superseded, &destination).unwrap();

        assert_eq!(fs::read_to_string(&destination).unwrap(), "the old image");
        assert!(!superseded.exists());
    }

    #[cfg(target_family = "windows")]
    const A_SHELL_EVERY_MACHINE_HAS: Shell = Shell::CommandPrompt;
    #[cfg(target_family = "unix")]
    const A_SHELL_EVERY_MACHINE_HAS: Shell = Shell::Bash;

    fn echoing_two_lines() -> (String, Vec<String>) {
        shell_invocation(
            A_SHELL_EVERY_MACHINE_HAS,
            &[
                "echo".to_owned(),
                "first".to_owned(),
                "&&".to_owned(),
                "echo".to_owned(),
                "second".to_owned(),
            ],
        )
    }

    #[test]
    fn every_line_a_changing_child_writes_reaches_the_log() {
        let directory = tempfile::tempdir().unwrap();
        let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
        let (program, arguments) = echoing_two_lines();

        let output = stream(Path::new(&program), &arguments, &[], &report).unwrap();

        let written = fs::read_to_string(report.log_path().unwrap()).unwrap();
        assert!(output.succeeded, "{output:?}");
        assert!(
            written.contains("first") && written.contains("second"),
            "{written}"
        );
    }

    #[test]
    fn a_streamed_child_still_hands_back_its_output_for_a_failure_message_to_quote() {
        let directory = tempfile::tempdir().unwrap();
        let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
        let (program, arguments) = echoing_two_lines();

        let output = stream(Path::new(&program), &arguments, &[], &report).unwrap();

        assert!(output.standard_output.contains("first"), "{output:?}");
        assert!(output.standard_output.contains("second"), "{output:?}");
    }

    #[test]
    fn the_log_names_the_invocation_a_captured_read_ran() {
        let directory = tempfile::tempdir().unwrap();
        let report = RunReport::open_in(directory.path(), RunKind::Plan).unwrap();
        let (program, arguments) = echoing_two_lines();

        capture(Path::new(&program), &arguments, &report).unwrap();

        let written = fs::read_to_string(report.log_path().unwrap()).unwrap();
        assert!(written.contains(&program), "{written}");
    }

    #[test]
    fn output_a_program_redirected_as_utf16_is_matchable_once_its_nuls_are_stripped() {
        let redirected: Vec<u8> = "Ubuntu (Default)"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();

        assert!(decode_output(&redirected).contains("Ubuntu (Default)"));
    }

    #[test]
    fn output_from_a_program_that_does_not_redirect_as_utf16_is_left_alone() {
        assert_eq!(decode_output(b"committed v1.1.11:"), "committed v1.1.11:");
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

    fn archive_holding(directory: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let archive_path = directory.join("release.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
        for (name, contents) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, contents).unwrap();
        }
        writer.finish().unwrap();
        archive_path
    }

    fn entry(path: &str) -> ArchiveEntry {
        ArchiveEntry::try_from(path.to_owned()).unwrap()
    }

    #[test]
    fn the_entry_read_out_of_an_archive_is_the_one_the_configuration_named() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = archive_holding(
            directory.path(),
            &[("notes.txt", b"read me"), ("bin/rg.exe", b"the binary")],
        );

        let contents = read_archive_entry(&archive_path, &entry("bin/rg.exe")).unwrap();

        assert_eq!(contents, b"the binary");
    }

    #[test]
    fn an_archive_holding_no_such_entry_is_refused_by_naming_what_it_does_hold() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = archive_holding(directory.path(), &[("rg.exe", b"the binary")]);

        let error = read_archive_entry(&archive_path, &entry("bin/rg.exe"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("rg.exe"), "{error}");
    }

    #[test]
    fn a_binary_written_where_none_is_installed_needs_no_displacement() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("rg.exe");

        place_executable(&destination, b"the new copy").unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"the new copy");
        assert!(!directory.path().join("rg.exe.superseded").exists());
    }

    #[test]
    fn a_wsl_command_is_run_through_wsl_rather_than_through_the_command_prompt() {
        let (program, arguments) = shell_invocation(Shell::Wsl, &["ls".to_owned()]);

        assert_eq!(program, "wsl");
        assert_eq!(arguments, vec!["--", "bash", "-c", "ls"]);
    }
}

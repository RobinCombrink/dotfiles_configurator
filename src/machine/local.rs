use {
    crate::{
        configuration::{
            Answer, ApplicationSource, ArchiveEntry, CloneDepth, CrateName, CrateVersion,
            DartPackage, GitHubAccount, GitHubRepository, Installer, Migration, PresenceCheck,
            ReleasedBinary, RepositoryClone, Shell, VariableName, VariableValue,
        },
        configuration_source::WriteSource,
        github::GitHubAccess,
        machine::{
            CommandOutput, ContentDigest, DisplacingInvocation, Downloaded, ElevatedBatch,
            ElevatedWork, Elevation, Exited, HeldReason, Placement, PrivilegeRefusal,
            ReadInvocation, ReadMachine, Replacement, ReplacingInvocation, SUPERSEDED_SUFFIX, Tool,
            WorkspaceBuild, WriteInvocation, WriteMachine, crate_index_reading,
            dart_reading::DartReading,
            environment_reading::SearchPathReading,
            invocation::without_terminal_prompts,
            partial_download_path,
            release_reading::{ReleaseAsset, ReleaseReading},
            superseded_name,
            workspace_reading::{Fingerprint, Revision, WorkspaceReading},
        },
        reporting::{Advancing, Entry, RunReport, entry_of_this_task},
        version::Version,
    },
    anyhow::{Context, Result, anyhow, bail},
    futures::StreamExt,
    git2::{Cred, FetchOptions, RemoteCallbacks, build::RepoBuilder},
    reqwest::{Client, header},
    secrecy::ExposeSecret,
    std::{
        collections::{BTreeMap, BTreeSet},
        env,
        ffi::{OsStr, OsString},
        fs,
        io::Read,
        path::{Path, PathBuf},
        process::{Command as ProcessCommand, Stdio},
    },
    tokio::io::{AsyncBufReadExt, AsyncRead},
    url::Url,
};

pub mod dart;
pub mod elevation;
pub mod environment;
#[cfg(target_family = "windows")]
pub mod privileges;
pub mod workspace;

const NOTHING_PUBLISHED: u16 = 404;

pub struct LocalMachine<'report, 'access> {
    home_directory: PathBuf,
    tool_directory: PathBuf,
    download_directory: PathBuf,
    cargo_binaries_directory: PathBuf,
    github: &'access GitHubAccess,
    http_client: Client,
    report: &'report RunReport,
    elevated: bool,
}

impl<'report, 'access> LocalMachine<'report, 'access> {
    pub fn new(
        report: &'report RunReport,
        github: &'access GitHubAccess,
        tool_directory: PathBuf,
    ) -> Result<Self> {
        let home_directory =
            env::home_dir().ok_or_else(|| anyhow!("Could not find the home directory"))?;

        Ok(Self {
            download_directory: dirs::download_dir()
                .ok_or_else(|| anyhow!("Could not find the download directory"))?,
            cargo_binaries_directory: cargo_binaries_directory(&home_directory),
            home_directory,
            tool_directory,
            github,
            http_client: Client::default(),
            report,
            elevated: this_process_is_elevated()?,
        })
    }

    #[cfg(target_family = "windows")]
    pub fn note_the_privileges_it_holds(&self) {
        for line in privileges::opening_lines(self.elevated, &privileges::developer_mode_is_on()) {
            self.report.note(&line);
        }
    }

    async fn run(
        &self,
        tool: Tool,
        arguments: &[String],
        environment: &[(String, String)],
    ) -> Result<CommandOutput> {
        stream(
            Path::new(tool.program()),
            arguments,
            environment,
            None,
            self.report,
        )
        .await
    }

    async fn download(&self, url: &Url, destination: &Path) -> Result<()> {
        if let Some(parent_directory) = destination.parent() {
            fs::create_dir_all(parent_directory)?;
        }

        let partial_path = partial_download_path(destination);
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
        let progress = self.report.advancing("downloading", &name, total_bytes);

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
        progress.finish(format!("downloaded {name}"));

        fs::rename(&partial_path, destination).with_context(|| {
            format!(
                "Could not move {} into place at {}",
                partial_path.display(),
                destination.display()
            )
        })
    }

    async fn run_installer(&self, installer_path: &Path) -> Result<Placement> {
        match run_installer_at(installer_path, self.report).await {
            Ok(()) => Ok(Placement::Placed),
            Err(error) if refused_for_want_of(&error, Want::Elevation) => {
                let digest = fs::File::open(installer_path)
                    .and_then(ContentDigest::of)
                    .with_context(|| format!("Could not read {}", installer_path.display()))?;
                Ok(Placement::Refused(PrivilegeRefusal {
                    work: ElevatedWork::Installer {
                        installer_path: installer_path.to_path_buf(),
                        digest,
                    },
                    refusal: format!("{error:#}"),
                }))
            }
            Err(error) => Err(error),
        }
    }

    fn git(&self, clone_directory: &Path, arguments: &[&str]) -> Result<()> {
        let arguments: Vec<String> = ["-C".to_owned(), clone_directory.display().to_string()]
            .into_iter()
            .chain(arguments.iter().map(|argument| (*argument).to_owned()))
            .collect();
        let output = capture(Path::new(Tool::Git.program()), &arguments, self.report)?;

        match output.exited.succeeded() {
            true => Ok(()),
            false => Err(refused(Tool::Git, &arguments, &output)),
        }
    }

    fn fetch_options<'token>(
        &'token self,
        token: &'token secrecy::SecretString,
        account: &'token str,
        progress: &'token Advancing<'_>,
        depth: FetchDepth,
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
        fetch_options.depth(depth.as_git_depth());
        fetch_options
    }
}

#[derive(Debug, Clone, Copy)]
enum FetchDepth {
    WholeHistory,
    Commits(CloneDepth),
    DeepenedToWholeHistory,
}

impl FetchDepth {
    const GIT_WHOLE_HISTORY: i32 = 0;
    const GIT_UNSHALLOW: i32 = i32::MAX;

    fn as_git_depth(self) -> i32 {
        match self {
            FetchDepth::WholeHistory => Self::GIT_WHOLE_HISTORY,
            FetchDepth::Commits(depth) => {
                i32::try_from(depth.commits().get()).unwrap_or(Self::GIT_UNSHALLOW)
            }
            FetchDepth::DeepenedToWholeHistory => Self::GIT_UNSHALLOW,
        }
    }
}

impl From<&RepositoryClone> for FetchDepth {
    fn from(clone: &RepositoryClone) -> Self {
        match clone.depth {
            None => FetchDepth::WholeHistory,
            Some(depth) => FetchDepth::Commits(depth),
        }
    }
}

fn builds_of_other_revisions_reaped(
    build_cache: &Path,
    keeping: &BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(build_cache) else {
        return Vec::new();
    };

    let mut reaped = Vec::new();
    for path in entries.flatten().map(|entry| entry.path()) {
        if keeping.contains(&path) {
            continue;
        }

        if fs::remove_dir_all(&path).is_ok() {
            reaped.push(path);
        }
    }

    reaped
}

fn rendered_invocation(program: &Path, arguments: &[String]) -> String {
    match arguments.is_empty() {
        true => program.display().to_string(),
        false => format!("{} {}", program.display(), arguments.join(" ")),
    }
}

fn capture(program: &Path, arguments: &[String], report: &RunReport) -> Result<CommandOutput> {
    capture_with(program, arguments, &[], report)
}

fn capture_with(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    report: &RunReport,
) -> Result<CommandOutput> {
    report.note(&rendered_invocation(program, arguments));

    let output = ProcessCommand::new(launchable(program, environment))
        .args(arguments)
        .envs(environment.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("Could not run {}", program.display()))?;

    let standard_output = decode_output(&output.stdout);
    let standard_error = decode_output(&output.stderr);
    report.captured_output(&standard_output);
    report.captured_output(&standard_error);
    let exited = Exited::from(output.status);
    report.note(&format!("{} {exited}", program.display()));

    Ok(CommandOutput {
        exited,
        standard_output,
        standard_error,
    })
}

// ADR 0013
async fn stream(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    working_directory: Option<&Path>,
    report: &RunReport,
) -> Result<CommandOutput> {
    let speaking_for = entry_of_this_task();
    report.child_line(
        speaking_for.as_ref(),
        &rendered_invocation(program, arguments),
    );

    let mut command = tokio::process::Command::new(launchable(program, environment));
    if let Some(directory) = working_directory {
        command.current_dir(directory);
    }
    let mut child = command
        .args(arguments)
        .envs(environment.iter().map(|(name, value)| (name, value)))
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

    let (standard_output, standard_error) = tokio::join!(
        drain(piped_output, report, speaking_for.as_ref()),
        drain(piped_error, report, speaking_for.as_ref())
    );

    let status = child
        .wait()
        .await
        .with_context(|| format!("Could not wait for {}", program.display()))?;
    let exited = Exited::from(status);
    report.child_line(
        speaking_for.as_ref(),
        &format!("{} {exited}", program.display()),
    );

    Ok(CommandOutput {
        exited,
        standard_output,
        standard_error,
    })
}

async fn drain(
    source: impl AsyncRead + Unpin,
    report: &RunReport,
    speaking_for: Option<&Entry>,
) -> String {
    let mut reader = tokio::io::BufReader::new(source);
    let mut collected = String::new();
    let mut raw_line = Vec::new();

    loop {
        raw_line.clear();
        match reader.read_until(b'\n', &mut raw_line).await {
            Ok(0) => return collected,
            Err(error) => {
                report.note(&format!("stopped reading this child's output: {error}"));
                return collected;
            }
            Ok(_) => {}
        }

        let line = decode_output(&raw_line);
        report.child_line(speaking_for, line.trim_end_matches(['\r', '\n']));
        collected.push_str(&line);
    }
}

// ADR 0036
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

fn holds_the_same_bytes(source: &Path, destination: &Path) -> Result<bool> {
    let wanted =
        fs::read(source).with_context(|| format!("Could not read {}", source.display()))?;
    let held = fs::read(destination)
        .with_context(|| format!("Could not read {}", destination.display()))?;

    Ok(wanted == held)
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
        "{tool} {} failed, {}:\n{}\n{}",
        arguments.join(" "),
        output.exited,
        output.standard_output.trim(),
        output.standard_error.trim()
    )
}

async fn run_installer_at(installer_path: &Path, report: &RunReport) -> Result<()> {
    let output = stream(installer_path, &[], &[], None, report).await?;

    match output.exited.succeeded() {
        true => Ok(()),
        false => bail!(
            "{} failed, {}:\n{}\n{}",
            installer_path.display(),
            output.exited,
            output.standard_output.trim(),
            output.standard_error.trim()
        ),
    }
}

fn place_link(link_path: &Path, target_path: &Path) -> Result<()> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    Privilege,
    Elevation,
}

#[cfg(target_family = "windows")]
fn refused_for_want_of(error: &anyhow::Error, want: Want) -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ELEVATION_REQUIRED, ERROR_PRIVILEGE_NOT_HELD};

    let refusal = match want {
        Want::Privilege => ERROR_PRIVILEGE_NOT_HELD,
        Want::Elevation => ERROR_ELEVATION_REQUIRED,
    };
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .any(|cause| cause.raw_os_error() == Some(refusal.cast_signed()))
}

#[cfg(target_family = "unix")]
fn refused_for_want_of(_error: &anyhow::Error, _want: Want) -> bool {
    false
}

#[cfg(target_family = "windows")]
fn this_process_is_elevated() -> Result<bool> {
    privileges::this_process_is_elevated()
}

#[cfg(target_family = "unix")]
fn this_process_is_elevated() -> Result<bool> {
    Ok(false)
}

fn cargo_binaries_directory(home_directory: &Path) -> PathBuf {
    match env::var_os("CARGO_HOME") {
        Some(cargo_home) => PathBuf::from(cargo_home).join("bin"),
        None => home_directory.join(".cargo").join("bin"),
    }
}

fn superseded_images_in(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };

    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(SUPERSEDED_SUFFIX) && path.is_file())
        .collect()
}

fn executable_extensions(pathext: Option<&OsStr>) -> Vec<String> {
    let Some(pathext) = pathext else {
        return vec![String::new()];
    };

    std::iter::once(String::new())
        .chain(pathext.to_string_lossy().split(';').map(str::to_lowercase))
        .collect()
}

fn read_text_file(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Could not read {}", path.display())),
    }
}

fn program_is_on_path(program: &str) -> bool {
    let extensions = executable_extensions(env::var_os("PATHEXT").as_deref());
    resolved_on(program, env::var_os("PATH").as_deref(), &extensions).is_some()
}

fn resolved_on(
    program: &str,
    search_path: Option<&OsStr>,
    extensions: &[String],
) -> Option<PathBuf> {
    env::split_paths(search_path?).find_map(|directory| {
        extensions
            .iter()
            .map(|extension| directory.join(format!("{program}{extension}")))
            .find(|candidate| candidate.is_file())
    })
}

fn launchable(program: &Path, environment: &[(String, String)]) -> PathBuf {
    let is_a_bare_name = program.parent() == Some(Path::new(""));
    if !is_a_bare_name {
        return program.to_path_buf();
    }

    let search_path = environment
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| OsString::from(value))
        .or_else(|| env::var_os("PATH"));
    let extensions = launchable_extensions(env::var_os("PATHEXT").as_deref());
    resolved_on(
        &program.to_string_lossy(),
        search_path.as_deref(),
        &extensions,
    )
    .unwrap_or_else(|| program.to_path_buf())
}

fn launchable_extensions(pathext: Option<&OsStr>) -> Vec<String> {
    let extensions = executable_extensions(pathext);
    match pathext {
        None => extensions,
        Some(_) => extensions
            .into_iter()
            .filter(|extension| !extension.is_empty())
            .collect(),
    }
}

impl ReadMachine for LocalMachine<'_, '_> {
    fn home_directory(&self) -> &Path {
        &self.home_directory
    }

    fn tool_directory(&self) -> &Path {
        &self.tool_directory
    }

    fn superseded_images(&self) -> Vec<PathBuf> {
        let mut images: Vec<PathBuf> = self
            .displacement_directories()
            .iter()
            .flat_map(|directory| superseded_images_in(directory))
            .collect();
        images.sort();

        images
    }

    fn displacement_directories(&self) -> Vec<PathBuf> {
        vec![
            self.cargo_binaries_directory.clone(),
            self.binaries_directory(),
        ]
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.symlink_metadata().is_ok()
    }

    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        fs::read_link(path).ok()
    }

    fn canonical_path(&self, path: &Path) -> Option<PathBuf> {
        path.canonicalize().ok()
    }

    fn text_file_at(&self, path: &Path) -> Result<Option<String>> {
        read_text_file(path)
    }

    fn tool_is_present(&self, tool: Tool) -> bool {
        program_is_on_path(tool.program())
    }

    fn is_elevated(&self) -> bool {
        self.elevated
    }

    fn read(&self, invocation: &ReadInvocation) -> Result<CommandOutput> {
        capture(
            Path::new(invocation.tool().program()),
            &invocation.arguments(),
            self.report,
        )
    }

    fn read_cargo_workspace(&self, repository_path: &Path) -> Result<Option<WorkspaceReading>> {
        workspace::read(repository_path)
    }

    fn read_cargo_workspace_at(
        &self,
        repository_path: &Path,
        revision: &Revision,
    ) -> Result<BTreeMap<CrateName, Fingerprint>> {
        workspace::read_at(repository_path, revision)
    }

    fn cargo_binaries_directory(&self) -> PathBuf {
        self.cargo_binaries_directory.clone()
    }

    fn check_presence(&self, check: &PresenceCheck) -> Result<Option<Answer>> {
        match check {
            PresenceCheck::PathExists { paths } => Ok(paths
                .first_found(|candidate| self.path_exists(&self.resolve_against_home(candidate)))),
            PresenceCheck::CommandOnPath { command } => {
                Ok(program_is_on_path(command).then_some(Answer::TheCheckItself))
            }
            PresenceCheck::CommandOutputContains {
                shell,
                args,
                contains,
            } => {
                let (program, arguments) = shell_invocation(*shell, args);
                let output = capture(Path::new(&program), &arguments, self.report)?;
                Ok(output
                    .standard_output
                    .contains(contains)
                    .then_some(Answer::TheCheckItself))
            }
        }
    }

    fn clone_is_shallow(&self, clone_directory: &Path) -> Result<bool> {
        let clone = git2::Repository::open(clone_directory).with_context(|| {
            format!("Could not open the clone at {}", clone_directory.display())
        })?;
        Ok(clone.is_shallow())
    }

    async fn latest_release(
        &self,
        repository: &GitHubRepository,
        account: &GitHubAccount,
    ) -> Result<Option<ReleaseReading>> {
        let owner = repository.owner.as_ref();
        let name = repository.repository.as_ref();
        let asked = self
            .github
            .account(account)?
            .client()
            .repos(owner, name)
            .releases()
            .get_latest()
            .await;

        let release = match asked {
            Ok(release) => release,
            Err(octocrab::Error::GitHub { source, .. })
                if source.status_code.as_u16() == NOTHING_PUBLISHED =>
            {
                return Ok(None);
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Could not read the latest release of {repository}"));
            }
        };

        let version = Version::try_from(release.tag_name.as_str())
            .map_err(|fault| anyhow!("the latest release of {repository} is tagged {fault}"))?;

        Ok(Some(ReleaseReading {
            version,
            assets: release
                .assets
                .into_iter()
                .map(|asset| ReleaseAsset {
                    name: asset.name,
                    download_url: asset.browser_download_url,
                })
                .collect(),
        }))
    }

    async fn newest_published_crate(&self, crate_name: &CrateName) -> Result<CrateVersion> {
        let address = format!(
            "{}/{}",
            crate_index_reading::SPARSE_INDEX,
            crate_index_reading::index_file_of(crate_name)
        );
        let response = self
            .http_client
            .get(&address)
            .send()
            .await
            .with_context(|| format!("Could not ask crates.io for {crate_name}"))?;
        if response.status().as_u16() == NOTHING_PUBLISHED {
            bail!("crates.io holds no crate named {crate_name}");
        }

        let index_file = response
            .error_for_status()
            .with_context(|| format!("crates.io refused to list {crate_name}"))?
            .text()
            .await
            .with_context(|| format!("Could not read what crates.io lists for {crate_name}"))?;
        crate_index_reading::newest_listed(crate_name, &index_file)
            .map_err(|reason| anyhow!(reason))
    }

    fn report_version(&self, binary_path: &Path, arguments: &[String]) -> Result<CommandOutput> {
        capture(binary_path, arguments, self.report)
    }

    fn read_search_path(&self) -> Result<SearchPathReading> {
        environment::read_search_path()
    }

    fn read_environment_variable(&self, name: &VariableName) -> Result<Option<VariableValue>> {
        environment::read_variable(name)
    }

    fn read_dart_package(&self, package: &DartPackage) -> Result<DartReading> {
        dart::read(&self.dart_locations(), package, &|directory, arguments| {
            let arguments: Vec<String> = ["-C".to_owned(), directory.display().to_string()]
                .into_iter()
                .chain(arguments.iter().map(|argument| (*argument).to_owned()))
                .collect();
            capture_with(
                Path::new(Tool::Git.program()),
                &arguments,
                &without_terminal_prompts(),
                self.report,
            )
        })
    }
}

impl WriteMachine for LocalMachine<'_, '_> {
    fn create_link(&self, link_path: &Path, target_path: &Path) -> Result<Placement> {
        match place_link(link_path, target_path) {
            Ok(()) => Ok(Placement::Placed),
            Err(error) if refused_for_want_of(&error, Want::Privilege) => {
                Ok(Placement::Refused(PrivilegeRefusal {
                    work: ElevatedWork::Link {
                        link_path: link_path.to_path_buf(),
                        target_path: target_path.to_path_buf(),
                    },
                    refusal: format!("{error:#}"),
                }))
            }
            Err(error) => Err(error),
        }
    }

    fn write_text_file(&self, path: &Path, contents: &str) -> Result<()> {
        if let Some(parent_directory) = path.parent() {
            fs::create_dir_all(parent_directory).with_context(|| {
                format!("Could not create the directory holding {}", path.display())
            })?;
        }

        fs::write(path, contents).with_context(|| format!("Could not write {}", path.display()))
    }

    async fn clone_repository(
        &self,
        clone: &RepositoryClone,
        clone_directory: &Path,
        account: &GitHubAccount,
    ) -> Result<()> {
        let repository = &clone.repository;
        let authenticated = self.github.account(account)?;
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

        let progress = self.report.advancing("cloning", repository, None);
        let cloned = RepoBuilder::new()
            .fetch_options(self.fetch_options(
                authenticated.token().secret(),
                account.as_ref(),
                &progress,
                FetchDepth::from(clone),
            ))
            .clone(url.as_str(), &directory_path)
            .map(|_| ())
            .map_err(|error| {
                let _ = fs::remove_dir_all(&directory_path);
                anyhow!(error)
            })
            .with_context(|| format!("Could not clone {url} into {}", directory_path.display()));

        progress.finish(format!("cloned {repository}"));
        cloned
    }

    async fn deepen_clone(
        &self,
        repository: &GitHubRepository,
        clone_directory: &Path,
        account: &GitHubAccount,
    ) -> Result<()> {
        let authenticated = self.github.account(account)?;
        let clone = git2::Repository::open(clone_directory).with_context(|| {
            format!("Could not open the clone at {}", clone_directory.display())
        })?;
        let mut origin = clone
            .find_remote("origin")
            .with_context(|| format!("The clone at {} has no origin", clone_directory.display()))?;

        let progress = self.report.advancing("deepening", repository, None);
        let deepened = origin
            .fetch(
                &[] as &[&str],
                Some(&mut self.fetch_options(
                    authenticated.token().secret(),
                    account.as_ref(),
                    &progress,
                    FetchDepth::DeepenedToWholeHistory,
                )),
                None,
            )
            .with_context(|| {
                format!(
                    "Could not fetch the whole history of {repository} into {}",
                    clone_directory.display()
                )
            });

        progress.finish(format!("deepened {repository}"));
        deepened
    }

    async fn download_installer(
        &self,
        installer: &Installer,
        release_asset: Option<&ReleaseAsset>,
    ) -> Result<Downloaded<Installer>> {
        let (url, file_name) = match (&installer.source, release_asset) {
            (
                ApplicationSource::Uri {
                    uri,
                    installer_file_name,
                },
                _,
            ) => (uri.clone(), installer_file_name.clone()),
            (ApplicationSource::GitHubRelease { .. }, Some(asset)) => {
                (asset.download_url.clone(), asset.name.clone())
            }
            (ApplicationSource::GitHubRelease { .. }, None) => bail!(
                "{} could not be installed: its release asset was not read",
                installer.name
            ),
        };

        let installer_path = self.download_directory.join(file_name);
        self.download(&url, &installer_path).await?;
        Ok(Downloaded::fetched(installer.clone(), installer_path))
    }

    async fn install_application(&self, downloaded: Downloaded<Installer>) -> Result<Placement> {
        self.run_installer(downloaded.file()).await
    }

    #[cfg(target_family = "windows")]
    async fn run_elevated(&self, batch: &ElevatedBatch<()>) -> Result<Elevation> {
        elevation::run_elevated(batch, self.report).await
    }

    #[cfg(target_family = "unix")]
    async fn run_elevated(&self, _batch: &ElevatedBatch<()>) -> Result<Elevation> {
        bail!("Elevation is asked for only on Windows")
    }

    async fn download_released_binary(
        &self,
        binary: &ReleasedBinary,
        asset: &ReleaseAsset,
    ) -> Result<Downloaded<ReleasedBinary>> {
        let archive_path = self.download_directory.join(&asset.name);
        self.download(&asset.download_url, &archive_path).await?;
        Ok(Downloaded::fetched(binary.clone(), archive_path))
    }

    fn install_released_binary(&self, downloaded: Downloaded<ReleasedBinary>) -> Result<Placement> {
        let binary = downloaded.declared();
        let contents = read_archive_entry(downloaded.file(), &binary.entry)?;
        let installed_path = self
            .binaries_directory()
            .join(binary.installed_name().file_name());

        fs::create_dir_all(self.binaries_directory())
            .with_context(|| format!("Could not create {}", self.binaries_directory().display()))?;

        // ADR 0022
        let Err(refused) = place_executable(&installed_path, &contents) else {
            return Ok(Placement::Placed);
        };
        let Ok(superseded) = displace(&installed_path) else {
            return Ok(Placement::Held(HeldReason::BeingExecuted(installed_path)));
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

    fn put_on_search_path(&self, directory: &Path) -> Result<()> {
        environment::put_on_search_path(directory)
    }

    fn set_environment_variable(&self, name: &VariableName, value: &VariableValue) -> Result<()> {
        environment::set_variable(name, value)
    }

    async fn write(&self, invocation: &WriteInvocation) -> Result<CommandOutput> {
        let arguments = invocation.arguments();
        let output = self
            .run(invocation.tool(), &arguments, &invocation.environment())
            .await?;
        match output.exited.succeeded() {
            true => Ok(output),
            false => Err(refused(invocation.tool(), &arguments, &output)),
        }
    }

    async fn attempt_write(&self, invocation: &WriteInvocation) -> Result<CommandOutput> {
        self.run(
            invocation.tool(),
            &invocation.arguments(),
            &invocation.environment(),
        )
        .await
    }

    async fn write_over_running_images(&self, invocation: &WriteInvocation) -> Result<Placement> {
        let arguments = invocation.arguments();
        let output = self.run(invocation.tool(), &arguments, &[]).await?;
        if output.exited.succeeded() {
            return Ok(Placement::Placed);
        }
        if let Some(reason) = invocation.held_by(&output) {
            return Ok(Placement::Held(reason));
        }

        let Some(copy) = invocation.refused_copy(&output) else {
            return Err(refused(invocation.tool(), &arguments, &output));
        };

        match holds_the_same_bytes(&copy.source, &copy.destination)? {
            true => {
                self.report.note(&format!(
                    "{} is running and already holds what would have been copied over it",
                    copy.destination.display()
                ));
                Ok(Placement::Placed)
            }
            false => Ok(Placement::Held(HeldReason::BeingExecuted(copy.destination))),
        }
    }

    async fn write_displacing(&self, invocation: &DisplacingInvocation) -> Result<Placement> {
        let tool = invocation.tool();
        let arguments = invocation.arguments();
        let environment = invocation.environment(&self.build_cache_directory());

        let output = self.run(tool, &arguments, &environment).await?;
        if output.exited.succeeded() {
            return Ok(Placement::Placed);
        }

        let Some(destination) = invocation.refused_destination(&output) else {
            return Err(refused(tool, &arguments, &output));
        };

        let Ok(superseded) = displace(&destination) else {
            return Ok(Placement::Held(HeldReason::BeingExecuted(destination)));
        };
        self.report
            .note(&format!("displaced {}", destination.display()));

        match self.run(tool, &arguments, &environment).await {
            Ok(retried) if retried.exited.succeeded() => Ok(Placement::Placed),
            Ok(retried) => Err(restoring(
                &superseded,
                &destination,
                refused(tool, &arguments, &retried),
            )),
            Err(error) => Err(restoring(&superseded, &destination, error)),
        }
    }

    fn reap_builds_of_other_revisions(&self, building: &BTreeSet<Revision>) {
        let build_cache = self.build_cache_directory();
        let keeping: BTreeSet<PathBuf> = building
            .iter()
            .map(|revision| revision.build_directory_in(&build_cache))
            .collect();

        for reaped in builds_of_other_revisions_reaped(&build_cache, &keeping) {
            self.report
                .note(&format!("reaped the build directory {}", reaped.display()));
        }
    }

    async fn build_workspace_members(&self, build: &WorkspaceBuild<'_>) -> Result<()> {
        let build_cache = self.build_cache_directory();
        let source = build.source_directory(&build_cache);
        let clone_directory = build.clone_directory();
        let source_argument = source.display().to_string();

        if source.exists() {
            fs::remove_dir_all(&source)
                .with_context(|| format!("Could not clear {}", source.display()))?;
        }
        self.git(clone_directory, &["worktree", "prune"])?;
        self.git(
            clone_directory,
            &[
                "worktree",
                "add",
                "--detach",
                &source_argument,
                build.revision().as_ref(),
            ],
        )?;

        let arguments = build.arguments();
        let built = stream(
            Path::new(Tool::Cargo.program()),
            &arguments,
            &build.environment(&build_cache),
            Some(&source),
            self.report,
        )
        .await
        .and_then(|output| match output.exited.succeeded() {
            true => Ok(()),
            false => Err(refused(Tool::Cargo, &arguments, &output)),
        });
        let removed = self.git(
            clone_directory,
            &["worktree", "remove", "--force", &source_argument],
        );

        match (built, removed) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(failure), Ok(())) => Err(failure),
            (Ok(()), Err(unremoved)) => Err(unremoved),
            (Err(failure), Err(unremoved)) => Err(failure.context(format!("{unremoved:#}"))),
        }
    }

    async fn replace(&self, invocation: &ReplacingInvocation) -> Result<Replacement> {
        let tool = invocation.tool();
        let commands = invocation.commands();

        let the_name_was_freed = self
            .run(tool, &commands.free_the_name, &[])
            .await?
            .exited
            .succeeded();
        let claimed = self.run(tool, &commands.claim_the_name, &[]).await?;
        if claimed.exited.succeeded() {
            return Ok(Replacement::Replaced);
        }

        invocation.refused_claim(
            the_name_was_freed,
            refused(tool, &commands.claim_the_name, &claimed),
        )
    }

    fn sweep_superseded_images(&self) {
        for image in self.superseded_images() {
            if fs::remove_file(&image).is_ok() {
                self.report.note(&format!("removed {}", image.display()));
            }
        }
    }

    async fn run_declared_command(&self, shell: Shell, args: &[String]) -> Result<CommandOutput> {
        let (program, arguments) = shell_invocation(shell, args);
        stream(Path::new(&program), &arguments, &[], None, self.report).await
    }
}

impl WriteSource for LocalMachine<'_, '_> {
    fn rewrite(&self, migration: &Migration) -> Result<()> {
        fs::write(migration.path(), migration.contents())
            .with_context(|| format!("Could not rewrite {}", migration.path().display()))
    }
}

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

// ADR 0005
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
mod tests;

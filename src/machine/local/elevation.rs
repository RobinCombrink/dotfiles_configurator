use {
    super::{place_link, run_installer_at},
    crate::{
        machine::{Batched, ContentDigest, ElevatedBatch, ElevatedOutcome, ElevatedWork},
        reporting::RunReport,
    },
    anyhow::{Context, Result, bail},
    std::{fs, io::Write, path::Path},
};
#[cfg(target_family = "windows")]
use {
    crate::machine::{Elevation, Exited},
    std::{
        env,
        ffi::OsStr,
        iter::once,
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
        path::PathBuf,
        process,
    },
    windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_CANCELLED, WAIT_FAILED},
        Storage::FileSystem::FILE_SHARE_READ,
        System::{
            Com::{
                COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
            },
            Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject},
        },
        UI::{
            Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
            WindowsAndMessaging::SW_HIDE,
        },
    },
};

pub const ELEVATED_BATCH: &str = "elevated-batch";
pub const BATCH: &str = "batch";
pub const RESULTS: &str = "results";

// ADR 0042
pub async fn perform(batch: ElevatedBatch<()>, results: &Path, report: &RunReport) -> Result<()> {
    let mut results_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(results)
        .with_context(|| {
            format!(
                "Could not create {}, which must not exist before the elevated batch writes it",
                results.display()
            )
        })?;

    let mut entries = Vec::new();
    for Batched { work, .. } in batch.entries {
        let outcome = match performed(&work, report).await {
            Ok(()) => ElevatedOutcome::Converged,
            Err(error) => ElevatedOutcome::Failed {
                reason: format!("{error:#}"),
            },
        };
        report.note(&format!("{work}: {outcome}"));
        entries.push(Batched { work, outcome });
    }

    let settled = serde_json::to_string(&ElevatedBatch { entries })
        .context("Could not write down what the elevated batch settled")?;
    results_file
        .write_all(settled.as_bytes())
        .with_context(|| format!("Could not write {}", results.display()))
}

async fn performed(work: &ElevatedWork, report: &RunReport) -> Result<()> {
    match work {
        ElevatedWork::Link {
            link_path,
            target_path,
        } => place_link(link_path, target_path),
        ElevatedWork::Installer {
            installer_path,
            digest,
        } => {
            let _held = opened_unchanged(installer_path, *digest)?;
            run_installer_at(installer_path, report).await
        }
    }
}

fn opened_unchanged(installer_path: &Path, collected: ContentDigest) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_family = "windows")]
    options.share_mode(FILE_SHARE_READ);
    let mut installer = options
        .open(installer_path)
        .with_context(|| format!("Could not open {}", installer_path.display()))?;

    let found = ContentDigest::of(&mut installer)
        .with_context(|| format!("Could not read {}", installer_path.display()))?;
    if found != collected {
        bail!(
            "{} has changed since it was refused unelevated: it was {collected}, and is now \
             {found}, so it is not run",
            installer_path.display()
        );
    }
    Ok(installer)
}

// 2026-10-05: the longest command line ShellExecute's default verb accepted on the personal
// machine, Windows 11.
#[cfg(target_family = "windows")]
const LONGEST_COMMAND_LINE: usize = 30_426;

#[cfg(target_family = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Launched {
    Exited(Exited),
    Declined,
}

// ADR 0042
#[cfg(target_family = "windows")]
pub async fn run_elevated(batch: &ElevatedBatch<()>, report: &RunReport) -> Result<Elevation> {
    let executable = env::current_exe().context("Could not find this program to elevate it")?;
    let results = results_path();
    let _ = fs::remove_file(&results);
    let parameters = command_line_parameters(batch, &results)?;
    within_the_longest_command_line(&executable, &parameters)?;

    report.note(&format!("{} {parameters}", executable.display()));
    let launching = executable.clone();
    let launched = tokio::task::spawn_blocking(move || launch_elevated(&launching, &parameters))
        .await
        .context("The elevated batch was abandoned before it ended")??;

    let exited = match launched {
        Launched::Declined => {
            report.note(&format!("{} elevation declined", executable.display()));
            return Ok(Elevation::Declined);
        }
        Launched::Exited(exited) => exited,
    };
    report.note(&format!("{} {exited}", executable.display()));

    settled_by(exited, &results).map(Elevation::Performed)
}

#[cfg(target_family = "windows")]
fn settled_by(exited: Exited, results: &Path) -> Result<ElevatedBatch<ElevatedOutcome>> {
    if !exited.succeeded() {
        bail!(
            "The elevated batch {exited}, so {} is not read as what it settled",
            results.display()
        );
    }

    let written = fs::read_to_string(results);
    let _ = fs::remove_file(results);
    let written = written.with_context(|| {
        format!(
            "The elevated batch {exited} without writing {}",
            results.display()
        )
    })?;
    serde_json::from_str(&written)
        .with_context(|| format!("{} is not what an elevated batch writes", results.display()))
}

#[cfg(target_family = "windows")]
fn results_path() -> PathBuf {
    env::temp_dir().join(format!(
        "{}-elevated-batch-{}.json",
        env!("CARGO_PKG_NAME"),
        process::id()
    ))
}

#[cfg(target_family = "windows")]
fn command_line_parameters(batch: &ElevatedBatch<()>, results: &Path) -> Result<String> {
    let batch = serde_json::to_string(batch).context("Could not write the elevated batch down")?;

    Ok([
        ELEVATED_BATCH.to_owned(),
        format!("--{BATCH}"),
        batch,
        format!("--{RESULTS}"),
        results.display().to_string(),
    ]
    .iter()
    .map(|argument| quoted(argument))
    .collect::<Vec<_>>()
    .join(" "))
}

#[cfg(target_family = "windows")]
fn within_the_longest_command_line(executable: &Path, parameters: &str) -> Result<()> {
    let length = format!("{} {parameters}", quoted(&executable.display().to_string()))
        .encode_utf16()
        .count();
    if length > LONGEST_COMMAND_LINE {
        bail!(
            "The elevated batch needs a command line of {length} characters, longer than the \
             {LONGEST_COMMAND_LINE} Windows accepts, and is not split across two prompts"
        );
    }
    Ok(())
}

#[cfg(target_family = "windows")]
fn quoted(argument: &str) -> String {
    let needs_quotes = argument.is_empty() || argument.contains([' ', '\t', '\n', '\u{b}', '"']);
    if !needs_quotes {
        return argument.to_owned();
    }

    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            other => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(other);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(target_family = "windows")]
fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(once(0)).collect()
}

#[cfg(target_family = "windows")]
fn refused_to_launch(refusal: std::io::Error) -> Result<Launched> {
    match refusal.raw_os_error() == Some(ERROR_CANCELLED.cast_signed()) {
        true => Ok(Launched::Declined),
        false => Err(refusal).context("Could not start the elevated batch"),
    }
}

#[cfg(target_family = "windows")]
struct ComApartment;

#[cfg(target_family = "windows")]
impl ComApartment {
    fn entered() -> Result<Self> {
        // SAFETY: the reserved argument is null as required, and every successful call is
        // balanced by the CoUninitialize in Drop on this same thread.
        let entered = unsafe {
            CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).cast_unsigned(),
            )
        };
        if entered < 0 {
            return Err(std::io::Error::from_raw_os_error(entered))
                .context("Could not initialise COM to start the elevated batch");
        }
        Ok(Self)
    }
}

#[cfg(target_family = "windows")]
impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: entered succeeded on this thread, so this call balances it.
        unsafe { CoUninitialize() };
    }
}

#[cfg(target_family = "windows")]
fn launch_elevated(executable: &Path, parameters: &str) -> Result<Launched> {
    let verb = wide(OsStr::new("runas"));
    let file = wide(executable.as_os_str());
    let parameters = wide(OsStr::new(parameters));
    let mut execution = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_HIDE,
        ..Default::default()
    };

    let _apartment = ComApartment::entered()?;
    // SAFETY: execution is fully initialised, its size is the one declared, and the strings it
    // points at are NUL-terminated and outlive the call.
    if unsafe { ShellExecuteExW(&raw mut execution) } == 0 {
        return refused_to_launch(std::io::Error::last_os_error());
    }
    if execution.hProcess.is_null() {
        bail!("The elevated batch started without handing back a process to wait on");
    }

    // SAFETY: hProcess is the process handle SEE_MASK_NOCLOSEPROCESS asked to be handed back.
    let waited = unsafe { WaitForSingleObject(execution.hProcess, INFINITE) };
    let unwaited = (waited == WAIT_FAILED).then(std::io::Error::last_os_error);
    let mut code = 0u32;
    // SAFETY: the same handle, and code is a valid place for the exit code.
    let read = unsafe { GetExitCodeProcess(execution.hProcess, &raw mut code) };
    let unread = (read == 0).then(std::io::Error::last_os_error);
    // SAFETY: the handle was handed to this function to close, and is closed exactly once.
    unsafe { CloseHandle(execution.hProcess) };

    if let Some(failure) = unwaited {
        return Err(failure).context("Could not wait for the elevated batch to end");
    }
    match unread {
        Some(failure) => Err(failure).context("Could not read how the elevated batch exited"),
        None => Ok(Launched::Exited(Exited::Code(code.cast_signed()))),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_family = "windows")]
    use windows_sys::Win32::{Foundation::LocalFree, UI::Shell::CommandLineToArgvW};
    use {super::*, crate::reporting::RunKind, std::path::PathBuf};

    #[tokio::test]
    async fn an_entry_the_elevated_side_cannot_settle_decides_nothing_for_the_one_beside_it() {
        let directory = tempfile::tempdir().unwrap();
        let report = RunReport::open_in(&directory.path().join("logs"), RunKind::Elevated).unwrap();
        let target = directory.path().join("gitconfig");
        fs::write(&target, "[user]").unwrap();
        let occupied = directory.path().join(".npmrc");
        fs::write(&occupied, "a file of Alice's own").unwrap();
        let free = directory.path().join(".gitconfig");
        let batch = ElevatedBatch::of([
            ElevatedWork::Link {
                link_path: occupied.clone(),
                target_path: target.clone(),
            },
            ElevatedWork::Link {
                link_path: free.clone(),
                target_path: target.clone(),
            },
        ]);
        let results = directory.path().join("results.json");

        perform(batch, &results, &report).await.unwrap();

        let settled: ElevatedBatch<ElevatedOutcome> =
            serde_json::from_str(&fs::read_to_string(&results).unwrap()).unwrap();
        assert_eq!(
            settled.outcome_of(&ElevatedWork::Link {
                link_path: free,
                target_path: target,
            }),
            Some(&ElevatedOutcome::Converged)
        );
    }

    #[tokio::test]
    async fn a_results_file_already_in_place_is_refused_and_left_as_it_was() {
        let directory = tempfile::tempdir().unwrap();
        let report = RunReport::open_in(&directory.path().join("logs"), RunKind::Elevated).unwrap();
        let target = directory.path().join("gitconfig");
        fs::write(&target, "[user]").unwrap();
        let batch = ElevatedBatch::of([ElevatedWork::Link {
            link_path: directory.path().join(".gitconfig"),
            target_path: target,
        }]);
        let results = directory.path().join("results.json");
        fs::write(&results, "Mallory's results").unwrap();

        let refused = perform(batch, &results, &report).await;

        assert!(refused.is_err());
        assert_eq!(fs::read_to_string(&results).unwrap(), "Mallory's results");
    }

    #[cfg(target_family = "windows")]
    fn a_results_file_settling_in(directory: &Path) -> PathBuf {
        let results = directory.join("results.json");
        let settled = ElevatedBatch {
            entries: vec![Batched {
                work: ElevatedWork::Link {
                    link_path: PathBuf::from(r"C:\Users\Alice\.gitconfig"),
                    target_path: PathBuf::from(r"C:\Repositories\dotfiles\gitconfig"),
                },
                outcome: ElevatedOutcome::Converged,
            }],
        };
        fs::write(&results, serde_json::to_string(&settled).unwrap()).unwrap();
        results
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn a_batch_that_exited_non_zero_is_not_believed_whatever_its_results_file_says() {
        let directory = tempfile::tempdir().unwrap();
        let results = a_results_file_settling_in(directory.path());

        assert!(settled_by(Exited::Code(1), &results).is_err());
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn a_batch_that_exited_zero_is_read_from_its_results_file() {
        let directory = tempfile::tempdir().unwrap();
        let results = a_results_file_settling_in(directory.path());

        assert!(settled_by(Exited::Code(0), &results).is_ok());
    }

    fn an_installer_collected_in(directory: &Path) -> (PathBuf, ContentDigest) {
        let installer = directory.join("SteamSetup.exe");
        fs::write(&installer, "Steam's installer").unwrap();
        let collected = ContentDigest::of(fs::File::open(&installer).unwrap()).unwrap();
        (installer, collected)
    }

    #[test]
    fn an_installer_changed_since_it_was_collected_is_refused_rather_than_run() {
        let directory = tempfile::tempdir().unwrap();
        let (installer, collected) = an_installer_collected_in(directory.path());
        fs::write(&installer, "Mallory's installer").unwrap();

        assert!(opened_unchanged(&installer, collected).is_err());
    }

    #[test]
    fn an_installer_unchanged_since_it_was_collected_is_let_through_to_run() {
        let directory = tempfile::tempdir().unwrap();
        let (installer, collected) = an_installer_collected_in(directory.path());

        assert!(opened_unchanged(&installer, collected).is_ok());
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn a_prompt_the_person_cancels_is_read_as_elevation_declined() {
        let launched = refused_to_launch(std::io::Error::from_raw_os_error(1223)).unwrap();

        assert_eq!(launched, Launched::Declined);
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn a_launch_refused_for_any_other_reason_is_an_error_rather_than_a_decline() {
        assert!(refused_to_launch(std::io::Error::from_raw_os_error(2)).is_err());
    }

    #[cfg(target_family = "windows")]
    fn split_as_windows_does(command_line: &str) -> Vec<String> {
        let command_line = wide(OsStr::new(&format!("configurator.exe {command_line}")));
        let mut count = 0;
        // SAFETY: command_line is NUL-terminated and outlives the call, and count is a valid
        // place for the number of arguments.
        let arguments = unsafe { CommandLineToArgvW(command_line.as_ptr(), &raw mut count) };
        assert!(!arguments.is_null(), "{}", std::io::Error::last_os_error());

        let mut split = Vec::new();
        for position in 0..usize::try_from(count).unwrap() {
            // SAFETY: CommandLineToArgvW hands back count NUL-terminated strings, and every read
            // here stops at the NUL of the string it is reading.
            let argument = unsafe {
                let argument = *arguments.add(position);
                let mut length = 0;
                while *argument.add(length) != 0 {
                    length += 1;
                }
                std::slice::from_raw_parts(argument, length)
            };
            split.push(String::from_utf16_lossy(argument));
        }
        // SAFETY: the array was allocated by CommandLineToArgvW for the caller to free once.
        unsafe { LocalFree(arguments.cast()) };
        split
    }

    #[cfg(target_family = "windows")]
    fn a_batch_of_awkward_paths() -> ElevatedBatch<()> {
        ElevatedBatch::of([
            ElevatedWork::Link {
                link_path: PathBuf::from(r"C:\Users\Alice Smith\.claude\skills\stop-gate"),
                target_path: PathBuf::from(r#"C:\Repositories\dotfiles\claude\mods\"quoted"\"#),
            },
            ElevatedWork::Installer {
                installer_path: PathBuf::from(r"C:\Users\Alice Smith\Downloads\SteamSetup.exe"),
                digest: ContentDigest::of(&b"Steam's installer"[..]).unwrap(),
            },
        ])
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn the_batch_on_the_command_line_reaches_the_elevated_side_as_it_was_written() {
        let batch = a_batch_of_awkward_paths();
        let parameters =
            command_line_parameters(&batch, Path::new(r"C:\Temp\results file.json")).unwrap();

        let arguments = split_as_windows_does(&parameters);

        let reached = arguments
            .get(3)
            .and_then(|written| serde_json::from_str::<ElevatedBatch<()>>(written).ok());
        assert_eq!(reached, Some(batch), "{arguments:?}");
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn the_results_file_on_the_command_line_reaches_the_elevated_side_as_it_was_named() {
        let parameters = command_line_parameters(
            &a_batch_of_awkward_paths(),
            Path::new(r"C:\Temp\results file.json"),
        )
        .unwrap();

        let arguments = split_as_windows_does(&parameters);

        assert_eq!(
            arguments.get(5).map(String::as_str),
            Some(r"C:\Temp\results file.json"),
            "{arguments:?}"
        );
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn a_batch_longer_than_a_command_line_accepts_is_refused_rather_than_split() {
        let batch = ElevatedBatch::of((0..400).map(|position| ElevatedWork::Link {
            link_path: PathBuf::from(format!(r"C:\Users\Alice\.claude\skills\skill-{position}")),
            target_path: PathBuf::from(format!(r"C:\Repositories\dotfiles\claude\mods\{position}")),
        }));
        let parameters =
            command_line_parameters(&batch, Path::new(r"C:\Temp\results.json")).unwrap();

        assert!(
            within_the_longest_command_line(Path::new(r"C:\configurator.exe"), &parameters)
                .is_err()
        );
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn a_batch_within_what_a_command_line_accepts_is_let_through() {
        let parameters = command_line_parameters(
            &a_batch_of_awkward_paths(),
            Path::new(r"C:\Temp\results.json"),
        )
        .unwrap();

        assert!(
            within_the_longest_command_line(Path::new(r"C:\configurator.exe"), &parameters).is_ok()
        );
    }
}

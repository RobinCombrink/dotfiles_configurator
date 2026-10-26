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
    crate::machine::{Elevation, Exited, Placement},
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
        #[cfg(target_family = "windows")]
        ElevatedWork::MachineSearchPathFront { directory } => {
            match super::environment::put_first_on_machine_search_path(directory)? {
                Placement::Placed => Ok(()),
                Placement::Refused(refusal) => bail!("{}, elevated", refusal.refusal),
                Placement::Held(reason) => bail!("{work} was held: {reason}"),
            }
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
mod tests;

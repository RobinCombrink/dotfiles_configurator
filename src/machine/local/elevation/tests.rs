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
    let parameters = command_line_parameters(&batch, Path::new(r"C:\Temp\results.json")).unwrap();

    assert!(
        within_the_longest_command_line(Path::new(r"C:\configurator.exe"), &parameters).is_err()
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

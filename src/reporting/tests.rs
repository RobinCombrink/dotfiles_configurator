use {
    super::*,
    crate::{
        configuration::{CargoPackage, CargoSource, CrateName, Package, Resource},
        convergence::Lane,
    },
};

#[test]
fn run_logs_are_written_into_the_tools_own_directory_under_the_home_directory() {
    let home = env::home_dir().unwrap();

    assert_eq!(
        log_directory().unwrap(),
        home.join(".dotfiles_configurator").join("logs")
    );
}

fn logs_in(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| is_a_run_log(path))
        .collect()
}

fn quiet_report(directory: &Path, kind: RunKind) -> RunReport {
    RunReport::open_showing(directory, kind, Screen::Lines(Box::new(std::io::sink()))).unwrap()
}

fn stop_gate() -> Entry {
    Entry::new(
        Lane::Cargo,
        &Resource::Package(Package::Cargo(CargoPackage {
            crate_name: CrateName::from("stop-gate"),
            source: CargoSource::Registry {
                version: crate::configuration::PackageCurrency::Latest,
            },
        })),
    )
}

#[test]
fn each_run_writes_its_own_log_rather_than_appending_to_the_previous_one() {
    let directory = tempfile::tempdir().unwrap();

    let first = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
    let second = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();

    assert_ne!(first.log_path(), second.log_path());
    assert_eq!(logs_in(directory.path()).len(), 2);
}

#[test]
fn opening_more_runs_than_are_retained_discards_the_oldest_logs() {
    let directory = tempfile::tempdir().unwrap();

    for _ in 0..RETAINED_RUNS + 10 {
        RunReport::open_in(directory.path(), RunKind::Plan).unwrap();
    }

    assert_eq!(logs_in(directory.path()).len(), RETAINED_RUNS);
}

#[test]
fn the_log_holds_what_the_run_was_doing_after_the_spinner_has_taken_it_back() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();

    drop(report.doing("installing Neovim"));
    let written = fs::read_to_string(report.log_path()).unwrap();

    assert!(written.contains("installing Neovim"), "{written}");
}

#[test]
fn output_captured_for_parsing_rather_than_shown_still_reaches_the_log() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Plan).unwrap();

    report.captured_output("Name  Id  Version\nA package  Microsoft.PowerShell  1.0.0");
    let written = fs::read_to_string(report.log_path()).unwrap();

    assert!(written.contains("Microsoft.PowerShell"), "{written}");
}

#[test]
fn a_line_an_entry_child_writes_reaches_the_log_prefixed_with_that_entry() {
    let directory = tempfile::tempdir().unwrap();
    let report = quiet_report(directory.path(), RunKind::Apply);

    report.child_line(Some(&stop_gate()), "Compiling stop-gate v0.1.0");
    let written = fs::read_to_string(report.log_path()).unwrap();

    assert!(
        written.contains("[cargo] stop-gate: Compiling stop-gate v0.1.0"),
        "{written}"
    );
}

#[tokio::test]
async fn output_captured_while_converging_an_entry_reaches_the_log_prefixed_with_it() {
    let directory = tempfile::tempdir().unwrap();
    let report = quiet_report(directory.path(), RunKind::Apply);

    report
        .converging(&stop_gate(), async {
            report.captured_output("stop-gate 0.1.0");
        })
        .await;
    let written = fs::read_to_string(report.log_path()).unwrap();

    assert!(
        written.contains("[cargo] stop-gate: stop-gate 0.1.0"),
        "{written}"
    );
}

fn watched_for_silence(report: &RunReport, entry: &Entry) -> bool {
    report
        .shared
        .listening()
        .contains_key(&Speaker::Entry(entry.clone()))
}

#[tokio::test]
async fn an_entry_waiting_for_its_lane_is_not_watched_for_silence() {
    let directory = tempfile::tempdir().unwrap();
    let report = quiet_report(directory.path(), RunKind::Apply);

    report.converging(&stop_gate(), async {}).await;
    report.awaiting_its_lane(&stop_gate());

    assert!(!watched_for_silence(&report, &stop_gate()));
}

#[tokio::test]
async fn an_entry_waiting_for_elevation_is_not_watched_for_silence() {
    let directory = tempfile::tempdir().unwrap();
    let report = quiet_report(directory.path(), RunKind::Apply);

    report.converging(&stop_gate(), async {}).await;
    report.awaiting_elevation(&stop_gate(), "os error 1314");

    assert!(!watched_for_silence(&report, &stop_gate()));
}

#[tokio::test]
async fn an_entry_resumed_once_its_lane_reaches_it_is_watched_for_silence_again() {
    let directory = tempfile::tempdir().unwrap();
    let report = quiet_report(directory.path(), RunKind::Apply);
    report.converging(&stop_gate(), async {}).await;
    report.awaiting_its_lane(&stop_gate());

    let watched = report
        .resuming(&stop_gate(), async {
            watched_for_silence(&report, &stop_gate())
        })
        .await;

    assert!(watched);
}

fn ripgrep() -> Entry {
    Entry::new(
        Lane::Cargo,
        &Resource::Package(Package::Cargo(CargoPackage {
            crate_name: CrateName::from("ripgrep"),
            source: CargoSource::Registry {
                version: crate::configuration::PackageCurrency::Latest,
            },
        })),
    )
}

#[tokio::test]
async fn of_two_running_entries_only_the_one_that_went_quiet_is_named_as_silent() {
    let directory = tempfile::tempdir().unwrap();
    let report = quiet_report(directory.path(), RunKind::Apply);
    report.converging(&stop_gate(), async {}).await;
    report.converging(&ripgrep(), async {}).await;
    let long_ago = Instant::now()
        .checked_sub(SILENCE_THRESHOLD + Duration::from_secs(1))
        .expect("the machine has been up longer than the silence threshold");
    for activity in report.shared.listening().values_mut() {
        activity.last_spoke = long_ago;
    }

    report.child_line(Some(&stop_gate()), "Compiling stop-gate v0.1.0");
    let silent: Vec<String> = report
        .shared
        .fallen_silent()
        .into_iter()
        .map(|(label, _)| label)
        .collect();

    assert_eq!(silent, vec![ripgrep().to_string()]);
}

#[tokio::test]
async fn output_captured_once_an_entry_resumes_reaches_the_log_prefixed_with_it() {
    let directory = tempfile::tempdir().unwrap();
    let report = quiet_report(directory.path(), RunKind::Apply);
    report.converging(&stop_gate(), async {}).await;
    report.awaiting_its_lane(&stop_gate());

    report
        .resuming(&stop_gate(), async {
            report.captured_output("installed stop-gate");
        })
        .await;
    let written = fs::read_to_string(report.log_path()).unwrap();

    assert!(
        written.contains("[cargo] stop-gate: installed stop-gate"),
        "{written}"
    );
}

#[test]
fn a_reported_silence_is_written_down_rather_than_only_shown() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();

    report_a_silence(
        &report.shared,
        "installing cargo-llvm-cov",
        Duration::from_secs(630),
    );

    let written = fs::read_to_string(report.log_path()).unwrap();
    assert!(written.contains("installing cargo-llvm-cov"), "{written}");
}

#[test]
fn reporting_a_silence_names_the_log_the_missing_output_is_going_to() {
    let message = silence_message(
        "installing cargo-llvm-cov",
        Duration::from_secs(630),
        Path::new("/home/alice/.dotfiles_configurator/logs/apply-20260807-141233.000-91.log"),
    );

    assert!(message.contains("installing cargo-llvm-cov"), "{message}");
    assert!(message.contains("10 minutes"), "{message}");
    assert!(
        message.contains("apply-20260807-141233.000-91.log"),
        "{message}"
    );
}

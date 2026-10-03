use {
    crate::TOOL_DIRECTORY,
    anyhow::{Context, Result},
    chrono::Local,
    entries::Presentation,
    indicatif::{MultiProgress, ProgressBar, ProgressStyle},
    std::{
        collections::BTreeMap,
        env,
        fmt::Display,
        fs::{self, File, OpenOptions},
        io::Write,
        path::{Path, PathBuf},
        process,
        sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
        thread::{self, JoinHandle},
        time::{Duration, Instant, SystemTime},
    },
};

mod entries;

pub use entries::{Closing, Entry, EntryOutcome, Screen};

tokio::task_local! {
    static SPEAKING_FOR: Entry;
}

pub fn entry_of_this_task() -> Option<Entry> {
    SPEAKING_FOR.try_with(Entry::clone).ok()
}

// ADR 0013
const SILENCE_THRESHOLD: Duration = Duration::from_secs(600); // 10 minutes

const RETAINED_RUNS: usize = 20;

const SILENCE_POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Plan,
    Apply,
}

impl RunKind {
    fn as_file_prefix(self) -> &'static str {
        match self {
            RunKind::Plan => "plan",
            RunKind::Apply => "apply",
        }
    }
}

impl Display for RunKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_file_prefix())
    }
}

pub struct RunReport {
    shared: Arc<Shared>,
    _watchdog: Option<SilenceWatchdog>,
}

impl std::fmt::Debug for RunReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunReport")
            .field("log_path", &self.log_path())
            .finish()
    }
}

struct Shared {
    log: Mutex<LogFile>,
    progress: MultiProgress,
    presentation: Mutex<Presentation>,
    speakers: Mutex<BTreeMap<Speaker, Activity>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Speaker {
    Run,
    Entry(Entry),
}

impl Speaker {
    fn of(entry: Option<&Entry>) -> Self {
        match entry {
            Some(entry) => Speaker::Entry(entry.clone()),
            None => Speaker::Run,
        }
    }
}

struct LogFile {
    path: PathBuf,
    file: File,
}

struct Activity {
    label: String,
    last_spoke: Instant,
    silence_already_reported: bool,
}

impl Activity {
    fn named(label: String) -> Self {
        Self {
            label,
            last_spoke: Instant::now(),
            silence_already_reported: false,
        }
    }
}

struct SilenceWatchdog {
    end_of_run: Arc<EndOfRun>,
    thread: Option<JoinHandle<()>>,
}

struct EndOfRun {
    reached: Mutex<bool>,
    announced: Condvar,
}

impl EndOfRun {
    fn was_reached_within(&self, interval: Duration) -> bool {
        let Ok(reached) = self.reached.lock() else {
            return true;
        };
        match self
            .announced
            .wait_timeout_while(reached, interval, |reached| !*reached)
        {
            Ok((reached, _)) => *reached,
            Err(_) => true,
        }
    }

    fn reach(&self) {
        if let Ok(mut reached) = self.reached.lock() {
            *reached = true;
        }
        self.announced.notify_all();
    }
}

impl Drop for SilenceWatchdog {
    fn drop(&mut self) {
        self.end_of_run.reach();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl RunReport {
    pub fn open(kind: RunKind) -> Result<Self> {
        Self::open_in(&log_directory()?, kind)
    }

    pub fn open_in(directory: &Path, kind: RunKind) -> Result<Self> {
        Self::open_showing(directory, kind, Screen::of_this_process())
    }

    pub fn open_showing(directory: &Path, kind: RunKind, screen: Screen) -> Result<Self> {
        fs::create_dir_all(directory)
            .with_context(|| format!("Could not create {}", directory.display()))?;
        discard_all_but_newest(directory, RETAINED_RUNS.saturating_sub(1))?;

        let (path, file) = create_log(directory, kind)?;
        let report = Self::new(LogFile { path, file }, screen);
        report.note(&format!("{kind} started"));
        Ok(report)
    }

    fn new(log: LogFile, screen: Screen) -> Self {
        let progress = MultiProgress::with_draw_target(screen.draw_target());
        let shared = Arc::new(Shared {
            log: Mutex::new(log),
            presentation: Mutex::new(Presentation::new(screen, progress.clone(), Instant::now())),
            progress,
            speakers: Mutex::new(BTreeMap::new()),
        });

        Self {
            _watchdog: Some(watch_for_silence(Arc::clone(&shared))),
            shared,
        }
    }

    pub fn log_path(&self) -> PathBuf {
        self.shared.log_path()
    }

    pub fn doing(&self, activity: impl Display) -> Doing<'_> {
        let label = activity.to_string();
        self.note(&label);

        let bar = self
            .shared
            .progress
            .add(ProgressBar::new_spinner().with_message(label.clone()));
        bar.set_style(
            ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        bar.enable_steady_tick(Duration::from_millis(120));

        self.shared.listening_to(Speaker::Run, label);

        Doing { report: self, bar }
    }

    pub async fn converging<Output>(
        &self,
        entry: &Entry,
        work: impl Future<Output = Output>,
    ) -> Output {
        let state_line = self.shared.presenting().started(entry, Instant::now());
        self.note(&state_line);
        self.shared
            .listening_to(Speaker::Entry(entry.clone()), entry.to_string());

        SPEAKING_FOR.scope(entry.clone(), work).await
    }

    pub fn entry_finished(&self, entry: &Entry, outcome: EntryOutcome) {
        self.shared
            .no_longer_listening_to(&Speaker::Entry(entry.clone()));
        let state_line = self
            .shared
            .presenting()
            .finished(entry, outcome, Instant::now());
        self.note(&state_line);
    }

    pub fn conclude(&self, closing: Closing) {
        let written_down = self.shared.presenting().concluded(closing, Instant::now());
        for line in written_down {
            self.note(&line);
        }
    }

    pub fn child_line(&self, speaking_for: Option<&Entry>, line: &str) {
        self.shared.heard_from(&Speaker::of(speaking_for));
        let Some(entry) = speaking_for else {
            self.note(line);
            self.shared.show(line);
            return;
        };

        self.note(&format!("{entry}: {line}"));
        self.shared.presenting().spoke(entry, line);
    }

    pub fn captured_output(&self, text: &str) {
        let speaking_for = entry_of_this_task();
        self.shared.heard_from(&Speaker::of(speaking_for.as_ref()));
        for line in text.lines() {
            match &speaking_for {
                Some(entry) => {
                    self.note(&format!("{entry}: {line}"));
                    self.shared.presenting().spoke(entry, line);
                }
                None => self.note(line),
            }
        }
    }

    pub fn progress_bar(&self, total: Option<u64>, message: String) -> ProgressBar {
        let (bar, style) = match total {
            Some(total) => (
                ProgressBar::new(total),
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} {msg}",
            ),
            None => (
                ProgressBar::no_length(),
                "{spinner:.green} [{elapsed_precise}] {bytes} {msg}",
            ),
        };

        self.note(&message);
        self.shared.progress.add(
            bar.with_message(message).with_style(
                ProgressStyle::with_template(style)
                    .unwrap_or_else(|_| ProgressStyle::default_bar())
                    .progress_chars("=> "),
            ),
        )
    }

    pub fn announce(&self, message: &str) {
        self.note(message);
        self.shared.show(message);
    }

    pub fn note(&self, message: &str) {
        self.shared.write_down(message);
    }
}

impl Shared {
    fn presenting(&self) -> MutexGuard<'_, Presentation> {
        self.presentation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn listening(&self) -> MutexGuard<'_, BTreeMap<Speaker, Activity>> {
        self.speakers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn listening_to(&self, speaker: Speaker, label: String) {
        self.listening().insert(speaker, Activity::named(label));
    }

    fn no_longer_listening_to(&self, speaker: &Speaker) {
        self.listening().remove(speaker);
    }

    fn heard_from(&self, speaker: &Speaker) {
        if let Some(activity) = self.listening().get_mut(speaker) {
            activity.last_spoke = Instant::now();
            activity.silence_already_reported = false;
        }
    }

    fn fallen_silent(&self) -> Vec<(String, Duration)> {
        let mut silent = Vec::new();
        for activity in self.listening().values_mut() {
            let silence = activity.last_spoke.elapsed();
            if activity.silence_already_reported || silence < SILENCE_THRESHOLD {
                continue;
            }
            activity.silence_already_reported = true;
            silent.push((activity.label.clone(), silence));
        }
        silent
    }

    fn show(&self, message: &str) {
        self.presenting().show(message);
    }

    fn write_down(&self, message: &str) {
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);

        let _ = writeln!(
            log.file,
            "{} {message}",
            Local::now().format("%H:%M:%S%.3f")
        );
    }

    fn log_path(&self) -> PathBuf {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .path
            .clone()
    }
}

pub struct Doing<'report> {
    report: &'report RunReport,
    bar: ProgressBar,
}

impl Drop for Doing<'_> {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
        self.report.shared.progress.remove(&self.bar);
        self.report.shared.no_longer_listening_to(&Speaker::Run);
    }
}

fn watch_for_silence(shared: Arc<Shared>) -> SilenceWatchdog {
    let end_of_run = Arc::new(EndOfRun {
        reached: Mutex::new(false),
        announced: Condvar::new(),
    });
    let watched_for = Arc::clone(&end_of_run);

    let thread = thread::spawn(move || {
        while !watched_for.was_reached_within(SILENCE_POLL_INTERVAL) {
            for (label, silence) in shared.fallen_silent() {
                report_a_silence(&shared, &label, silence);
            }
        }
    });

    SilenceWatchdog {
        end_of_run,
        thread: Some(thread),
    }
}

fn report_a_silence(shared: &Shared, label: &str, silence: Duration) {
    let message = silence_message(label, silence, &shared.log_path());
    shared.write_down(&message);
    shared.show(&message);
}

fn silence_message(label: &str, silence: Duration, log_path: &Path) -> String {
    let minutes = silence.as_secs() / 60;
    format!(
        "{label} has said nothing for {minutes} minutes. Still waiting; its output is going to {}",
        log_path.display()
    )
}

fn create_log(directory: &Path, kind: RunKind) -> Result<(PathBuf, File)> {
    let started = Local::now().format("%Y%m%d-%H%M%S%.3f");
    let process_id = process::id();
    let prefix = kind.as_file_prefix();

    for attempt in 0.. {
        let name = match attempt {
            0 => format!("{prefix}-{started}-{process_id}.log"),
            _ => format!("{prefix}-{started}-{process_id}-{attempt}.log"),
        };
        let path = directory.join(name);

        match OpenOptions::new().create_new(true).append(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("Could not create {}", path.display()));
            }
        }
    }

    unreachable!("a free name is always reached")
}

fn is_a_run_log(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "log")
}

fn discard_all_but_newest(directory: &Path, keep: usize) -> Result<()> {
    let entries = fs::read_dir(directory)
        .with_context(|| format!("Could not read {}", directory.display()))?;

    let mut logs: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| is_a_run_log(path))
        .filter_map(|path| {
            let modified = path
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()?;
            Some((modified, path))
        })
        .collect();

    logs.sort_by(|left, right| right.cmp(left));

    for (_, path) in logs.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn log_directory() -> Result<PathBuf> {
    Ok(env::home_dir()
        .context("Could not find the home directory to write a run log into")?
        .join(TOOL_DIRECTORY)
        .join("logs"))
}

#[cfg(test)]
mod tests {
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
                source: CargoSource::Registry { version: None },
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
}

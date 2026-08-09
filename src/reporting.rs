use {
    crate::TOOL_DIRECTORY,
    anyhow::{Context, Result},
    chrono::Local,
    indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle},
    std::{
        env,
        fmt::Display,
        fs::{self, File, OpenOptions},
        io::{IsTerminal, Write},
        path::{Path, PathBuf},
        process,
        sync::{Arc, Condvar, Mutex},
        thread::{self, JoinHandle},
        time::{Duration, Instant, SystemTime},
    },
};

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
    log: Option<Mutex<LogFile>>,
    progress: MultiProgress,
    screen: Screen,
    current_activity: Mutex<Option<Activity>>,
}

/// Indicatif draws nothing at all where stderr is not a terminal, so a run under a git trigger
/// would otherwise be as mute as the one that motivated ADR 0013.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Terminal,
    PlainLines,
    Nothing,
}

impl Screen {
    fn of_this_process() -> Self {
        match std::io::stderr().is_terminal() {
            true => Screen::Terminal,
            false => Screen::PlainLines,
        }
    }

    fn draw_target(self) -> ProgressDrawTarget {
        match self {
            Screen::Terminal => ProgressDrawTarget::stderr(),
            Screen::PlainLines | Screen::Nothing => ProgressDrawTarget::hidden(),
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
        fs::create_dir_all(directory)
            .with_context(|| format!("Could not create {}", directory.display()))?;
        discard_all_but_newest(directory, RETAINED_RUNS.saturating_sub(1))?;

        let (path, file) = create_log(directory, kind)?;
        let report = Self::new(Some(LogFile { path, file }), Screen::of_this_process());
        report.note(&format!("{kind} started"));
        Ok(report)
    }

    pub fn discarded() -> Self {
        Self::new(None, Screen::Nothing)
    }

    fn new(log: Option<LogFile>, screen: Screen) -> Self {
        let shared = Arc::new(Shared {
            log: log.map(Mutex::new),
            progress: MultiProgress::with_draw_target(screen.draw_target()),
            screen,
            current_activity: Mutex::new(None),
        });

        Self {
            _watchdog: Some(watch_for_silence(Arc::clone(&shared))),
            shared,
        }
    }

    pub fn log_path(&self) -> Option<PathBuf> {
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

        if let Ok(mut current) = self.shared.current_activity.lock() {
            *current = Some(Activity {
                label,
                last_spoke: Instant::now(),
                silence_already_reported: false,
            });
        }

        Doing { report: self, bar }
    }

    pub fn child_line(&self, line: &str) {
        self.restart_the_silence_clock();
        self.note(line);
        self.shared.show(line);
    }

    pub fn captured_output(&self, text: &str) {
        self.restart_the_silence_clock();
        for line in text.lines() {
            self.note(line);
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

    fn restart_the_silence_clock(&self) {
        if let Ok(mut current) = self.shared.current_activity.lock()
            && let Some(activity) = current.as_mut()
        {
            activity.last_spoke = Instant::now();
            activity.silence_already_reported = false;
        }
    }

    fn finished_the_current_activity(&self) {
        if let Ok(mut current) = self.shared.current_activity.lock() {
            *current = None;
        }
    }
}

impl Shared {
    fn show(&self, message: &str) {
        match self.screen {
            Screen::Terminal => {
                let _ = self.progress.println(message);
            }
            Screen::PlainLines => eprintln!("{message}"),
            Screen::Nothing => {}
        }
    }

    fn write_down(&self, message: &str) {
        let Some(log) = self.log.as_ref() else {
            return;
        };
        let Ok(mut log) = log.lock() else {
            return;
        };

        let _ = writeln!(
            log.file,
            "{} {message}",
            Local::now().format("%H:%M:%S%.3f")
        );
    }

    fn log_path(&self) -> Option<PathBuf> {
        self.log
            .as_ref()
            .and_then(|log| log.lock().ok().map(|log| log.path.clone()))
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
        self.report.finished_the_current_activity();
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
            let Ok(mut current) = shared.current_activity.lock() else {
                continue;
            };
            let Some(activity) = current.as_mut() else {
                continue;
            };

            let silence = activity.last_spoke.elapsed();
            if activity.silence_already_reported || silence < SILENCE_THRESHOLD {
                continue;
            }
            activity.silence_already_reported = true;

            report_a_silence(&shared, &activity.label, silence);
        }
    });

    SilenceWatchdog {
        end_of_run,
        thread: Some(thread),
    }
}

fn report_a_silence(shared: &Shared, label: &str, silence: Duration) {
    let log_path = shared.log_path();
    let message = silence_message(label, silence, log_path.as_deref());
    shared.write_down(&message);
    shared.show(&message);
}

fn silence_message(label: &str, silence: Duration, log_path: Option<&Path>) -> String {
    let minutes = silence.as_secs() / 60;
    match log_path {
        Some(path) => format!(
            "{label} has said nothing for {minutes} minutes. Still waiting; its output is going to {}",
            path.display()
        ),
        None => format!("{label} has said nothing for {minutes} minutes. Still waiting"),
    }
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
    use super::*;

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
        let written = fs::read_to_string(report.log_path().unwrap()).unwrap();

        assert!(written.contains("installing Neovim"), "{written}");
    }

    #[test]
    fn output_captured_for_parsing_rather_than_shown_still_reaches_the_log() {
        let directory = tempfile::tempdir().unwrap();
        let report = RunReport::open_in(directory.path(), RunKind::Plan).unwrap();

        report.captured_output("Name  Id  Version\nA package  Microsoft.PowerShell  1.0.0");
        let written = fs::read_to_string(report.log_path().unwrap()).unwrap();

        assert!(written.contains("Microsoft.PowerShell"), "{written}");
    }

    #[test]
    fn a_discarded_report_writes_no_log_at_all() {
        let report = RunReport::discarded();

        report.announce("installing Neovim");

        assert_eq!(report.log_path(), None);
    }

    #[test]
    fn a_real_run_speaks_whether_or_not_it_has_a_terminal_to_speak_to() {
        assert_ne!(Screen::of_this_process(), Screen::Nothing);
    }

    #[test]
    fn a_run_without_a_terminal_draws_no_progress_bars() {
        assert!(Screen::PlainLines.draw_target().is_hidden());
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

        let written = fs::read_to_string(report.log_path().unwrap()).unwrap();
        assert!(written.contains("installing cargo-llvm-cov"), "{written}");
    }

    #[test]
    fn reporting_a_silence_names_the_log_the_missing_output_is_going_to() {
        let message = silence_message(
            "installing cargo-llvm-cov",
            Duration::from_secs(630),
            Some(Path::new(
                "/home/alice/.dotfiles_configurator/logs/apply-20260807-141233.000-91.log",
            )),
        );

        assert!(message.contains("installing cargo-llvm-cov"), "{message}");
        assert!(message.contains("10 minutes"), "{message}");
        assert!(
            message.contains("apply-20260807-141233.000-91.log"),
            "{message}"
        );
    }
}

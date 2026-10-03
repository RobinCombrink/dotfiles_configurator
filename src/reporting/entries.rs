use {
    crate::{
        configuration::{Resource, ResourceKind},
        convergence::Lane,
    },
    indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle},
    std::{
        fmt::{self, Display},
        io::{self, Write},
        time::{Duration, Instant},
    },
};

const SPINNER_TICK: Duration = Duration::from_millis(120);

const LANE_COLUMN: usize = 12;
const KIND_COLUMN: usize = 20;
const NAME_COLUMN: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry {
    lane: Lane,
    kind: ResourceKind,
    name: String,
}

impl Entry {
    pub fn new(lane: Lane, resource: &Resource) -> Self {
        Self {
            lane,
            kind: resource.kind(),
            name: resource.name(),
        }
    }

    fn on_the_table(&self) -> String {
        format!(
            "{:<LANE_COLUMN$} {:<KIND_COLUMN$} {:<NAME_COLUMN$}",
            self.lane.to_string(),
            self.kind.to_string(),
            self.name
        )
    }

    fn verb(&self) -> Verb {
        Verb::of(self.kind)
    }
}

impl Display for Entry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "[{}] {}", self.lane, self.name)
    }
}

#[derive(Debug, Clone, Copy)]
struct Verb {
    present: &'static str,
    past: &'static str,
}

impl Verb {
    fn of(kind: ResourceKind) -> Self {
        let (present, past) = match kind {
            ResourceKind::Repository => ("cloning", "cloned"),
            ResourceKind::Application | ResourceKind::Package => ("installing", "installed"),
            ResourceKind::EnvironmentVariable => ("setting", "set"),
            ResourceKind::Symlink => ("linking", "linked"),
            ResourceKind::Registration => ("registering", "registered"),
            ResourceKind::Command => ("running", "ran"),
        };
        Self { present, past }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    Running { elapsed: Duration },
    Converged { took: Duration },
    Failed,
    Held,
}

impl EntryStatus {
    fn as_a_line(self, verb: Verb) -> String {
        match self {
            EntryStatus::Running { .. } => verb.present.to_owned(),
            EntryStatus::Converged { took } => format!("{} {}", verb.past, Elapsed(took)),
            EntryStatus::Failed => "failed".to_owned(),
            EntryStatus::Held => "held".to_owned(),
        }
    }

    fn on_the_table(self, verb: Verb) -> String {
        match self {
            EntryStatus::Running { elapsed } => format!("{}… {}", verb.present, Elapsed(elapsed)),
            EntryStatus::Converged { took } => format!("{} {}", verb.past, Elapsed(took)),
            EntryStatus::Failed => "failed".to_owned(),
            EntryStatus::Held => "held".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryOutcome {
    Converged,
    Failed,
    Held,
}

struct Elapsed(Duration);

impl Display for Elapsed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds = self.0.as_secs();
        let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
        match (hours, minutes) {
            (0, 0) => write!(formatter, "{seconds}s"),
            (0, _) => write!(formatter, "{minutes}m{seconds}s"),
            _ => write!(formatter, "{hours}h{minutes}m{seconds}s"),
        }
    }
}

struct Row {
    entry: Entry,
    began: Instant,
    output: Vec<String>,
    ending: Option<(EntryOutcome, Instant)>,
}

impl Row {
    fn status(&self, now: Instant) -> EntryStatus {
        match self.ending {
            None => EntryStatus::Running {
                elapsed: now.saturating_duration_since(self.began),
            },
            Some((EntryOutcome::Converged, ended)) => EntryStatus::Converged {
                took: ended.saturating_duration_since(self.began),
            },
            Some((EntryOutcome::Failed, _)) => EntryStatus::Failed,
            Some((EntryOutcome::Held, _)) => EntryStatus::Held,
        }
    }

    fn state_line(&self, now: Instant) -> String {
        format!(
            "{}: {}",
            self.entry,
            self.status(now).as_a_line(self.entry.verb())
        )
    }

    fn calls_for_diagnosis(&self) -> bool {
        match self.ending {
            Some((EntryOutcome::Failed | EntryOutcome::Held, _)) => !self.output.is_empty(),
            Some((EntryOutcome::Converged, _)) | None => false,
        }
    }
}

struct Board {
    began: Instant,
    rows: Vec<Row>,
}

impl Board {
    fn running(&self, entry: &Entry) -> Option<usize> {
        self.rows
            .iter()
            .rposition(|row| row.entry == *entry && row.ending.is_none())
    }

    fn started(&mut self, entry: &Entry, at: Instant) -> &Row {
        self.rows.push(Row {
            entry: entry.clone(),
            began: at,
            output: Vec::new(),
            ending: None,
        });
        &self.rows[self.rows.len() - 1]
    }

    fn spoke(&mut self, entry: &Entry, line: &str) {
        if let Some(position) = self.running(entry) {
            self.rows[position].output.push(line.to_owned());
        }
    }

    fn finished(&mut self, entry: &Entry, outcome: EntryOutcome, at: Instant) -> &Row {
        let position = match self.running(entry) {
            Some(position) => position,
            None => {
                self.started(entry, at);
                self.rows.len() - 1
            }
        };
        let row = &mut self.rows[position];
        row.ending = Some((outcome, at));
        if outcome == EntryOutcome::Converged {
            row.output = Vec::new();
        }
        row
    }

    fn tally(&self, now: Instant) -> Tally {
        let count = |wanted: EntryOutcome| {
            self.rows
                .iter()
                .filter(|row| row.ending.is_some_and(|(outcome, _)| outcome == wanted))
                .count()
        };
        Tally {
            converged: count(EntryOutcome::Converged),
            failed: count(EntryOutcome::Failed),
            held: count(EntryOutcome::Held),
            duration: now.saturating_duration_since(self.began),
        }
    }

    fn diagnostics(&self, now: Instant) -> Option<String> {
        let diagnosed: Vec<&Row> = self
            .rows
            .iter()
            .filter(|row| row.calls_for_diagnosis())
            .collect();
        if diagnosed.is_empty() {
            return None;
        }

        let mut written = String::from("diagnostics");
        for row in diagnosed {
            written.push('\n');
            written.push_str(&row.state_line(now));
            for line in &row.output {
                written.push_str("\n  ");
                written.push_str(line);
            }
        }
        Some(written)
    }
}

struct Tally {
    converged: usize,
    failed: usize,
    held: usize,
    duration: Duration,
}

impl Display for Tally {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} converged, {} failed, {} held in {}",
            self.converged,
            self.failed,
            self.held,
            Elapsed(self.duration)
        )
    }
}

// 2026-08-07: indicatif draws nothing at all where standard error is not a terminal.
// ADR 0013
pub enum Screen {
    Terminal,
    Lines(Box<dyn Write + Send>),
}

impl fmt::Debug for Screen {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Screen::Terminal => formatter.write_str("Terminal"),
            Screen::Lines(_) => formatter.write_str("Lines"),
        }
    }
}

impl Screen {
    pub fn of_this_process() -> Self {
        match io::IsTerminal::is_terminal(&io::stderr()) {
            true => Screen::Terminal,
            false => Screen::Lines(Box::new(StandardError)),
        }
    }

    pub(super) fn draw_target(&self) -> ProgressDrawTarget {
        match self {
            Screen::Terminal => ProgressDrawTarget::stderr(),
            Screen::Lines(_) => ProgressDrawTarget::hidden(),
        }
    }
}

struct StandardError;

impl Write for StandardError {
    fn write(&mut self, written: &[u8]) -> io::Result<usize> {
        eprint!("{}", String::from_utf8_lossy(written));
        Ok(written.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) struct Presentation {
    screen: Screen,
    progress: MultiProgress,
    board: Board,
    table: Vec<(Entry, ProgressBar)>,
    concluded: bool,
}

impl Presentation {
    pub(super) fn new(screen: Screen, progress: MultiProgress, began: Instant) -> Self {
        Self {
            screen,
            progress,
            board: Board {
                began,
                rows: Vec::new(),
            },
            table: Vec::new(),
            concluded: false,
        }
    }

    pub(super) fn show(&mut self, message: &str) {
        match &mut self.screen {
            Screen::Lines(output) => {
                let _ = writeln!(output, "{message}");
            }
            Screen::Terminal if self.concluded => eprintln!("{message}"),
            Screen::Terminal => {
                let _ = self.progress.println(message);
            }
        }
    }

    pub(super) fn started(&mut self, entry: &Entry, at: Instant) -> String {
        let state_line = self.board.started(entry, at).state_line(at);
        match &mut self.screen {
            Screen::Lines(output) => {
                let _ = writeln!(output, "{state_line}");
            }
            Screen::Terminal => {
                let row = self.progress.add(running_row(entry, at));
                self.table.push((entry.clone(), row));
            }
        }
        state_line
    }

    pub(super) fn spoke(&mut self, entry: &Entry, line: &str) {
        self.board.spoke(entry, line);
    }

    pub(super) fn finished(&mut self, entry: &Entry, outcome: EntryOutcome, at: Instant) -> String {
        let row = self.board.finished(entry, outcome, at);
        let state_line = row.state_line(at);
        let on_the_table = row.status(at).on_the_table(entry.verb());

        match &mut self.screen {
            Screen::Lines(output) => {
                let _ = writeln!(output, "{state_line}");
            }
            Screen::Terminal => {
                let position = self
                    .table
                    .iter()
                    .rposition(|(drawn, row)| drawn == entry && !row.is_finished());
                let row = match position {
                    Some(position) => self.table[position].1.clone(),
                    None => {
                        let row = self.progress.add(ProgressBar::new_spinner());
                        self.table.push((entry.clone(), row.clone()));
                        row
                    }
                };
                row.set_style(finished_row_style());
                row.set_prefix(entry.on_the_table());
                row.finish_with_message(on_the_table);
            }
        }
        state_line
    }

    pub(super) fn concluded(&mut self, at: Instant) -> Option<String> {
        if self.board.rows.is_empty() || self.concluded {
            return None;
        }

        let tally = self.board.tally(at).to_string();
        let diagnostics = self.board.diagnostics(at);
        let written = match (&self.screen, diagnostics) {
            (_, None) => tally,
            (Screen::Terminal, Some(diagnostics)) => format!("{tally}\n{diagnostics}"),
            (Screen::Lines(_), Some(diagnostics)) => format!("{diagnostics}\n{tally}"),
        };

        if let Screen::Terminal = self.screen {
            self.progress.set_draw_target(ProgressDrawTarget::hidden());
        }
        self.concluded = true;
        self.show(&written);
        Some(written)
    }
}

fn running_row(entry: &Entry, began: Instant) -> ProgressBar {
    let verb = entry.verb();
    let style = ProgressStyle::with_template("{spinner:.green} {prefix} {status}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
        .with_key(
            "status",
            move |_: &ProgressState, written: &mut dyn fmt::Write| {
                let status = EntryStatus::Running {
                    elapsed: began.elapsed(),
                };
                let _ = written.write_str(&status.on_the_table(verb));
            },
        );

    let row = ProgressBar::new_spinner()
        .with_style(style)
        .with_prefix(entry.on_the_table());
    row.enable_steady_tick(SPINNER_TICK);
    row
}

fn finished_row_style() -> ProgressStyle {
    ProgressStyle::with_template("  {prefix} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_bar())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::configuration::{
            CargoPackage, CargoSource, CrateName, Package, WingetPackage, WingetPackageId,
        },
        std::sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct Transcript(Arc<Mutex<Vec<u8>>>);

    impl Write for Transcript {
        fn write(&mut self, written: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(written);
            Ok(written.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Transcript {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn cargo_package(name: &str) -> Resource {
        Resource::Package(Package::Cargo(CargoPackage {
            crate_name: CrateName::from(name),
            source: CargoSource::Registry { version: None },
        }))
    }

    fn winget_package(id: &str) -> Resource {
        Resource::Package(Package::Winget(WingetPackage {
            id: WingetPackageId::from(id),
        }))
    }

    fn written_without_a_terminal(transcript: &Transcript, began: Instant) -> Presentation {
        Presentation::new(
            Screen::Lines(Box::new(transcript.clone())),
            MultiProgress::with_draw_target(ProgressDrawTarget::hidden()),
            began,
        )
    }

    #[test]
    fn without_a_terminal_each_state_change_is_a_line_then_the_diagnostics_then_the_tally() {
        let transcript = Transcript::default();
        let began = Instant::now();
        let at = |seconds: u64| began + Duration::from_secs(seconds);
        let mut presentation = written_without_a_terminal(&transcript, began);
        let stop_gate = Entry::new(Lane::Cargo, &cargo_package("stop-gate"));
        let neovim = Entry::new(Lane::Install, &winget_package("Neovim.Neovim"));
        let ripgrep = Entry::new(Lane::Cargo, &cargo_package("ripgrep"));

        presentation.started(&stop_gate, at(0));
        presentation.spoke(&stop_gate, "Compiling stop-gate v0.1.0");
        presentation.finished(&stop_gate, EntryOutcome::Converged, at(41));
        presentation.started(&neovim, at(41));
        presentation.spoke(&neovim, "Installer failed with exit code: 1603");
        presentation.finished(&neovim, EntryOutcome::Failed, at(60));
        presentation.started(&ripgrep, at(60));
        presentation.spoke(&ripgrep, "Replacing rg.exe");
        presentation.spoke(&ripgrep, "rg.exe is being executed");
        presentation.finished(&ripgrep, EntryOutcome::Held, at(100));
        presentation.concluded(at(100));

        assert_eq!(
            transcript.text(),
            "[cargo] stop-gate: installing\n\
             [cargo] stop-gate: installed 41s\n\
             [install] Neovim.Neovim: installing\n\
             [install] Neovim.Neovim: failed\n\
             [cargo] ripgrep: installing\n\
             [cargo] ripgrep: held\n\
             diagnostics\n\
             [install] Neovim.Neovim: failed\n\
             \x20 Installer failed with exit code: 1603\n\
             [cargo] ripgrep: held\n\
             \x20 Replacing rg.exe\n\
             \x20 rg.exe is being executed\n\
             1 converged, 1 failed, 1 held in 1m40s\n"
        );
    }

    #[test]
    fn an_entry_that_fails_before_it_starts_is_still_counted_as_failed() {
        let transcript = Transcript::default();
        let began = Instant::now();
        let mut presentation = written_without_a_terminal(&transcript, began);
        let configurator = Entry::new(Lane::Replacement, &cargo_package("dotfiles_configurator"));

        presentation.finished(&configurator, EntryOutcome::Failed, began);
        presentation.concluded(began);

        assert_eq!(
            transcript.text(),
            "[self] dotfiles_configurator: failed\n0 converged, 1 failed, 0 held in 0s\n"
        );
    }

    #[test]
    fn a_run_that_changed_nothing_ends_without_a_tally() {
        let transcript = Transcript::default();
        let began = Instant::now();
        let mut presentation = written_without_a_terminal(&transcript, began);

        presentation.concluded(began);

        assert_eq!(transcript.text(), "");
    }

    #[test]
    fn a_running_row_on_the_table_shows_how_long_it_has_been_running() {
        let verb = Verb::of(ResourceKind::Package);

        assert_eq!(
            EntryStatus::Running {
                elapsed: Duration::from_secs(41)
            }
            .on_the_table(verb),
            "installing… 41s"
        );
    }

    #[test]
    fn a_duration_is_written_in_the_largest_units_it_reaches() {
        assert_eq!(Elapsed(Duration::from_secs(41)).to_string(), "41s");
        assert_eq!(Elapsed(Duration::from_secs(100)).to_string(), "1m40s");
        assert_eq!(Elapsed(Duration::from_secs(3_605)).to_string(), "1h0m5s");
    }

    #[test]
    fn a_run_without_a_terminal_draws_no_progress_bars() {
        assert!(
            Screen::Lines(Box::new(io::sink()))
                .draw_target()
                .is_hidden()
        );
    }
}

use {
    crate::{
        configuration::{Resource, ResourceKind},
        convergence::Lane,
        machine::HeldReason,
    },
    indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle},
    std::{
        fmt::{self, Display},
        io::{self, Write},
        sync::{Arc, Mutex, PoisonError},
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
    subject: Subject,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Subject {
    Resource(ResourceKind),
    WorkspaceBuild { workspace: String },
    WingetSources,
    Removal,
}

impl Display for Subject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Subject::Resource(kind) => Display::fmt(kind, formatter),
            Subject::WorkspaceBuild { .. } => formatter.write_str("workspace build"),
            Subject::WingetSources => formatter.write_str("source update"),
            Subject::Removal => formatter.write_str("removal"),
        }
    }
}

impl Entry {
    pub fn new(lane: Lane, resource: &Resource) -> Self {
        Self {
            lane,
            subject: Subject::Resource(resource.kind()),
            name: resource.name(),
        }
    }

    pub fn workspace_build(workspace: impl Display) -> Self {
        Self {
            lane: Lane::Cargo,
            subject: Subject::WorkspaceBuild {
                workspace: workspace.to_string(),
            },
            name: "build".to_owned(),
        }
    }

    pub fn winget_sources() -> Self {
        Self {
            lane: Lane::Install,
            subject: Subject::WingetSources,
            name: "winget sources".to_owned(),
        }
    }

    // ADR 0041
    pub fn removal(binary: impl Display) -> Self {
        Self {
            lane: Lane::Cargo,
            subject: Subject::Removal,
            name: binary.to_string(),
        }
    }

    fn on_the_table(&self) -> String {
        format!(
            "{:<LANE_COLUMN$} {:<KIND_COLUMN$} {:<NAME_COLUMN$}",
            self.lane.to_string(),
            self.subject.to_string(),
            self.name
        )
    }

    fn verb(&self) -> Verb {
        Verb::of(&self.subject)
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
    fn of(subject: &Subject) -> Self {
        let (present, past) = match subject {
            Subject::Resource(ResourceKind::Repository) => ("cloning", "cloned"),
            Subject::Resource(ResourceKind::Application | ResourceKind::Package) => {
                ("installing", "installed")
            }
            Subject::Resource(ResourceKind::EnvironmentVariable) => ("setting", "set"),
            Subject::Resource(ResourceKind::Symlink) => ("linking", "linked"),
            Subject::Resource(ResourceKind::Registration) => ("registering", "registered"),
            Subject::Resource(ResourceKind::Command) => ("running", "ran"),
            Subject::WorkspaceBuild { .. } => ("building", "built"),
            Subject::WingetSources => ("updating", "updated"),
            Subject::Removal => ("removing", "removed"),
        };
        Self { present, past }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    Running {
        elapsed: Duration,
        advance: Option<Advance>,
    },
    Converged {
        took: Duration,
    },
    Failed,
    Held,
    Blocked,
    DidNotTake,
}

impl EntryStatus {
    fn as_a_line(self, verb: Verb) -> String {
        match self {
            EntryStatus::Running { .. } => verb.present.to_owned(),
            EntryStatus::Converged { took } => format!("{} {}", verb.past, Elapsed(took)),
            EntryStatus::Failed => "failed".to_owned(),
            EntryStatus::Held => "held".to_owned(),
            EntryStatus::Blocked => "blocked".to_owned(),
            EntryStatus::DidNotTake => "did not take".to_owned(),
        }
    }

    fn on_the_table(self, verb: Verb) -> String {
        match self {
            EntryStatus::Running {
                advance:
                    Some(Advance {
                        verb,
                        percent: Some(percent),
                    }),
                ..
            } => format!("{verb} {percent}%"),
            EntryStatus::Running {
                elapsed,
                advance:
                    Some(Advance {
                        verb,
                        percent: None,
                    }),
            } => format!("{verb}… {}", Elapsed(elapsed)),
            EntryStatus::Running {
                elapsed,
                advance: None,
            } => format!("{}… {}", verb.present, Elapsed(elapsed)),
            EntryStatus::Converged { took } => format!("{} {}", verb.past, Elapsed(took)),
            EntryStatus::Failed => "failed".to_owned(),
            EntryStatus::Held => "held".to_owned(),
            EntryStatus::Blocked => "blocked".to_owned(),
            EntryStatus::DidNotTake => "did not take".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryOutcome {
    Converged,
    Failed { reason: String },
    Held { reason: HeldReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ending {
    Converged,
    Failed(String),
    Held(String),
    Blocked(String),
    DidNotTake(String),
}

impl From<EntryOutcome> for Ending {
    fn from(outcome: EntryOutcome) -> Self {
        match outcome {
            EntryOutcome::Converged => Ending::Converged,
            EntryOutcome::Failed { reason } => Ending::Failed(reason),
            EntryOutcome::Held { reason } => Ending::Held(reason.to_string()),
        }
    }
}

impl Ending {
    fn reason(&self) -> Option<&str> {
        match self {
            Ending::Converged => None,
            Ending::Failed(reason)
            | Ending::Held(reason)
            | Ending::Blocked(reason)
            | Ending::DidNotTake(reason) => Some(reason),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Closing {
    pub converged: usize,
    pub failed: usize,
    pub held: usize,
    pub blocked: Vec<(Entry, String)>,
    pub did_not_take: Vec<(Entry, String)>,
    pub removed: usize,
    pub migrated: Vec<String>,
    pub notices: Vec<String>,
    pub passes: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advance {
    verb: &'static str,
    percent: Option<u64>,
}

impl Advance {
    pub fn of(verb: &'static str, position: u64, length: Option<u64>) -> Self {
        let percent = length
            .filter(|length| *length > 0)
            .map(|length| position.min(length) * 100 / length);
        Self { verb, percent }
    }
}

type SharedAdvance = Arc<Mutex<Option<Advance>>>;

fn read_advance(advance: &SharedAdvance) -> Option<Advance> {
    *advance.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Row {
    entry: Entry,
    began: Instant,
    output: Vec<String>,
    advance: SharedAdvance,
    ending: Option<(Ending, Instant)>,
}

impl Row {
    fn status(&self, now: Instant) -> EntryStatus {
        match &self.ending {
            None => EntryStatus::Running {
                elapsed: now.saturating_duration_since(self.began),
                advance: read_advance(&self.advance),
            },
            Some((Ending::Converged, ended)) => EntryStatus::Converged {
                took: ended.saturating_duration_since(self.began),
            },
            Some((Ending::Failed(_), _)) => EntryStatus::Failed,
            Some((Ending::Held(_), _)) => EntryStatus::Held,
            Some((Ending::Blocked(_), _)) => EntryStatus::Blocked,
            Some((Ending::DidNotTake(_), _)) => EntryStatus::DidNotTake,
        }
    }

    fn state_line(&self, now: Instant) -> String {
        format!(
            "{}: {}",
            self.entry,
            self.status(now).as_a_line(self.entry.verb())
        )
    }

    fn reason(&self) -> Option<&str> {
        self.ending.as_ref().and_then(|(ending, _)| ending.reason())
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
            advance: SharedAdvance::default(),
            ending: None,
        });
        &self.rows[self.rows.len() - 1]
    }

    fn spoke(&mut self, entry: &Entry, line: &str) {
        if let Some(position) = self.running(entry) {
            self.rows[position].output.push(line.to_owned());
        }
    }

    fn advanced(&mut self, entry: &Entry, advance: Option<Advance>) {
        if let Some(position) = self.running(entry) {
            *self.rows[position]
                .advance
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = advance;
        }
    }

    fn ended(&mut self, entry: &Entry, ending: Ending, at: Instant) -> &Row {
        let position = match self.running(entry) {
            Some(position) => position,
            None => {
                self.started(entry, at);
                self.rows.len() - 1
            }
        };
        let row = &mut self.rows[position];
        if ending == Ending::Converged {
            row.output = Vec::new();
        }
        row.ending = Some((ending, at));
        row
    }

    fn did_not_take(&mut self, entry: &Entry, reason: String, at: Instant) -> &Row {
        let converged = self
            .rows
            .iter()
            .rposition(|row| row.entry == *entry && matches_converged(row.ending.as_ref()));
        let Some(position) = converged else {
            return self.ended(entry, Ending::DidNotTake(reason), at);
        };

        let row = &mut self.rows[position];
        let ended = row.ending.as_ref().map_or(at, |(_, ended)| *ended);
        row.ending = Some((Ending::DidNotTake(reason), ended));
        row
    }

    fn diagnostics(&self, now: Instant) -> Option<String> {
        let diagnosed: Vec<&Row> = self
            .rows
            .iter()
            .filter(|row| row.reason().is_some())
            .collect();
        if diagnosed.is_empty() {
            return None;
        }

        let mut written = String::from("diagnostics");
        for row in diagnosed {
            written.push('\n');
            written.push_str(&row.state_line(now));
            let reason = row.reason().unwrap_or_default();
            for line in row.output.iter().map(String::as_str).chain(reason.lines()) {
                written.push_str("\n  ");
                written.push_str(line);
            }
        }
        Some(written)
    }
}

fn matches_converged(ending: Option<&(Ending, Instant)>) -> bool {
    match ending {
        Some((Ending::Converged, _)) => true,
        Some((
            Ending::Failed(_) | Ending::Held(_) | Ending::Blocked(_) | Ending::DidNotTake(_),
            _,
        ))
        | None => false,
    }
}

struct Tally<'closing> {
    closing: &'closing Closing,
    duration: Duration,
}

impl Display for Tally<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} converged, {} failed, {} held, {} still blocked, {} did not take, {} removed, \
             {} migrated, over {} pass(es) in {}",
            self.closing.converged,
            self.closing.failed,
            self.closing.held,
            self.closing.blocked.len(),
            self.closing.did_not_take.len(),
            self.closing.removed,
            self.closing.migrated.len(),
            self.closing.passes,
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

    pub(super) fn table_is_live(&self) -> bool {
        match self.screen {
            Screen::Terminal => !self.table.is_empty() && !self.concluded,
            Screen::Lines(_) => false,
        }
    }

    pub(super) fn started(&mut self, entry: &Entry, at: Instant) -> String {
        let row = self.board.started(entry, at);
        let state_line = row.state_line(at);
        let advance = Arc::clone(&row.advance);
        match &mut self.screen {
            Screen::Lines(output) => {
                let _ = writeln!(output, "{state_line}");
            }
            Screen::Terminal => {
                let row = self.progress.add(running_row(entry, at, advance));
                self.table.push((entry.clone(), row));
            }
        }
        state_line
    }

    pub(super) fn spoke(&mut self, entry: &Entry, line: &str) {
        self.board.spoke(entry, line);
    }

    pub(super) fn advanced(&mut self, entry: &Entry, advance: Option<Advance>) {
        self.board.advanced(entry, advance);
    }

    pub(super) fn finished(&mut self, entry: &Entry, outcome: EntryOutcome, at: Instant) -> String {
        self.ended(entry, Ending::from(outcome), at)
    }

    fn ended(&mut self, entry: &Entry, ending: Ending, at: Instant) -> String {
        let row = self.board.ended(entry, ending, at);
        let state_line = row.state_line(at);
        let on_the_table = row.status(at).on_the_table(entry.verb());
        self.redrawn(entry, &state_line, on_the_table);
        state_line
    }

    fn did_not_take(&mut self, entry: &Entry, reason: String, at: Instant) -> String {
        let row = self.board.did_not_take(entry, reason, at);
        let state_line = row.state_line(at);
        let on_the_table = row.status(at).on_the_table(entry.verb());
        self.redrawn(entry, &state_line, on_the_table);
        state_line
    }

    fn redrawn(&mut self, entry: &Entry, state_line: &str, on_the_table: String) {
        match &mut self.screen {
            Screen::Lines(output) => {
                let _ = writeln!(output, "{state_line}");
            }
            Screen::Terminal => {
                let drawn = self
                    .table
                    .iter()
                    .rposition(|(drawn, row)| drawn == entry && !row.is_finished())
                    .or_else(|| self.table.iter().rposition(|(drawn, _)| drawn == entry));
                let row = match drawn {
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
    }

    pub(super) fn concluded(&mut self, closing: Closing, at: Instant) -> Vec<String> {
        if self.concluded {
            return Vec::new();
        }

        let mut written_down = Vec::new();
        for (entry, reason) in &closing.blocked {
            written_down.push(self.ended(entry, Ending::Blocked(reason.clone()), at));
        }
        for (entry, reason) in &closing.did_not_take {
            written_down.push(self.did_not_take(entry, reason.clone(), at));
        }

        let tally = Tally {
            closing: &closing,
            duration: at.saturating_duration_since(self.board.began),
        }
        .to_string();
        let mut afterwards: Vec<String> = closing
            .migrated
            .iter()
            .map(|migration| format!("migrated {migration}"))
            .chain(
                closing
                    .notices
                    .iter()
                    .map(|notice| format!("notice {notice}")),
            )
            .collect();
        written_down.extend(afterwards.iter().cloned());
        written_down.push(tally.clone());

        let diagnostics = self.board.diagnostics(at);
        let shown: Vec<String> = match &self.screen {
            Screen::Lines(_) => {
                let mut shown: Vec<String> = diagnostics.into_iter().collect();
                shown.append(&mut afterwards);
                shown.push(tally);
                shown
            }
            Screen::Terminal => {
                let mut shown = vec![tally];
                shown.append(&mut afterwards);
                shown.extend(diagnostics);
                shown
            }
        };

        if let Screen::Terminal = self.screen {
            self.progress.set_draw_target(ProgressDrawTarget::hidden());
        }
        self.concluded = true;
        self.show(&shown.join("\n"));
        written_down
    }
}

fn running_row(entry: &Entry, began: Instant, advance: SharedAdvance) -> ProgressBar {
    let verb = entry.verb();
    let style = ProgressStyle::with_template("{spinner:.green} {prefix} {status}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
        .with_key(
            "status",
            move |_: &ProgressState, written: &mut dyn fmt::Write| {
                let status = EntryStatus::Running {
                    elapsed: began.elapsed(),
                    advance: read_advance(&advance),
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
mod tests;

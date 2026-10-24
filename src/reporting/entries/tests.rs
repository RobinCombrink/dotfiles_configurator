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
        source: CargoSource::Registry {
            version: crate::configuration::PackageCurrency::Latest,
        },
    }))
}

fn winget_package(id: &str) -> Resource {
    Resource::Package(Package::Winget(WingetPackage {
        id: WingetPackageId::from(id),
        version: crate::configuration::PackageCurrency::Latest,
    }))
}

fn written_without_a_terminal(transcript: &Transcript, began: Instant) -> Presentation {
    Presentation::new(
        Screen::Lines(Box::new(transcript.clone())),
        MultiProgress::with_draw_target(ProgressDrawTarget::hidden()),
        began,
    )
}

fn failed(reason: &str) -> EntryOutcome {
    EntryOutcome::Failed {
        reason: reason.to_owned(),
    }
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
    let coverage = Entry::new(Lane::Cargo, &cargo_package("cargo-llvm-cov"));

    presentation.started(&stop_gate, at(0));
    presentation.spoke(&stop_gate, "Compiling stop-gate v0.1.0");
    presentation.finished(&stop_gate, EntryOutcome::Converged, at(41));
    presentation.started(&neovim, at(41));
    presentation.spoke(&neovim, "Installer failed with exit code: 1603");
    presentation.finished(&neovim, failed("Could not install Neovim.Neovim"), at(60));
    presentation.started(&ripgrep, at(60));
    presentation.spoke(&ripgrep, "Replacing rg.exe");
    presentation.finished(
        &ripgrep,
        EntryOutcome::Held {
            reason: HeldReason::BeingExecuted("rg.exe".into()),
        },
        at(100),
    );
    presentation.concluded(
        Closing {
            converged: 1,
            failed: 1,
            held: 1,
            blocked: vec![(coverage, "cargo is not on the path".to_owned())],
            notices: vec!["open a new shell".to_owned()],
            passes: 2,
            ..Closing::default()
        },
        at(100),
    );

    assert_eq!(
        transcript.text(),
        "[cargo] stop-gate: installing\n\
         [cargo] stop-gate: installed 41s\n\
         [install] Neovim.Neovim: installing\n\
         [install] Neovim.Neovim: failed\n\
         [cargo] ripgrep: installing\n\
         [cargo] ripgrep: held\n\
         [cargo] cargo-llvm-cov: blocked\n\
         diagnostics\n\
         [install] Neovim.Neovim: failed\n\
         \x20 Installer failed with exit code: 1603\n\
         \x20 Could not install Neovim.Neovim\n\
         [cargo] ripgrep: held\n\
         \x20 Replacing rg.exe\n\
         \x20 rg.exe is being executed\n\
         [cargo] cargo-llvm-cov: blocked\n\
         \x20 cargo is not on the path\n\
         notice open a new shell\n\
         1 converged, 1 failed, 1 held, 1 still blocked, 0 still uncloned, 0 did not take, \
         0 removed, \
         0 migrated, over 2 pass(es) in 1m40s\n"
    );
}

#[test]
fn an_entry_that_converged_but_still_reads_as_drifted_is_reported_as_not_having_taken() {
    let transcript = Transcript::default();
    let began = Instant::now();
    let mut presentation = written_without_a_terminal(&transcript, began);
    let stop_gate = Entry::new(Lane::Cargo, &cargo_package("stop-gate"));

    presentation.started(&stop_gate, began);
    presentation.finished(&stop_gate, EntryOutcome::Converged, began);
    presentation.concluded(
        Closing {
            converged: 1,
            did_not_take: vec![(stop_gate, "still at 0.1.0".to_owned())],
            passes: 1,
            ..Closing::default()
        },
        began,
    );

    assert_eq!(
        transcript.text(),
        "[cargo] stop-gate: installing\n\
         [cargo] stop-gate: installed 0s\n\
         [cargo] stop-gate: did not take\n\
         diagnostics\n\
         [cargo] stop-gate: did not take\n\
         \x20 still at 0.1.0\n\
         1 converged, 0 failed, 0 held, 0 still blocked, 0 still uncloned, 1 did not take, \
         0 removed, \
         0 migrated, over 1 pass(es) in 0s\n"
    );
}

#[test]
fn an_entry_that_fails_before_it_starts_is_still_reported_as_failed() {
    let transcript = Transcript::default();
    let began = Instant::now();
    let mut presentation = written_without_a_terminal(&transcript, began);
    let configurator = Entry::new(Lane::Replacement, &cargo_package("dotfiles_configurator"));

    presentation.finished(&configurator, failed("still behind"), began);

    assert_eq!(transcript.text(), "[self] dotfiles_configurator: failed\n");
}

#[test]
fn a_run_that_changed_nothing_still_closes_on_its_tally() {
    let transcript = Transcript::default();
    let began = Instant::now();
    let mut presentation = written_without_a_terminal(&transcript, began);

    presentation.concluded(
        Closing {
            passes: 1,
            ..Closing::default()
        },
        began,
    );

    assert_eq!(
        transcript.text(),
        "0 converged, 0 failed, 0 held, 0 still blocked, 0 still uncloned, 0 did not take, \
         0 removed, \
         0 migrated, over 1 pass(es) in 0s\n"
    );
}

#[test]
fn a_running_row_on_the_table_shows_how_long_it_has_been_running() {
    let verb = Verb::of(&Subject::Resource(ResourceKind::Package));

    assert_eq!(
        EntryStatus::Running {
            elapsed: Duration::from_secs(41),
            advance: None,
        }
        .on_the_table(verb),
        "installing… 41s"
    );
}

#[test]
fn a_row_whose_download_has_a_known_size_shows_how_far_it_has_come() {
    let transcript = Transcript::default();
    let began = Instant::now();
    let mut presentation = written_without_a_terminal(&transcript, began);
    let neovim = Entry::new(Lane::Install, &winget_package("Neovim.Neovim"));

    presentation.started(&neovim, began);
    presentation.advanced(&neovim, Some(Advance::of("downloading", 68, Some(200))));

    let status = presentation.board.rows[0].status(began);
    assert_eq!(status.on_the_table(neovim.verb()), "downloading 34%");
}

#[test]
fn a_row_whose_download_has_finished_goes_back_to_how_long_it_has_been_running() {
    let transcript = Transcript::default();
    let began = Instant::now();
    let mut presentation = written_without_a_terminal(&transcript, began);
    let neovim = Entry::new(Lane::Install, &winget_package("Neovim.Neovim"));

    presentation.started(&neovim, began);
    presentation.advanced(&neovim, Some(Advance::of("downloading", 68, Some(200))));
    presentation.advanced(&neovim, None);

    let status = presentation.board.rows[0].status(began);
    assert_eq!(status.on_the_table(neovim.verb()), "installing… 0s");
}

#[test]
fn a_download_of_unknown_size_shows_no_percentage() {
    assert_eq!(
        Advance::of("cloning", 68, None),
        Advance {
            verb: "cloning",
            percent: None
        }
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

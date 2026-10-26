use {super::*, crate::machine::Exited};

const A_COMMIT: &str = "e54928664eb4222b2ea79c7ef56126c5c58c7e01";

fn printed(standard_output: &str) -> CommandOutput {
    CommandOutput {
        exited: Exited::Code(0),
        standard_output: standard_output.to_owned(),
        standard_error: String::new(),
    }
}

fn stop_gate() -> BinaryName {
    BinaryName::from("stop-gate")
}

#[test]
fn a_binary_naming_itself_and_a_commit_reports_that_commit() {
    assert_eq!(
        stamp_reported(&stop_gate(), &printed(&format!("stop-gate {A_COMMIT}\n"))),
        Stamp::Commit(Revision::from(A_COMMIT))
    );
}

#[test]
fn a_binary_built_under_no_repository_reports_an_unrecorded_build() {
    assert_eq!(
        stamp_reported(&stop_gate(), &printed("stop-gate unrecorded\n")),
        Stamp::Unrecorded
    );
}

fn naming_no_commit(printed: &str) -> Stamp {
    Stamp::Unreadable(UnreadableStamp::NamesNoCommit {
        printed: printed.to_owned(),
    })
}

#[test]
fn a_binary_built_before_the_stamp_reporting_its_package_version_is_unreadable() {
    assert_eq!(
        stamp_reported(&stop_gate(), &printed("stop-gate 0.1.0\n")),
        naming_no_commit("stop-gate 0.1.0")
    );
}

#[test]
fn a_binary_reporting_another_name_is_unreadable_even_beside_a_commit() {
    assert_eq!(
        stamp_reported(&stop_gate(), &printed(&format!("ci-checks {A_COMMIT}\n"))),
        naming_no_commit(&format!("ci-checks {A_COMMIT}"))
    );
}

#[test]
fn a_binary_whose_version_request_fails_is_unreadable() {
    let failed = CommandOutput {
        exited: Exited::Code(2),
        standard_output: format!("stop-gate {A_COMMIT}\n"),
        standard_error: "error: unexpected argument".to_owned(),
    };

    assert_eq!(
        stamp_reported(&stop_gate(), &failed),
        Stamp::Unreadable(UnreadableStamp::RequestFailed(FailedExit {
            exited: Exited::Code(2),
            standard_error: "error: unexpected argument".to_owned(),
        }))
    );
}

#[test]
fn a_commit_spelled_in_capitals_is_not_one_the_stamp_writes() {
    let capitals = format!("stop-gate {}", A_COMMIT.to_uppercase());

    assert_eq!(
        stamp_reported(&stop_gate(), &printed(&format!("{capitals}\n"))),
        naming_no_commit(&capitals)
    );
}

fn judging(own_copy: OwnCopy, resolution: Resolution) -> Assessment {
    judged([(stop_gate(), own_copy, resolution)])
}

#[test]
fn a_current_copy_that_the_search_path_resolves_to_is_converged() {
    assert_eq!(
        judging(OwnCopy::Current, Resolution::Ours),
        Assessment::Converged
    );
}

#[test]
fn a_current_copy_shadowed_by_another_file_is_a_finding_rather_than_drift() {
    assert_eq!(
        judging(
            OwnCopy::Current,
            Resolution::Another(PathBuf::from("C:\\tools\\stop-gate.exe"))
        ),
        Assessment::Found(Finding::from(
            "stop-gate is shadowed by C:\\tools\\stop-gate.exe".to_owned()
        ))
    );
}

#[test]
fn a_stale_copy_shadowed_by_another_file_is_still_installed() {
    assert_eq!(
        judging(
            OwnCopy::Stale(Staleness::Unrecorded),
            Resolution::Another(PathBuf::from("C:\\tools\\stop-gate.exe")),
        ),
        Assessment::Drifted(DriftReason::OwnCopies(vec![BinaryDrift {
            binary: stop_gate(),
            copy: CopyDrift::Stale(Staleness::Unrecorded),
            shadowed_by: Some(PathBuf::from("C:\\tools\\stop-gate.exe")),
        }]))
    );
}

#[test]
fn an_absent_copy_is_installed_and_named_as_absent() {
    assert_eq!(
        judging(OwnCopy::Absent, Resolution::Nothing),
        Assessment::Drifted(DriftReason::OwnCopies(vec![BinaryDrift {
            binary: stop_gate(),
            copy: CopyDrift::Absent,
            shadowed_by: None,
        }]))
    );
}

#[test]
fn a_member_names_every_binary_it_installs_for_with_its_verdict() {
    assert_eq!(
        judged([
            (
                BinaryName::from("sweep"),
                OwnCopy::Stale(Staleness::Unrecorded),
                Resolution::Ours
            ),
            (
                BinaryName::from("reach"),
                OwnCopy::Current,
                Resolution::Ours
            ),
            (
                BinaryName::from("tool-use-statistics"),
                OwnCopy::Absent,
                Resolution::Nothing
            ),
        ]),
        Assessment::Drifted(DriftReason::OwnCopies(vec![
            BinaryDrift {
                binary: BinaryName::from("sweep"),
                copy: CopyDrift::Stale(Staleness::Unrecorded),
                shadowed_by: None,
            },
            BinaryDrift {
                binary: BinaryName::from("tool-use-statistics"),
                copy: CopyDrift::Absent,
                shadowed_by: None,
            },
        ]))
    );
}

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
    let stamp = stamp_reported(&stop_gate(), &printed(&format!("stop-gate {A_COMMIT}\n")));

    let Stamp::Commit(commit) = stamp else {
        panic!("{stamp:?}");
    };
    assert_eq!(commit, Revision::from(A_COMMIT));
}

#[test]
fn a_binary_built_under_no_repository_reports_an_unrecorded_build() {
    let stamp = stamp_reported(&stop_gate(), &printed("stop-gate unrecorded\n"));

    assert!(matches!(stamp, Stamp::Unrecorded), "{stamp:?}");
}

fn printed_naming_no_commit(stamp: Stamp) -> String {
    let Stamp::Unreadable(UnreadableStamp::NamesNoCommit { printed }) = stamp else {
        panic!("{stamp:?}");
    };
    printed
}

#[test]
fn a_binary_built_before_the_stamp_reporting_its_package_version_is_unreadable() {
    let stamp = stamp_reported(&stop_gate(), &printed("stop-gate 0.1.0\n"));

    assert_eq!(printed_naming_no_commit(stamp), "stop-gate 0.1.0");
}

#[test]
fn a_binary_reporting_another_name_is_unreadable_even_beside_a_commit() {
    let stamp = stamp_reported(&stop_gate(), &printed(&format!("ci-checks {A_COMMIT}\n")));

    assert_eq!(
        printed_naming_no_commit(stamp),
        format!("ci-checks {A_COMMIT}")
    );
}

#[test]
fn a_binary_whose_version_request_fails_is_unreadable() {
    let failed = CommandOutput {
        exited: Exited::Code(2),
        standard_output: format!("stop-gate {A_COMMIT}\n"),
        standard_error: "error: unexpected argument".to_owned(),
    };

    let stamp = stamp_reported(&stop_gate(), &failed);

    let Stamp::Unreadable(UnreadableStamp::RequestFailed(failure)) = stamp else {
        panic!("{stamp:?}");
    };
    assert_eq!(
        failure,
        FailedExit {
            exited: Exited::Code(2),
            standard_error: "error: unexpected argument".to_owned(),
        }
    );
}

#[test]
fn a_commit_spelled_in_capitals_is_not_one_the_stamp_writes() {
    let capitals = format!("stop-gate {}", A_COMMIT.to_uppercase());

    let stamp = stamp_reported(&stop_gate(), &printed(&format!("{capitals}\n")));

    assert_eq!(printed_naming_no_commit(stamp), capitals);
}

fn judging(own_copy: OwnCopy, resolution: Resolution) -> Assessment {
    judged([(stop_gate(), own_copy, resolution)])
}

fn binary_drifts(assessment: Assessment) -> Vec<BinaryDrift> {
    let Assessment::Drifted(DriftReason::OwnCopies(drifts)) = assessment else {
        panic!("{assessment:?}");
    };
    drifts
}

#[test]
fn a_current_copy_that_the_search_path_resolves_to_is_converged() {
    let assessment = judging(OwnCopy::Current, Resolution::Ours);

    assert!(
        matches!(assessment, Assessment::Converged),
        "{assessment:?}"
    );
}

#[test]
fn a_current_copy_shadowed_by_another_file_is_a_finding_rather_than_drift() {
    let assessment = judging(
        OwnCopy::Current,
        Resolution::Another(PathBuf::from("C:\\tools\\stop-gate.exe")),
    );

    let Assessment::Found(finding) = assessment else {
        panic!("{assessment:?}");
    };
    assert_eq!(
        finding,
        Finding::OnSearchPath(vec![SearchPathFinding::Shadowed {
            binary: stop_gate(),
            by: PathBuf::from("C:\\tools\\stop-gate.exe"),
        }])
    );
}

#[test]
fn a_current_copy_nothing_on_the_search_path_resolves_is_a_finding_rather_than_drift() {
    let assessment = judging(OwnCopy::Current, Resolution::Nothing);

    let Assessment::Found(finding) = assessment else {
        panic!("{assessment:?}");
    };
    assert_eq!(
        finding,
        Finding::OnSearchPath(vec![SearchPathFinding::Unresolved {
            binary: stop_gate(),
        }])
    );
}

#[test]
fn a_stale_copy_shadowed_by_another_file_is_still_installed() {
    let drifts = binary_drifts(judging(
        OwnCopy::Stale(Staleness::Unrecorded),
        Resolution::Another(PathBuf::from("C:\\tools\\stop-gate.exe")),
    ));

    let [
        BinaryDrift {
            binary,
            copy: CopyDrift::Stale(Staleness::Unrecorded),
            shadowed_by,
        },
    ] = drifts.as_slice()
    else {
        panic!("{drifts:?}");
    };
    assert_eq!(*binary, stop_gate());
    assert_eq!(
        *shadowed_by,
        Some(PathBuf::from("C:\\tools\\stop-gate.exe"))
    );
}

#[test]
fn an_absent_copy_is_installed_and_named_as_absent() {
    let drifts = binary_drifts(judging(OwnCopy::Absent, Resolution::Nothing));

    let [
        BinaryDrift {
            binary,
            copy: CopyDrift::Absent,
            shadowed_by: None,
        },
    ] = drifts.as_slice()
    else {
        panic!("{drifts:?}");
    };
    assert_eq!(*binary, stop_gate());
}

#[test]
fn a_member_names_every_binary_it_installs_for_with_its_verdict() {
    let drifts = binary_drifts(judged([
        (
            BinaryName::from("sweep"),
            OwnCopy::Stale(Staleness::Unrecorded),
            Resolution::Ours,
        ),
        (
            BinaryName::from("reach"),
            OwnCopy::Current,
            Resolution::Ours,
        ),
        (
            BinaryName::from("tool-use-statistics"),
            OwnCopy::Absent,
            Resolution::Nothing,
        ),
    ]));

    let [
        BinaryDrift {
            binary: stale,
            copy: CopyDrift::Stale(Staleness::Unrecorded),
            shadowed_by: None,
        },
        BinaryDrift {
            binary: absent,
            copy: CopyDrift::Absent,
            shadowed_by: None,
        },
    ] = drifts.as_slice()
    else {
        panic!("{drifts:?}");
    };
    assert_eq!(*stale, BinaryName::from("sweep"));
    assert_eq!(*absent, BinaryName::from("tool-use-statistics"));
}

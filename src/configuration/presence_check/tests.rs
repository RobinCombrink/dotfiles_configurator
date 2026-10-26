use super::*;

fn candidates(paths: &[&str]) -> Candidates {
    Candidates::try_from(paths.iter().map(PathBuf::from).collect::<Vec<_>>())
        .expect("at least one candidate")
}

#[test]
fn a_check_reads_as_what_was_not_true_rather_than_as_the_condition_it_tests() {
    let check = PresenceCheck::CommandOnPath {
        command: "git".to_owned(),
    };

    assert_eq!(check.to_string(), "git is not on the path");
}

#[test]
fn a_path_check_with_no_candidates_cannot_be_made() {
    assert!(Candidates::try_from(Vec::new()).is_err());
}

#[test]
fn a_path_check_declaring_no_candidates_is_not_read() {
    let read = serde_json::from_str::<PresenceCheck>(r#"{ "check": "path_exists", "paths": [] }"#);

    assert!(read.is_err());
}

#[test]
fn a_path_check_no_candidate_answered_names_every_candidate() {
    let check = PresenceCheck::PathExists {
        paths: candidates(&["C:/Program Files/Docker", "AppData/Local/Docker"]),
    };

    assert_eq!(
        check.to_string(),
        "none of C:/Program Files/Docker, AppData/Local/Docker exists"
    );
}

#[test]
fn an_answered_path_check_names_the_candidate_that_answered_and_every_one_that_did_not() {
    let check = PresenceCheck::PathExists {
        paths: candidates(&[
            "C:/Program Files/Docker",
            "AppData/Local/Docker",
            "D:/Docker",
        ]),
    };

    assert_eq!(
        check.answered(&Answer::Candidate(PathBuf::from("AppData/Local/Docker"))),
        Some(AnsweringCandidate {
            answering: PathBuf::from("AppData/Local/Docker"),
            unanswered: vec![
                PathBuf::from("C:/Program Files/Docker"),
                PathBuf::from("D:/Docker"),
            ],
        })
    );
}

#[test]
fn a_check_answering_with_itself_names_no_candidate() {
    let check = PresenceCheck::CommandOnPath {
        command: "git".to_owned(),
    };

    assert_eq!(check.answered(&Answer::TheCheckItself), None);
}

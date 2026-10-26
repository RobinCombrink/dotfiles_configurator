use super::*;

fn linking(name: &str) -> ElevatedWork {
    ElevatedWork::Link {
        link_path: PathBuf::from(name),
        target_path: PathBuf::from("dotfiles").join(name),
    }
}

#[test]
fn each_entry_of_a_settled_batch_answers_for_its_own_work() {
    let settled = ElevatedBatch {
        entries: vec![
            Batched {
                work: linking(".gitconfig"),
                outcome: ElevatedOutcome::Failed {
                    reason: "os error 5".to_owned(),
                },
            },
            Batched {
                work: linking(".npmrc"),
                outcome: ElevatedOutcome::Converged,
            },
        ],
    };

    assert_eq!(
        settled.outcome_of(&linking(".npmrc")),
        Some(&ElevatedOutcome::Converged)
    );
}

#[test]
fn work_a_settled_batch_never_held_has_no_outcome_in_it() {
    let settled = ElevatedBatch {
        entries: vec![Batched {
            work: linking(".gitconfig"),
            outcome: ElevatedOutcome::Converged,
        }],
    };

    assert_eq!(settled.outcome_of(&linking(".npmrc")), None);
}

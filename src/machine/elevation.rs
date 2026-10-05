use {
    serde::{Deserialize, Serialize},
    std::{
        fmt::{self, Display},
        path::PathBuf,
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElevatedWork {
    Link {
        link_path: PathBuf,
        target_path: PathBuf,
    },
    Installer {
        installer_path: PathBuf,
    },
}

impl Display for ElevatedWork {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElevatedWork::Link {
                link_path,
                target_path,
            } => write!(
                formatter,
                "link {} to {}",
                link_path.display(),
                target_path.display()
            ),
            ElevatedWork::Installer { installer_path } => {
                write!(formatter, "run {}", installer_path.display())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeRefusal {
    pub work: ElevatedWork,
    pub refusal: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElevatedBatch<Outcome> {
    pub entries: Vec<Batched<Outcome>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batched<Outcome> {
    pub work: ElevatedWork,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElevatedOutcome {
    Converged,
    Failed { reason: String },
}

impl Display for ElevatedOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElevatedOutcome::Converged => formatter.write_str("converged"),
            ElevatedOutcome::Failed { reason } => write!(formatter, "failed: {reason}"),
        }
    }
}

impl ElevatedBatch<()> {
    pub fn of(works: impl IntoIterator<Item = ElevatedWork>) -> Self {
        Self {
            entries: works
                .into_iter()
                .map(|work| Batched { work, outcome: () })
                .collect(),
        }
    }
}

impl ElevatedBatch<ElevatedOutcome> {
    pub fn outcome_of(&self, work: &ElevatedWork) -> Option<&ElevatedOutcome> {
        self.entries
            .iter()
            .find(|batched| batched.work == *work)
            .map(|batched| &batched.outcome)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Elevation {
    Performed(ElevatedBatch<ElevatedOutcome>),
    Declined,
}

#[cfg(test)]
mod tests {
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
}

use {
    crate::configuration::resource::Shell,
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{
        fmt::Display,
        path::{Path, PathBuf},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(
    description = "An author-declared test that establishes whether a resource is already in its \
                   desired state,\n\
                   used where the machine cannot be asked directly.\n\
                   \n\
                   The forms are a fixed set rather than arbitrary shell so that plan's guarantee \
                   is precise: plan\n\
                   cannot change a machine through anything the tool decides, and can change one \
                   only through a\n\
                   check the configuration's author wrote and declared as a check. Two of the \
                   three forms cannot\n\
                   change anything by construction; `CommandOutputContains` is the narrow, \
                   deliberate escape\n\
                   hatch. See ADR 0006."
)]
#[serde(tag = "check", rename_all = "snake_case")]
pub enum PresenceCheck {
    #[schemars(
        description = "Any of the candidate paths exists. Relative paths resolve against the home \
                       directory."
    )]
    PathExists { paths: Candidates },
    #[schemars(description = "A program is resolvable on the machine's search path.")]
    CommandOnPath { command: String },
    #[schemars(
        description = "A declared command's output contains a string. The only form that can run \
                       something the\n\
                       author chose, and so the only one that is not side-effect-free by \
                       construction."
    )]
    CommandOutputContains {
        shell: Shell,
        args: Vec<String>,
        contains: String,
    },
}

// ADR 0032
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(try_from = "Vec<PathBuf>", into = "Vec<PathBuf>")]
#[schemars(
    description = "The locations a path check accepts, at least one. The check is satisfied by \
                   any of them.",
    extend("minItems" = 1)
)]
pub struct Candidates(Vec<PathBuf>);

impl TryFrom<Vec<PathBuf>> for Candidates {
    type Error = String;

    fn try_from(paths: Vec<PathBuf>) -> Result<Self, Self::Error> {
        match paths.is_empty() {
            true => Err("a path check names at least one candidate".to_owned()),
            false => Ok(Self(paths)),
        }
    }
}

impl From<Candidates> for Vec<PathBuf> {
    fn from(candidates: Candidates) -> Self {
        candidates.0
    }
}

impl Candidates {
    pub fn iter(&self) -> impl Iterator<Item = &Path> {
        self.0.iter().map(PathBuf::as_path)
    }

    pub fn first_found(&self, exists: impl Fn(&Path) -> bool) -> Option<Answer> {
        self.iter()
            .find(|candidate| exists(candidate))
            .map(|candidate| Answer::Candidate(candidate.to_path_buf()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Candidate(PathBuf),
    TheCheckItself,
}

impl PresenceCheck {
    pub fn answered(&self, answer: &Answer) -> Option<String> {
        let (PresenceCheck::PathExists { paths }, Answer::Candidate(answering)) = (self, answer)
        else {
            return None;
        };

        let others: Vec<String> = paths
            .iter()
            .filter(|candidate| *candidate != answering)
            .map(|candidate| candidate.display().to_string())
            .collect();
        Some(match others.is_empty() {
            true => format!("answered by {}", answering.display()),
            false => format!(
                "answered by {}, not by {}",
                answering.display(),
                others.join(", ")
            ),
        })
    }
}

impl Display for PresenceCheck {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresenceCheck::PathExists { paths } => {
                let named: Vec<String> = paths
                    .iter()
                    .map(|candidate| candidate.display().to_string())
                    .collect();
                match named.as_slice() {
                    [only] => write!(formatter, "{only} does not exist"),
                    several => write!(formatter, "none of {} exists", several.join(", ")),
                }
            }
            PresenceCheck::CommandOnPath { command } => {
                write!(formatter, "{command} is not on the path")
            }
            PresenceCheck::CommandOutputContains { args, contains, .. } => {
                write!(
                    formatter,
                    "the output of `{}` does not contain {contains:?}",
                    args.join(" ")
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
        let read =
            serde_json::from_str::<PresenceCheck>(r#"{ "check": "path_exists", "paths": [] }"#);

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
            Some(
                "answered by AppData/Local/Docker, not by C:/Program Files/Docker, D:/Docker"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_check_answering_with_itself_names_no_candidate() {
        let check = PresenceCheck::CommandOnPath {
            command: "git".to_owned(),
        };

        assert_eq!(check.answered(&Answer::TheCheckItself), None);
    }
}

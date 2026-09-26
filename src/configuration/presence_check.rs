use {
    crate::configuration::resource::Shell,
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{fmt::Display, path::PathBuf},
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
    #[schemars(description = "A path exists. Relative paths resolve against the home directory.")]
    PathExists { path: PathBuf },
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

impl Display for PresenceCheck {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresenceCheck::PathExists { path } => {
                write!(formatter, "{} does not exist", path.display())
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

    #[test]
    fn a_check_reads_as_what_was_not_true_rather_than_as_the_condition_it_tests() {
        let check = PresenceCheck::CommandOnPath {
            command: "git".to_owned(),
        };

        assert_eq!(check.to_string(), "git is not on the path");
    }
}

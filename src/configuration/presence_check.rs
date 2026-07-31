use {
    crate::configuration::resource::Shell,
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{fmt::Display, path::PathBuf},
};

/// An author-declared test that establishes whether a resource is already in its desired state,
/// used where the machine cannot be asked directly.
///
/// The forms are a fixed set rather than arbitrary shell so that plan's guarantee is precise: plan
/// cannot change a machine through anything the tool decides, and can change one only through a
/// check the configuration's author wrote and declared as a check. Two of the three forms cannot
/// change anything by construction; `CommandOutputContains` is the narrow, deliberate escape
/// hatch. See ADR 0006.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "check", rename_all = "snake_case")]
pub enum PresenceCheck {
    /// A path exists. Relative paths resolve against the home directory.
    PathExists { path: PathBuf },
    /// A program is resolvable on the machine's search path.
    CommandOnPath { command: String },
    /// A declared command's output contains a string. The only form that can run something the
    /// author chose, and so the only one that is not side-effect-free by construction.
    CommandOutputContains {
        shell: Shell,
        args: Vec<String>,
        contains: String,
    },
}

/// A check is only ever rendered to explain why something is *not* in its desired state, so it
/// reads as the thing that was not true rather than as the condition that was tested.
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

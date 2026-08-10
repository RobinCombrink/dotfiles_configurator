use {
    super::generation::{BUILD_GENERATION, Generation},
    std::fmt::{Display, Formatter, Result},
};

/// A configuration a run could not turn into desired state. Three causes with three closures:
/// malformed is a fault in the repository it was read from and a person resolves it; too new is a
/// fault in the build reading it and the program resolves it by updating itself; too old is a
/// document this build has outgrown, and a person resolves it by running an intervening build once
/// or by rewriting the document. See ADR 0026.
#[derive(Debug)]
pub enum Unreadable {
    Malformed(anyhow::Error),
    TooNew {
        source: String,
        required: Generation,
        available: Generation,
    },
    TooOld {
        source: String,
        stated: Generation,
        oldest_readable: Generation,
    },
}

impl Unreadable {
    pub fn is_too_new(&self) -> bool {
        match self {
            Self::TooNew { .. } => true,
            Self::Malformed(_) | Self::TooOld { .. } => false,
        }
    }
}

impl Display for Unreadable {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        match self {
            Self::Malformed(fault) => write!(formatter, "{fault:#}"),
            Self::TooNew {
                source,
                required,
                available,
            } => write!(
                formatter,
                "{source} needs generation {required} of dotfiles_configurator, and this build is \
                 generation {available}. A newer build reads it."
            ),
            Self::TooOld {
                source,
                stated,
                oldest_readable,
            } => write!(
                formatter,
                "{source} states generation {stated} of dotfiles_configurator, and this build \
                 reads back as far as generation {oldest_readable}. Run an intervening build once, \
                 or rewrite it as a generation {BUILD_GENERATION} document."
            ),
        }
    }
}

impl std::error::Error for Unreadable {}

impl From<anyhow::Error> for Unreadable {
    fn from(fault: anyhow::Error) -> Self {
        Self::Malformed(fault)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::generation::{
            BENEATH_OLDEST_READABLE_GENERATION, BEYOND_BUILD_GENERATION, OLDEST_READABLE_GENERATION,
        },
        *,
    };

    #[test]
    fn a_configuration_needing_a_newer_build_is_reported_by_source_and_both_generations() {
        let unreadable = Unreadable::TooNew {
            source: "everywhere.dotconfig.json".to_owned(),
            required: BEYOND_BUILD_GENERATION,
            available: BUILD_GENERATION,
        };

        let reported = unreadable.to_string();

        assert!(
            reported.contains("everywhere.dotconfig.json")
                && reported.contains(&format!("generation {BEYOND_BUILD_GENERATION}"))
                && reported.contains(&format!("generation {BUILD_GENERATION}")),
            "{reported}"
        );
    }

    #[test]
    fn a_configuration_this_build_has_outgrown_is_answered_with_an_intervening_build() {
        let unreadable = Unreadable::TooOld {
            source: "everywhere.dotconfig.json".to_owned(),
            stated: BENEATH_OLDEST_READABLE_GENERATION,
            oldest_readable: OLDEST_READABLE_GENERATION,
        };

        let reported = unreadable.to_string();

        assert!(
            reported.contains("everywhere.dotconfig.json")
                && reported.contains(&format!("generation {BENEATH_OLDEST_READABLE_GENERATION}"))
                && reported.contains("intervening build"),
            "{reported}"
        );
    }

    #[test]
    fn a_malformed_configuration_is_reported_with_the_whole_chain_of_faults() {
        let fault = anyhow::anyhow!("resources[1] is not a shell")
            .context("personal.dotconfig.json is not a valid configuration");

        let reported = Unreadable::Malformed(fault).to_string();

        assert!(
            reported.contains("personal.dotconfig.json") && reported.contains("resources[1]"),
            "{reported}"
        );
    }
}

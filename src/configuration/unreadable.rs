use {
    super::{
        generation::{BUILD_GENERATION, Generation},
        names::ConfigurationName,
    },
    std::fmt::{Display, Formatter, Result},
};

// ADR 0026
#[derive(Debug)]
pub enum Unreadable {
    Malformed(anyhow::Error),
    TooNew {
        source: ConfigurationName,
        required: Generation,
        available: Generation,
    },
    TooOld {
        source: ConfigurationName,
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
    use super::{super::generation::OLDEST_READABLE_GENERATION, *};

    #[test]
    fn a_configuration_needing_a_newer_build_is_reported_by_source_and_both_generations() {
        let unreadable = Unreadable::TooNew {
            source: ConfigurationName::from("everywhere.dotconfig.json"),
            required: BUILD_GENERATION.stepped_by(1),
            available: BUILD_GENERATION,
        };

        let reported = unreadable.to_string();

        assert!(
            reported.contains("everywhere.dotconfig.json")
                && reported.contains(&format!("generation {}", BUILD_GENERATION.stepped_by(1)))
                && reported.contains(&format!("generation {BUILD_GENERATION}")),
            "{reported}"
        );
    }

    #[test]
    fn a_configuration_this_build_has_outgrown_is_answered_with_an_intervening_build() {
        let unreadable = Unreadable::TooOld {
            source: ConfigurationName::from("everywhere.dotconfig.json"),
            stated: OLDEST_READABLE_GENERATION.stepped_by(-1),
            oldest_readable: OLDEST_READABLE_GENERATION,
        };

        let reported = unreadable.to_string();

        assert!(
            reported.contains("everywhere.dotconfig.json")
                && reported.contains(&format!(
                    "generation {}",
                    OLDEST_READABLE_GENERATION.stepped_by(-1)
                ))
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

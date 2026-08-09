use {
    super::generation::Generation,
    std::fmt::{Display, Formatter, Result},
};

#[derive(Debug)]
pub enum Unreadable {
    Malformed(anyhow::Error),
    TooNew {
        source: String,
        required: Generation,
        available: Generation,
    },
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
    use {
        super::{super::generation::BEYOND_BUILD_GENERATION, *},
        crate::configuration::BUILD_GENERATION,
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

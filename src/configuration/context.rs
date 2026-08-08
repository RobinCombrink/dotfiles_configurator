use {
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{fmt::Display, str::FromStr},
    strum::{EnumIter, IntoEnumIterator},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, EnumIter)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    Everywhere,
    Personal,
    Work,
}

impl Context {
    /// ```
    /// use dotfiles_configurator::configuration::Context;
    ///
    /// assert!(Context::Everywhere.applies_on(Context::Personal));
    /// assert!(!Context::Work.applies_on(Context::Personal));
    /// ```
    pub fn applies_on(self, machine: Context) -> bool {
        self == Context::Everywhere || self == machine
    }

    fn as_written(self) -> &'static str {
        match self {
            Context::Everywhere => "everywhere",
            Context::Personal => "personal",
            Context::Work => "work",
        }
    }

    pub fn machine_described(self) -> &'static str {
        match self {
            Context::Everywhere => "a machine of no class",
            Context::Personal => "a personal machine",
            Context::Work => "a work machine",
        }
    }
}

impl Display for Context {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_written())
    }
}

impl FromStr for Context {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Context::iter()
            .find(|context| context.as_written() == value)
            .ok_or_else(|| {
                let known: Vec<&str> = Context::iter()
                    .map(|context| context.as_written())
                    .collect();
                format!(
                    "{value:?} is no machine this tool knows; expected one of {}",
                    known.join(", ")
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configuration_for_every_machine_applies_to_a_machine_of_any_class() {
        assert!(Context::Everywhere.applies_on(Context::Personal));
        assert!(Context::Everywhere.applies_on(Context::Work));
        assert!(Context::Everywhere.applies_on(Context::Everywhere));
    }

    #[test]
    fn a_configuration_for_one_class_of_machine_applies_to_no_other_class() {
        assert!(!Context::Personal.applies_on(Context::Work));
        assert!(!Context::Work.applies_on(Context::Personal));
    }

    #[test]
    fn a_configuration_for_one_class_of_machine_applies_to_that_class() {
        assert!(Context::Personal.applies_on(Context::Personal));
        assert!(Context::Work.applies_on(Context::Work));
    }

    #[test]
    fn a_machine_belonging_to_no_class_takes_nothing_written_for_a_class() {
        assert!(!Context::Personal.applies_on(Context::Everywhere));
        assert!(!Context::Work.applies_on(Context::Everywhere));
    }

    #[test]
    fn a_machine_this_tool_does_not_know_is_rejected_with_the_ones_it_does() {
        let error = Context::from_str("laptop").unwrap_err();

        assert!(
            error.contains("everywhere") && error.contains("personal") && error.contains("work"),
            "{error}"
        );
    }

    #[test]
    fn every_context_is_written_the_way_it_is_read() {
        for context in Context::iter() {
            assert_eq!(Context::from_str(&context.to_string()), Ok(context));
        }
    }

    #[test]
    fn every_context_reaches_json_spelled_the_way_the_command_line_reads_it() {
        for context in Context::iter() {
            assert_eq!(
                serde_json::to_string(&context).unwrap(),
                format!("\"{context}\"")
            );
        }
    }
}

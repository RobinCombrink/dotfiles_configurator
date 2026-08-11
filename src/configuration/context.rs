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
    /// use dotfiles_configurator::configuration::{Context, MachineClass};
    ///
    /// assert!(Context::Everywhere.applies_on(MachineClass::Personal));
    /// assert!(!Context::Work.applies_on(MachineClass::Personal));
    /// ```
    pub fn applies_on(self, machine: MachineClass) -> bool {
        match (self, machine) {
            (Context::Everywhere, _) => true,
            (Context::Personal, MachineClass::Personal) | (Context::Work, MachineClass::Work) => {
                true
            }
            (Context::Personal, MachineClass::Work) | (Context::Work, MachineClass::Personal) => {
                false
            }
        }
    }

    /// ```
    /// use dotfiles_configurator::configuration::Context;
    ///
    /// assert_eq!(Context::Everywhere.repositories_leaf(), "Personal");
    /// assert_eq!(Context::Work.repositories_leaf(), "Work");
    /// ```
    pub fn repositories_leaf(self) -> &'static str {
        match self {
            Context::Work => "Work",
            Context::Personal | Context::Everywhere => "Personal",
        }
    }

    fn as_written(self) -> &'static str {
        match self {
            Context::Everywhere => "everywhere",
            Context::Personal => "personal",
            Context::Work => "work",
        }
    }
}

impl Display for Context {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_written())
    }
}

// ADR 0025
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum MachineClass {
    Personal,
    Work,
}

impl MachineClass {
    fn as_written(self) -> &'static str {
        match self {
            MachineClass::Personal => "personal",
            MachineClass::Work => "work",
        }
    }

    pub fn described(self) -> &'static str {
        match self {
            MachineClass::Personal => "a personal machine",
            MachineClass::Work => "a work machine",
        }
    }

    /// ```
    /// use dotfiles_configurator::configuration::MachineClass;
    ///
    /// assert_eq!(MachineClass::Work.repositories_leaf(), "Work");
    /// assert_eq!(MachineClass::Personal.repositories_leaf(), "Personal");
    /// ```
    pub fn repositories_leaf(self) -> &'static str {
        Context::from(self).repositories_leaf()
    }
}

impl From<MachineClass> for Context {
    fn from(machine: MachineClass) -> Self {
        match machine {
            MachineClass::Personal => Context::Personal,
            MachineClass::Work => Context::Work,
        }
    }
}

impl Display for MachineClass {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_written())
    }
}

impl FromStr for MachineClass {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        MachineClass::iter()
            .find(|machine| machine.as_written() == value)
            .ok_or_else(|| {
                let known: Vec<&str> = MachineClass::iter()
                    .map(|machine| machine.as_written())
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
        assert!(Context::Everywhere.applies_on(MachineClass::Personal));
        assert!(Context::Everywhere.applies_on(MachineClass::Work));
    }

    #[test]
    fn a_configuration_for_one_class_of_machine_applies_to_no_other_class() {
        assert!(!Context::Personal.applies_on(MachineClass::Work));
        assert!(!Context::Work.applies_on(MachineClass::Personal));
    }

    #[test]
    fn a_configuration_for_one_class_of_machine_applies_to_that_class() {
        assert!(Context::Personal.applies_on(MachineClass::Personal));
        assert!(Context::Work.applies_on(MachineClass::Work));
    }

    #[test]
    fn a_machine_this_tool_does_not_know_is_rejected_with_the_ones_it_does() {
        let error = MachineClass::from_str("laptop").unwrap_err();

        assert!(
            error.contains("personal") && error.contains("work"),
            "{error}"
        );
    }

    #[test]
    fn no_invocation_can_name_the_context_a_configuration_writes_for_every_machine() {
        assert!(MachineClass::from_str("everywhere").is_err());
    }

    #[test]
    fn every_machine_class_is_written_the_way_it_is_read() {
        for machine in MachineClass::iter() {
            assert_eq!(MachineClass::from_str(&machine.to_string()), Ok(machine));
        }
    }

    #[test]
    fn every_context_reaches_json_spelled_the_way_a_configuration_writes_it() {
        for context in Context::iter() {
            assert_eq!(
                serde_json::to_string(&context).unwrap(),
                format!("\"{context}\"")
            );
        }
    }
}

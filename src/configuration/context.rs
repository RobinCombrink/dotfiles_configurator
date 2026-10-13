use {
    machine_manifest_document::MachineClass,
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::fmt::Display,
    strum::EnumIter,
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
    /// use dotfiles_configurator::configuration::{Context, MachineClass};
    ///
    /// assert_eq!(Context::Everywhere.repositories_leaf(), "Personal");
    /// assert_eq!(Context::Work.repositories_leaf(), "Work");
    /// assert_eq!(Context::from(MachineClass::Personal).repositories_leaf(), "Personal");
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

impl From<MachineClass> for Context {
    fn from(machine: MachineClass) -> Self {
        match machine {
            MachineClass::Personal => Context::Personal,
            MachineClass::Work => Context::Work,
        }
    }
}

#[cfg(test)]
#[path = "context/tests.rs"]
mod tests;

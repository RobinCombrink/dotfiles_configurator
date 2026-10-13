use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};

/// ```
/// use std::str::FromStr;
///
/// use machine_manifest_document::MachineClass;
///
/// assert_eq!(MachineClass::from_str("work"), Ok(MachineClass::Work));
/// assert_eq!(MachineClass::Personal.to_string(), "personal");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumIter)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(description = "The class of machine the run that wrote this manifest applied.")
)]
#[serde(rename_all = "snake_case")]
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
}

impl fmt::Display for MachineClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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
mod tests;

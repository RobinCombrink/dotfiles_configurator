use {
    schemars::{JsonSchema, Schema, SchemaGenerator, json_schema},
    serde::{Deserialize, Deserializer, Serialize, Serializer},
    std::{borrow::Cow, fmt::Display},
};

pub const BUILD_GENERATION: Generation = Generation(11);

// ADR 0026
pub const OLDEST_READABLE_GENERATION: Generation = Generation(BUILD_GENERATION.0 - 1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Generation(u32);

impl Generation {
    pub fn is_met_by(self, build: Generation) -> bool {
        self <= build
    }

    /// The generation this many steps from this one.
    ///
    /// ```
    /// # use dotfiles_configurator::configuration::BUILD_GENERATION;
    /// assert_eq!(BUILD_GENERATION.stepped_by(1).stepped_by(-1), BUILD_GENERATION);
    /// ```
    pub fn stepped_by(self, steps: i32) -> Self {
        Self(self.0.saturating_add_signed(steps))
    }

    pub fn is_outgrown_by(self, oldest_readable: Generation) -> bool {
        self < oldest_readable
    }
}

impl TryFrom<&str> for Generation {
    type Error = String;

    fn try_from(stated: &str) -> Result<Self, Self::Error> {
        if stated.is_empty() || !stated.bytes().all(|character| character.is_ascii_digit()) {
            return Err(format!(
                "\"{stated}\" is not a generation; a generation is written as digits alone"
            ));
        }

        stated.parse::<u32>().map(Self).map_err(|_| {
            format!("\"{stated}\" is larger than any generation this program can name")
        })
    }
}

impl Display for Generation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl Serialize for Generation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Generation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stated = Cow::<'de, str>::deserialize(deserializer)?;
        Self::try_from(stated.as_ref()).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Generation {
    fn schema_name() -> Cow<'static, str> {
        "Generation".into()
    }

    fn schema_id() -> Cow<'static, str> {
        concat!(module_path!(), "::Generation").into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": r"^\d+$",
        })
    }
}

#[cfg(test)]
#[path = "generation/tests.rs"]
mod tests;

use {
    schemars::{JsonSchema, Schema, SchemaGenerator, json_schema},
    serde::{Deserialize, Deserializer, Serialize, Serializer},
    std::{borrow::Cow, fmt::Display},
};

pub const BUILD_GENERATION: Generation = Generation(4);

pub const BEYOND_BUILD_GENERATION: Generation = Generation(BUILD_GENERATION.0 + 1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Generation(u32);

impl Generation {
    pub fn is_met_by(self, build: Generation) -> bool {
        self <= build
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
mod tests {
    use super::*;

    #[test]
    fn a_generation_this_build_has_passed_is_met_by_it() {
        assert!(
            Generation::try_from("2")
                .unwrap()
                .is_met_by(BUILD_GENERATION)
        );
    }

    #[test]
    fn the_generation_this_build_is_is_met_by_it() {
        assert!(BUILD_GENERATION.is_met_by(BUILD_GENERATION));
    }

    #[test]
    fn a_generation_beyond_this_build_is_not_met_by_it() {
        assert!(!BEYOND_BUILD_GENERATION.is_met_by(BUILD_GENERATION));
    }

    #[test]
    fn a_version_that_is_not_a_whole_number_is_not_a_generation() {
        let refusal = Generation::try_from("0.1.0").unwrap_err();

        assert!(refusal.contains("0.1.0"), "{refusal}");
    }

    #[test]
    fn a_version_carrying_a_sign_is_not_a_generation_the_published_schema_would_accept() {
        let refusal = Generation::try_from("+3").unwrap_err();

        assert!(refusal.contains("digits alone"), "{refusal}");
    }

    #[test]
    fn a_version_above_the_largest_generation_is_refused_for_its_size_not_its_shape() {
        let refusal = Generation::try_from("4294967296").unwrap_err();

        assert!(refusal.contains("larger than"), "{refusal}");
    }

    #[test]
    fn a_generation_is_written_the_way_it_is_read() {
        let stated = "3";

        let generation = Generation::try_from(stated).unwrap();

        assert_eq!(generation.to_string(), stated);
    }

    #[test]
    fn a_generation_reaches_json_as_the_string_a_configuration_states() {
        let generation = Generation::try_from("3").unwrap();

        let written = serde_json::to_string(&generation).unwrap();

        assert_eq!(written, r#""3""#);
        assert_eq!(
            serde_json::from_str::<Generation>(&written).unwrap(),
            generation
        );
    }
}

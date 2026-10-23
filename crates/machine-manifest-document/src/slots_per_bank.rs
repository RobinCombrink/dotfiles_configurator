use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{EnumIter, IntoEnumIterator};

/// ```
/// use machine_manifest_document::SlotsPerBank;
///
/// assert_eq!(SlotsPerBank::try_from(2), Ok(SlotsPerBank::Two));
/// assert_eq!(SlotsPerBank::Three.count(), 3);
/// assert!(SlotsPerBank::try_from(4).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum SlotsPerBank {
    Two,
    Three,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a bank holds two or three slots, never {0}")]
pub struct UnsupportedSlotsPerBank(pub u64);

impl SlotsPerBank {
    pub fn count(self) -> u8 {
        match self {
            SlotsPerBank::Two => 2,
            SlotsPerBank::Three => 3,
        }
    }
}

impl TryFrom<u64> for SlotsPerBank {
    type Error = UnsupportedSlotsPerBank;

    fn try_from(count: u64) -> Result<Self, Self::Error> {
        SlotsPerBank::iter()
            .find(|slots| u64::from(slots.count()) == count)
            .ok_or(UnsupportedSlotsPerBank(count))
    }
}

impl Serialize for SlotsPerBank {
    fn serialize<Output: Serializer>(
        &self,
        serializer: Output,
    ) -> Result<Output::Ok, Output::Error> {
        serializer.serialize_u8(self.count())
    }
}

impl<'de> Deserialize<'de> for SlotsPerBank {
    fn deserialize<Input: Deserializer<'de>>(deserializer: Input) -> Result<Self, Input::Error> {
        let count = u64::deserialize(deserializer)?;
        Self::try_from(count).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for SlotsPerBank {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SlotsPerBank".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::SlotsPerBank").into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "integer",
            "enum": [2, 3],
            "description": "How many session slots one bank of this machine's screen holds.",
        })
    }
}

#[cfg(test)]
mod tests;

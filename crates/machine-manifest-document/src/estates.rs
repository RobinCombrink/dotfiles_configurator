use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use github_owner::Owner;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, Visitor},
};

/// ```
/// use machine_manifest_document::EstateName;
///
/// assert!(EstateName::try_from("personal").is_ok());
/// assert!(EstateName::try_from("../personal").is_err());
/// assert!(EstateName::try_from("NUL").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EstateName(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotAnEstateName {
    #[error("an estate is named, and \"\" is not a name")]
    Unnamed,
    #[error(
        "{0:?} is not an estate name: an estate names a directory, so its name is letters, digits, \
         '-' and '_' alone"
    )]
    NotNameCharacters(String),
    #[error(
        "{0:?} is not an estate name: Windows reserves it for a device, so no directory can carry it"
    )]
    DeviceName(String),
}

const DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn is_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-' || character == '_'
}

impl TryFrom<String> for EstateName {
    type Error = NotAnEstateName;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.is_empty() {
            return Err(NotAnEstateName::Unnamed);
        }
        if !name.chars().all(is_name_character) {
            return Err(NotAnEstateName::NotNameCharacters(name));
        }
        if DEVICE_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
            return Err(NotAnEstateName::DeviceName(name));
        }
        Ok(Self(name))
    }
}

impl TryFrom<&str> for EstateName {
    type Error = NotAnEstateName;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::try_from(name.to_owned())
    }
}

impl EstateName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EstateName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for EstateName {
    fn serialize<Output: Serializer>(
        &self,
        serializer: Output,
    ) -> Result<Output::Ok, Output::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for EstateName {
    fn deserialize<Input: Deserializer<'de>>(deserializer: Input) -> Result<Self, Input::Error> {
        let name = String::deserialize(deserializer)?;
        Self::try_from(name).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for EstateName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "EstateName".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::EstateName").into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[A-Za-z0-9_-]+$",
            "description": "The name of an estate, which also names its directory: letters, \
                            digits, '-' and '_' alone.",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EstatesRefused {
    #[error("the {0} estate is listed twice")]
    EstateListedTwice(EstateName),
    #[error("{owner} is listed twice in the {estate} estate")]
    OwnerListedTwice { owner: Owner, estate: EstateName },
    #[error(
        "{owner} is an owner in both the {first} and the {second} estate. An owner belongs to one \
         estate."
    )]
    OwnerInTwoEstates {
        owner: Owner,
        first: EstateName,
        second: EstateName,
    },
}

/// ```
/// use github_owner::Owner;
/// use machine_manifest_document::{EstateName, Estates};
///
/// let owner = |spelled: &str| Owner::try_from(spelled).expect("an owner");
/// let estates = Estates::default()
///     .with(EstateName::try_from("work").expect("an estate name"), [owner("skynamo")])
///     .expect("one estate claims skynamo");
///
/// assert_eq!(estates.estate_of(&owner("Skynamo")).map(EstateName::as_str), Some("work"));
/// assert!(
///     estates
///         .with(EstateName::try_from("personal").expect("an estate name"), [owner("skynamo")])
///         .is_err()
/// );
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Estates(BTreeMap<EstateName, BTreeSet<Owner>>);

impl Estates {
    pub fn with(
        mut self,
        estate: EstateName,
        owners: impl IntoIterator<Item = Owner>,
    ) -> Result<Self, EstatesRefused> {
        if self.0.contains_key(&estate) {
            return Err(EstatesRefused::EstateListedTwice(estate));
        }
        let mut listed = BTreeSet::new();
        for owner in owners {
            if let Some(claimant) = self.estate_of(&owner) {
                return Err(EstatesRefused::OwnerInTwoEstates {
                    owner,
                    first: claimant.clone(),
                    second: estate,
                });
            }
            if listed.contains(&owner) {
                return Err(EstatesRefused::OwnerListedTwice { owner, estate });
            }
            listed.insert(owner);
        }
        self.0.insert(estate, listed);
        Ok(self)
    }

    pub fn estate_of(&self, owner: &Owner) -> Option<&EstateName> {
        self.0
            .iter()
            .find(|(_, owners)| owners.contains(owner))
            .map(|(estate, _)| estate)
    }

    pub fn owners_of(&self, estate: &EstateName) -> Option<&BTreeSet<Owner>> {
        self.0.get(estate)
    }

    pub fn machines_owners(&self) -> impl Iterator<Item = &Owner> {
        self.0.values().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for Estates {
    fn serialize<Output: Serializer>(
        &self,
        serializer: Output,
    ) -> Result<Output::Ok, Output::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Estates {
    fn deserialize<Input: Deserializer<'de>>(deserializer: Input) -> Result<Self, Input::Error> {
        deserializer
            .deserialize_map(EveryEstateInOrder)?
            .into_iter()
            .try_fold(Self::default(), |estates, (estate, owners)| {
                estates.with(estate, owners)
            })
            .map_err(serde::de::Error::custom)
    }
}

struct EveryEstateInOrder;

impl<'de> Visitor<'de> for EveryEstateInOrder {
    type Value = Vec<(EstateName, Vec<Owner>)>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object naming each estate and listing its owners")
    }

    fn visit_map<Entries: MapAccess<'de>>(
        self,
        mut entries: Entries,
    ) -> Result<Self::Value, Entries::Error> {
        let mut declared = Vec::new();
        while let Some(entry) = entries.next_entry::<EstateName, Vec<Owner>>()? {
            declared.push(entry);
        }
        Ok(declared)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Estates {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Estates".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::Estates").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let estate_name = generator.subschema_for::<EstateName>();
        let owner = generator.subschema_for::<Owner>();
        schemars::json_schema!({
            "type": "object",
            "description": "Each estate on this machine and the owners it groups. An estate is \
                            named once, lists an owner once, and an owner belongs to one estate.",
            "propertyNames": estate_name,
            "additionalProperties": {
                "type": "array",
                "uniqueItems": true,
                "items": owner,
            },
        })
    }
}

#[cfg(test)]
mod tests;

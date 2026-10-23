use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use github_owner::{NotAnOwner, Owner};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{IgnoredAny, MapAccess, SeqAccess, Visitor},
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

/// ```
/// use machine_manifest_document::Account;
///
/// let account = Account::try_from("Robin-Combrink").expect("an account");
///
/// assert_eq!(account.as_declared(), "Robin-Combrink");
/// assert_eq!(account, Account::try_from("robin-combrink").expect("an account"));
/// assert!(Account::try_from("Robin Combrink").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct Account(Owner);

impl TryFrom<String> for Account {
    type Error = NotAnOwner;

    fn try_from(login: String) -> Result<Self, Self::Error> {
        Owner::try_from(login).map(Self)
    }
}

impl TryFrom<&str> for Account {
    type Error = NotAnOwner;

    fn try_from(login: &str) -> Result<Self, Self::Error> {
        Self::try_from(login.to_owned())
    }
}

impl Account {
    pub fn as_declared(&self) -> &str {
        self.0.as_declared()
    }
}

impl fmt::Display for Account {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl Serialize for Account {
    fn serialize<Output: Serializer>(
        &self,
        serializer: Output,
    ) -> Result<Output::Ok, Output::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Account {
    fn deserialize<Input: Deserializer<'de>>(deserializer: Input) -> Result<Self, Input::Error> {
        Owner::deserialize(deserializer).map(Self)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Account {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Account".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::Account").into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": format!("^{}$", github_owner::OWNER_PATTERN),
            "description": "The GitHub login an estate acts as, spelled as the configuration \
                            declaring the estate spells it. It answers for the estate's owners \
                            and need not be one of them.",
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
/// use machine_manifest_document::{Account, EstateName, Estates};
///
/// let owner = |spelled: &str| Owner::try_from(spelled).expect("an owner");
/// let account = Account::try_from("Robin-Combrink").expect("an account");
/// let estates = Estates::default()
///     .with(
///         EstateName::try_from("work").expect("an estate name"),
///         [owner("skynamo")],
///         account.clone(),
///     )
///     .expect("one estate claims skynamo");
///
/// assert_eq!(estates.estate_of(&owner("Skynamo")).map(EstateName::as_str), Some("work"));
/// assert_eq!(estates.account_of(&owner("Skynamo")), Some(&account));
/// assert!(
///     estates
///         .with(
///             EstateName::try_from("personal").expect("an estate name"),
///             [owner("skynamo")],
///             account,
///         )
///         .is_err()
/// );
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Estates(BTreeMap<EstateName, Estate>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Estate {
    owners: BTreeSet<Owner>,
    account: Account,
}

impl Estates {
    pub fn with(
        mut self,
        estate: EstateName,
        owners: impl IntoIterator<Item = Owner>,
        account: Account,
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
        self.0.insert(
            estate,
            Estate {
                owners: listed,
                account,
            },
        );
        Ok(self)
    }

    fn holding(&self, owner: &Owner) -> Option<(&EstateName, &Estate)> {
        self.0.iter().find(|(_, held)| held.owners.contains(owner))
    }

    pub fn estate_of(&self, owner: &Owner) -> Option<&EstateName> {
        self.holding(owner).map(|(estate, _)| estate)
    }

    pub fn account_of(&self, owner: &Owner) -> Option<&Account> {
        self.holding(owner).map(|(_, held)| &held.account)
    }

    pub fn owners_of(&self, estate: &EstateName) -> Option<&BTreeSet<Owner>> {
        self.0.get(estate).map(|held| &held.owners)
    }

    pub fn machines_owners(&self) -> impl Iterator<Item = &Owner> {
        self.0.values().flat_map(|held| &held.owners)
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
            .try_fold(Self::default(), |estates, (estate, declared)| {
                estates.with(estate, declared.owners, declared.account)
            })
            .map_err(serde::de::Error::custom)
    }
}

struct EveryEstateInOrder;

impl<'de> Visitor<'de> for EveryEstateInOrder {
    type Value = Vec<(EstateName, DeclaredEstate)>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object naming each estate with its owners and its account")
    }

    fn visit_map<Entries: MapAccess<'de>>(
        self,
        mut entries: Entries,
    ) -> Result<Self::Value, Entries::Error> {
        let mut declared = Vec::new();
        while let Some(entry) = entries.next_entry::<EstateName, DeclaredEstate>()? {
            declared.push(entry);
        }
        Ok(declared)
    }
}

struct DeclaredEstate {
    owners: Vec<Owner>,
    account: Account,
}

const OWNERS_FIELD: &str = "owners";
const ACCOUNT_FIELD: &str = "account";

impl<'de> Deserialize<'de> for DeclaredEstate {
    fn deserialize<Input: Deserializer<'de>>(deserializer: Input) -> Result<Self, Input::Error> {
        deserializer.deserialize_any(OneEstate)
    }
}

struct OneEstate;

impl<'de> Visitor<'de> for OneEstate {
    type Value = DeclaredEstate;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object listing the estate's owners and naming its account")
    }

    fn visit_seq<Elements: SeqAccess<'de>>(
        self,
        mut elements: Elements,
    ) -> Result<Self::Value, Elements::Error> {
        while elements.next_element::<IgnoredAny>()?.is_some() {}
        Err(serde::de::Error::custom(
            "the estate is a bare list of owners, the shape written before each estate named its \
             account",
        ))
    }

    fn visit_map<Fields: MapAccess<'de>>(
        self,
        mut fields: Fields,
    ) -> Result<Self::Value, Fields::Error> {
        let mut owners = None;
        let mut account = None;
        while let Some(field) = fields.next_key::<String>()? {
            match field.as_str() {
                OWNERS_FIELD if owners.is_some() => {
                    return Err(serde::de::Error::duplicate_field(OWNERS_FIELD));
                }
                OWNERS_FIELD => owners = Some(fields.next_value::<Vec<Owner>>()?),
                ACCOUNT_FIELD if account.is_some() => {
                    return Err(serde::de::Error::duplicate_field(ACCOUNT_FIELD));
                }
                ACCOUNT_FIELD => account = Some(fields.next_value::<Account>()?),
                _ => {
                    fields.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(DeclaredEstate {
            owners: owners.ok_or_else(|| serde::de::Error::missing_field(OWNERS_FIELD))?,
            account: account.ok_or_else(|| serde::de::Error::missing_field(ACCOUNT_FIELD))?,
        })
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
        let account = generator.subschema_for::<Account>();
        schemars::json_schema!({
            "type": "object",
            "description": "Each estate on this machine, the owners it groups and the account it \
                            acts as. An estate is named once, lists an owner once, and an owner \
                            belongs to one estate.",
            "propertyNames": estate_name,
            "additionalProperties": {
                "type": "object",
                "properties": {
                    OWNERS_FIELD: {
                        "type": "array",
                        "uniqueItems": true,
                        "items": owner,
                    },
                    ACCOUNT_FIELD: account,
                },
                "required": [OWNERS_FIELD, ACCOUNT_FIELD],
            },
        })
    }
}

#[cfg(test)]
mod tests;

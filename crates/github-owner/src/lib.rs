use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
};

pub const OWNER_PATTERN: &str = "[A-Za-z0-9-]+";

/// ```
/// use github_owner::Owner;
///
/// let declared = Owner::try_from("RobinCombrink").expect("an owner");
///
/// assert_eq!(declared, Owner::try_from("robincombrink").expect("an owner"));
/// assert_eq!(declared.to_string(), "RobinCombrink");
/// assert!(Owner::try_from("robin combrink").is_err());
/// ```
#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct Owner(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not an owner, which is ASCII letters, digits and hyphens")]
pub struct NotAnOwner(pub String);

impl TryFrom<String> for Owner {
    type Error = NotAnOwner;

    fn try_from(candidate: String) -> Result<Self, Self::Error> {
        let spelled_as_an_owner = !candidate.is_empty()
            && candidate
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-');
        if !spelled_as_an_owner {
            return Err(NotAnOwner(candidate));
        }
        Ok(Self(candidate))
    }
}

impl TryFrom<&str> for Owner {
    type Error = NotAnOwner;

    fn try_from(candidate: &str) -> Result<Self, Self::Error> {
        Self::try_from(candidate.to_owned())
    }
}

impl Owner {
    pub fn as_declared(&self) -> &str {
        &self.0
    }

    fn folded(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.bytes().map(|byte| byte.to_ascii_lowercase())
    }
}

impl PartialEq for Owner {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for Owner {}

impl Ord for Owner {
    fn cmp(&self, other: &Self) -> Ordering {
        self.folded().cmp(other.folded())
    }
}

impl PartialOrd for Owner {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for Owner {
    fn hash<State: Hasher>(&self, state: &mut State) {
        self.folded().for_each(|byte| state.write_u8(byte));
    }
}

impl fmt::Display for Owner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl serde::Serialize for Owner {
    fn serialize<Serializer: serde::Serializer>(
        &self,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Owner {
    fn deserialize<Deserializer: serde::Deserializer<'de>>(
        deserializer: Deserializer,
    ) -> Result<Self, Deserializer::Error> {
        let candidate = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::try_from(candidate).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Owner {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Owner".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::Owner").into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": format!("^{OWNER_PATTERN}$"),
            "description": "An account or organisation that owns repositories on GitHub, \
                            compared without regard to case.",
        })
    }
}

#[cfg(test)]
mod tests;

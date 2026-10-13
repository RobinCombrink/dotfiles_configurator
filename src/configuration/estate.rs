use {
    crate::configuration::{
        Configuration,
        names::{ConfigurationName, GitHubAccount, RepositoryOwner},
    },
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{
        borrow::Cow,
        cmp::Ordering,
        collections::{BTreeMap, BTreeSet},
        fmt::Display,
        hash::{Hash, Hasher},
    },
};

/// The name of an estate, which is also the name of a directory, so it is one path segment that
/// every platform can hold.
///
/// ```
/// # use dotfiles_configurator::configuration::EstateName;
/// assert!(EstateName::try_from("personal").is_ok());
/// assert!(EstateName::try_from("../personal").is_err());
/// assert!(EstateName::try_from("NUL").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EstateName(String);

const DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn is_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-' || character == '_'
}

impl TryFrom<String> for EstateName {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.is_empty() {
            return Err("an estate is named, and \"\" is not a name".to_owned());
        }
        if !name.chars().all(is_name_character) {
            return Err(format!(
                "{name:?} is not an estate name: an estate names a directory, so its name is \
                 letters, digits, '-' and '_' alone"
            ));
        }
        if DEVICE_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
            return Err(format!(
                "{name:?} is not an estate name: Windows reserves it for a device, so no directory \
                 can carry it"
            ));
        }

        Ok(Self(name))
    }
}

impl TryFrom<&str> for EstateName {
    type Error = String;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::try_from(name.to_owned())
    }
}

impl Display for EstateName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for EstateName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for EstateName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::try_from(name).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for EstateName {
    fn schema_name() -> Cow<'static, str> {
        "EstateName".into()
    }

    fn schema_id() -> Cow<'static, str> {
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

/// An account or organisation owning repositories, spelled as it was declared and compared
/// without regard to case, as GitHub compares it.
///
/// ```
/// # use dotfiles_configurator::configuration::EstateOwner;
/// assert_eq!(EstateOwner::from("RobinCombrink"), EstateOwner::from("robincombrink"));
/// assert_eq!(EstateOwner::from("RobinCombrink").to_string(), "RobinCombrink");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "An account or organisation that owns repositories on GitHub.")]
#[serde(transparent)]
#[repr(transparent)]
pub struct EstateOwner(String);

impl EstateOwner {
    fn compared(&self) -> String {
        self.0.to_lowercase()
    }
}

impl PartialEq for EstateOwner {
    fn eq(&self, other: &Self) -> bool {
        self.compared() == other.compared()
    }
}

impl Eq for EstateOwner {}

impl PartialOrd for EstateOwner {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EstateOwner {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compared().cmp(&other.compared())
    }
}

impl Hash for EstateOwner {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.compared().hash(state);
    }
}

impl From<&str> for EstateOwner {
    fn from(owner: &str) -> Self {
        Self(owner.to_owned())
    }
}

impl From<&GitHubAccount> for EstateOwner {
    fn from(account: &GitHubAccount) -> Self {
        Self(account.to_string())
    }
}

impl From<&RepositoryOwner> for EstateOwner {
    fn from(owner: &RepositoryOwner) -> Self {
        Self(owner.to_string())
    }
}

impl Display for EstateOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(
    description = "The estate this configuration governs. Its owners are the configuration's \
                   GitHub account, the\n\
                   owners of its workspaces, and any named here besides; the owner of a \
                   repository it only clones\n\
                   is never one of them."
)]
pub struct EstateDeclaration {
    pub name: EstateName,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owners: Vec<EstateOwner>,
}

pub type Estates = BTreeMap<EstateName, BTreeSet<EstateOwner>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EstateConflict {
    DeclaredTwice {
        estate: EstateName,
        first: ConfigurationName,
        second: ConfigurationName,
    },
    OwnerInTwoEstates {
        owner: EstateOwner,
        first: EstateName,
        second: EstateName,
    },
}

impl Display for EstateConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EstateConflict::DeclaredTwice {
                estate,
                first,
                second,
            } => write!(
                formatter,
                "The estate {estate} is declared by both {first} and {second}. Each estate is \
                 declared by one configuration."
            ),
            EstateConflict::OwnerInTwoEstates {
                owner,
                first,
                second,
            } => write!(
                formatter,
                "{owner} is an owner in both the {first} and the {second} estate. An owner belongs \
                 to one estate."
            ),
        }
    }
}

impl std::error::Error for EstateConflict {}

impl Configuration {
    fn estate_owners(&self) -> BTreeSet<EstateOwner> {
        std::iter::once(EstateOwner::from(&self.github_account))
            .chain(
                self.workspaces
                    .iter()
                    .map(|workspace| EstateOwner::from(&workspace.repository.owner)),
            )
            .chain(
                self.estate
                    .iter()
                    .flat_map(|estate| estate.owners.iter().cloned()),
            )
            .collect()
    }
}

pub fn resolve_estates<'configuration>(
    configurations: impl IntoIterator<
        Item = (
            &'configuration ConfigurationName,
            &'configuration Configuration,
        ),
    >,
) -> Result<Estates, EstateConflict> {
    let mut declared_by: BTreeMap<EstateName, &ConfigurationName> = BTreeMap::new();
    let mut estate_of: BTreeMap<EstateOwner, EstateName> = BTreeMap::new();
    let mut estates = Estates::new();

    for (name, configuration) in configurations {
        let Some(declaration) = &configuration.estate else {
            continue;
        };
        if let Some(first) = declared_by.insert(declaration.name.clone(), name) {
            return Err(EstateConflict::DeclaredTwice {
                estate: declaration.name.clone(),
                first: first.clone(),
                second: name.clone(),
            });
        }

        let owners = configuration.estate_owners();
        for owner in &owners {
            if let Some(first) = estate_of.insert(owner.clone(), declaration.name.clone()) {
                return Err(EstateConflict::OwnerInTwoEstates {
                    owner: owner.clone(),
                    first,
                    second: declaration.name.clone(),
                });
            }
        }
        estates.insert(declaration.name.clone(), owners);
    }

    Ok(estates)
}

#[cfg(test)]
#[path = "estate/tests.rs"]
mod tests;

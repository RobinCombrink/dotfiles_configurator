use {
    crate::configuration::{
        Configuration,
        names::{ConfigurationName, GitHubAccount, RepositoryOwner},
        resource::Symlink,
    },
    github_owner::{NotAnOwner, Owner},
    machine_manifest_document::{Account, EstateName, Estates, EstatesRefused, MANIFEST_DIRECTORY},
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt::Display,
        path::{Component, Path, PathBuf},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(description = "The estate this configuration governs.")]
pub struct EstateDeclaration {
    pub name: EstateName,
    #[schemars(
        description = "Path within the dotfiles repository of the folder holding the estate's \
                       estate.json, linked at .dotconfig/estates/<name>."
    )]
    #[schemars(with = "String")]
    pub folder: WithinTheRepository,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "PathBuf", into = "String")]
pub struct WithinTheRepository(PathBuf);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutsideTheRepository {
    Rooted(PathBuf),
    Climbing(PathBuf),
}

impl Display for OutsideTheRepository {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutsideTheRepository::Rooted(path) => write!(
                formatter,
                "{} is rooted, and a folder of an estate is named relative to the root of the \
                 dotfiles repository",
                path.display()
            ),
            OutsideTheRepository::Climbing(path) => write!(
                formatter,
                "{} climbs out through `..`, and a folder of an estate lies within the dotfiles \
                 repository",
                path.display()
            ),
        }
    }
}

impl std::error::Error for OutsideTheRepository {}

impl TryFrom<PathBuf> for WithinTheRepository {
    type Error = OutsideTheRepository;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let mut segments = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Normal(segment) => segments.push(segment),
                Component::CurDir => {}
                Component::Prefix(_) | Component::RootDir => {
                    return Err(OutsideTheRepository::Rooted(path));
                }
                Component::ParentDir => return Err(OutsideTheRepository::Climbing(path)),
            }
        }
        Ok(Self(segments))
    }
}

impl From<WithinTheRepository> for String {
    fn from(within: WithinTheRepository) -> Self {
        within.to_string()
    }
}

impl Display for WithinTheRepository {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let segments: Vec<_> = self
            .0
            .iter()
            .map(|segment| segment.to_string_lossy())
            .collect();
        formatter.write_str(&segments.join("/"))
    }
}

impl WithinTheRepository {
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, segment: &str) -> Self {
        Self(self.0.join(segment))
    }
}

const ESTATES_DIRECTORY: &str = "estates";

impl EstateDeclaration {
    pub fn link_path_of(name: &EstateName) -> PathBuf {
        Path::new(MANIFEST_DIRECTORY)
            .join(ESTATES_DIRECTORY)
            .join(name.as_str())
    }

    pub fn link(&self) -> Symlink {
        Symlink {
            source_path: self.folder.as_path().to_path_buf(),
            link_path: Self::link_path_of(&self.name),
        }
    }

    pub fn file_within_the_repository(&self) -> WithinTheRepository {
        self.folder.join(ESTATE_FILE)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EstateConflict {
    DeclaredTwice {
        estate: EstateName,
        first: ConfigurationName,
        second: ConfigurationName,
    },
    NotAnOwner {
        estate: EstateName,
        configuration: ConfigurationName,
        cause: NotAnOwner,
    },
    OwnerNotKeyed {
        configuration: ConfigurationName,
        owner: Owner,
        file: EstateFile,
    },
    Refused(EstatesRefused),
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
            EstateConflict::NotAnOwner {
                estate,
                configuration,
                cause,
            } => write!(
                formatter,
                "{configuration} places an owner in the {estate} estate that GitHub would not \
                 name: {cause}."
            ),
            EstateConflict::OwnerNotKeyed {
                configuration,
                owner,
                file,
            } => write!(
                formatter,
                "{configuration} acts as or builds a workspace of {owner}, and {file} does not key \
                 {owner} in its `owners` object. The account a configuration acts as and the \
                 owners of its workspaces are owners of the estate it declares."
            ),
            EstateConflict::Refused(refused) => Display::fmt(refused, formatter),
        }
    }
}

impl std::error::Error for EstateConflict {}

pub const ESTATE_FILE: &str = "estate.json";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EstateFile(String);

impl From<&Path> for EstateFile {
    fn from(path: &Path) -> Self {
        Self(path.components().collect::<PathBuf>().display().to_string())
    }
}

impl From<String> for EstateFile {
    fn from(named: String) -> Self {
        Self(named)
    }
}

impl Display for EstateFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct EstateOwners(BTreeSet<Owner>);

#[derive(Debug)]
pub enum EstateFileFault {
    NotJson(serde_json::Error),
    NoOwnersObject,
    NotAnOwner(NotAnOwner),
}

impl Display for EstateFileFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EstateFileFault::NotJson(failure) => {
                write!(formatter, "it is not valid JSON: {failure}")
            }
            EstateFileFault::NoOwnersObject => formatter.write_str(
                "it holds no top-level `owners` object, and an estate's owners are that object's \
                 keys",
            ),
            EstateFileFault::NotAnOwner(cause) => {
                write!(
                    formatter,
                    "a key of its `owners` object is not one: {cause}"
                )
            }
        }
    }
}

impl EstateOwners {
    pub fn decoded(contents: &str) -> Result<Self, EstateFileFault> {
        let document: serde_json::Value =
            serde_json::from_str(contents).map_err(EstateFileFault::NotJson)?;
        let owners = document
            .get("owners")
            .and_then(serde_json::Value::as_object)
            .ok_or(EstateFileFault::NoOwnersObject)?;

        owners
            .keys()
            .map(|key| Owner::try_from(key.as_str()))
            .collect::<Result<_, _>>()
            .map(Self)
            .map_err(EstateFileFault::NotAnOwner)
    }

    pub fn contains(&self, owner: &Owner) -> bool {
        self.0.contains(owner)
    }

    pub fn owners(&self) -> &BTreeSet<Owner> {
        &self.0
    }
}

#[derive(Debug)]
pub struct EstateFileRefused {
    pub file: EstateFile,
    pub fault: EstateFileFault,
}

impl Display for EstateFileRefused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} cannot be read as an estate's owners: {}",
            self.file, self.fault
        )
    }
}

impl std::error::Error for EstateFileRefused {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadEstate {
    pub name: EstateName,
    pub file: EstateFile,
    pub owners: EstateOwners,
}

impl ReadEstate {
    pub fn decoded(
        name: EstateName,
        file: EstateFile,
        contents: &str,
    ) -> Result<Self, EstateFileRefused> {
        match EstateOwners::decoded(contents) {
            Ok(owners) => Ok(Self { name, file, owners }),
            Err(fault) => Err(EstateFileRefused { file, fault }),
        }
    }
}

impl Configuration {
    fn estate_account(&self) -> Result<Account, NotAnOwner> {
        Account::try_from(GitHubAccount::as_ref(&self.github_account))
    }

    fn owners_its_estate_keys(&self) -> Result<Vec<Owner>, NotAnOwner> {
        std::iter::once(Owner::try_from(GitHubAccount::as_ref(&self.github_account)))
            .chain(self.workspaces.iter().map(|workspace| {
                Owner::try_from(RepositoryOwner::as_ref(&workspace.repository.owner))
            }))
            .collect()
    }
}

pub fn resolve_estates<'configuration>(
    configurations: impl IntoIterator<
        Item = (
            &'configuration ConfigurationName,
            &'configuration Configuration,
            Option<&'configuration ReadEstate>,
        ),
    >,
) -> Result<Estates, EstateConflict> {
    let mut declared_by: BTreeMap<EstateName, &ConfigurationName> = BTreeMap::new();
    let mut estates = Estates::default();

    for (name, configuration, estate) in configurations {
        let Some(estate) = estate else {
            continue;
        };
        if let Some(first) = declared_by.insert(estate.name.clone(), name) {
            return Err(EstateConflict::DeclaredTwice {
                estate: estate.name.clone(),
                first: first.clone(),
                second: name.clone(),
            });
        }

        let not_an_owner = |cause| EstateConflict::NotAnOwner {
            estate: estate.name.clone(),
            configuration: name.clone(),
            cause,
        };
        let keyed = configuration
            .owners_its_estate_keys()
            .map_err(not_an_owner)?;
        let account = configuration.estate_account().map_err(not_an_owner)?;
        if let Some(owner) = keyed
            .into_iter()
            .find(|owner| !estate.owners.contains(owner))
        {
            return Err(EstateConflict::OwnerNotKeyed {
                configuration: name.clone(),
                owner,
                file: estate.file.clone(),
            });
        }

        estates = estates
            .with(estate.name.clone(), estate.owners.owners().clone(), account)
            .map_err(EstateConflict::Refused)?;
    }

    Ok(estates)
}

#[cfg(test)]
#[path = "estate/tests.rs"]
mod tests;

use {
    crate::configuration::{
        Configuration,
        names::{ConfigurationName, GitHubAccount, RepositoryOwner},
    },
    github_owner::{NotAnOwner, Owner},
    machine_manifest_document::{Account, EstateName, Estates, EstatesRefused},
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt::Display,
    },
};

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
    pub owners: Vec<Owner>,
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
            EstateConflict::Refused(refused) => Display::fmt(refused, formatter),
        }
    }
}

impl std::error::Error for EstateConflict {}

impl Configuration {
    fn estate_account(&self) -> Result<Account, NotAnOwner> {
        Account::try_from(GitHubAccount::as_ref(&self.github_account))
    }

    fn estate_owners(&self) -> Result<BTreeSet<Owner>, NotAnOwner> {
        std::iter::once(Owner::try_from(GitHubAccount::as_ref(&self.github_account)))
            .chain(self.workspaces.iter().map(|workspace| {
                Owner::try_from(RepositoryOwner::as_ref(&workspace.repository.owner))
            }))
            .chain(
                self.estate
                    .iter()
                    .flat_map(|estate| estate.owners.iter().cloned().map(Ok)),
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
    let mut estates = Estates::default();

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

        let not_an_owner = |cause| EstateConflict::NotAnOwner {
            estate: declaration.name.clone(),
            configuration: name.clone(),
            cause,
        };
        let owners = configuration.estate_owners().map_err(not_an_owner)?;
        let account = configuration.estate_account().map_err(not_an_owner)?;
        estates = estates
            .with(declaration.name.clone(), owners, account)
            .map_err(EstateConflict::Refused)?;
    }

    Ok(estates)
}

#[cfg(test)]
#[path = "estate/tests.rs"]
mod tests;

use {
    super::{
        Configuration,
        context::Context,
        estate::{
            EstateDeclaration, EstateFile, OutsideTheRepository, ReadEstate, WithinTheRepository,
        },
        generation::{BUILD_GENERATION, Generation},
        names::GitHubAccount,
        resource::{Resource, Symlink},
        workspace::CargoWorkspace,
    },
    crate::configuration::DeclaredNotice,
    anyhow::{Context as _, Result},
    github_owner::Owner,
    machine_manifest_document::{EstateName, SlotsPerBank},
    serde::Deserialize,
    serde_json::ser::PrettyFormatter,
    std::{
        fmt::Display,
        path::{Path, PathBuf},
    },
};

// ADR 0026
#[derive(Debug, Deserialize)]
pub struct OutgoingConfiguration {
    version: Generation,
    applies_to: Context,
    github_account: GitHubAccount,
    #[serde(default)]
    estate: Option<OutgoingEstateDeclaration>,
    #[serde(default)]
    slots_per_bank: Option<SlotsPerBank>,
    #[serde(default)]
    workspaces: Vec<CargoWorkspace>,
    #[serde(default)]
    resources: Vec<Resource>,
    #[serde(default)]
    notices: Vec<DeclaredNotice>,
}

#[derive(Debug, Deserialize)]
struct OutgoingEstateDeclaration {
    name: EstateName,
    #[serde(default)]
    owners: Vec<Owner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationRefused {
    NoEstateLink {
        estate: EstateName,
        link_path: PathBuf,
    },
    OwnerMissingFromTheEstateFile {
        estate: EstateName,
        owner: Owner,
        file: EstateFile,
    },
    FolderOutsideTheRepository {
        estate: EstateName,
        cause: OutsideTheRepository,
    },
}

impl Display for MigrationRefused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationRefused::NoEstateLink { estate, link_path } => write!(
                formatter,
                "it declares the estate {estate} and no symlink at {}, which is where the folder \
                 of a generation {BUILD_GENERATION} estate is read from",
                link_path.display()
            ),
            MigrationRefused::OwnerMissingFromTheEstateFile {
                estate,
                owner,
                file,
            } => write!(
                formatter,
                "it names {owner} as an owner of the estate {estate}, and {file} does not key \
                 {owner} in its `owners` object"
            ),
            MigrationRefused::FolderOutsideTheRepository { estate, cause } => write!(
                formatter,
                "the symlink linking the estate {estate} cannot name its folder: {cause}"
            ),
        }
    }
}

impl std::error::Error for MigrationRefused {}

#[derive(Debug)]
pub struct PendingMigration {
    from: Generation,
    declared_owners: Vec<Owner>,
}

impl PendingMigration {
    pub fn completed(self, estate: Option<&ReadEstate>) -> Result<Generation, MigrationRefused> {
        let Some(estate) = estate else {
            return Ok(self.from);
        };
        match self
            .declared_owners
            .into_iter()
            .find(|owner| !estate.owners.contains(owner))
        {
            None => Ok(self.from),
            Some(owner) => Err(MigrationRefused::OwnerMissingFromTheEstateFile {
                estate: estate.name.clone(),
                owner,
                file: estate.file.clone(),
            }),
        }
    }
}

impl OutgoingConfiguration {
    pub fn migrated(self) -> Result<(Configuration, PendingMigration), MigrationRefused> {
        let mut resources = self.resources;
        let (estate, declared_owners) =
            match self.estate {
                None => (None, Vec::new()),
                Some(outgoing) => {
                    let link_path = EstateDeclaration::link_path_of(&outgoing.name);
                    let Some((position, folder)) = resources.iter().enumerate().find_map(
                        |(position, resource)| match resource {
                            Resource::Symlink(Symlink {
                                source_path,
                                link_path: declared,
                            }) if *declared == link_path => Some((position, source_path.clone())),
                            _ => None,
                        },
                    ) else {
                        return Err(MigrationRefused::NoEstateLink {
                            estate: outgoing.name,
                            link_path,
                        });
                    };
                    let folder = match WithinTheRepository::try_from(folder) {
                        Ok(folder) => folder,
                        Err(cause) => {
                            return Err(MigrationRefused::FolderOutsideTheRepository {
                                estate: outgoing.name,
                                cause,
                            });
                        }
                    };
                    resources.remove(position);
                    (
                        Some(EstateDeclaration {
                            name: outgoing.name,
                            folder,
                        }),
                        outgoing.owners,
                    )
                }
            };

        Ok((
            Configuration {
                version: BUILD_GENERATION,
                applies_to: self.applies_to,
                github_account: self.github_account,
                estate,
                slots_per_bank: self.slots_per_bank,
                workspaces: self.workspaces,
                resources,
                notices: self.notices,
            },
            PendingMigration {
                from: self.version,
                declared_owners,
            },
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    path: PathBuf,
    rewritten: String,
    from: Generation,
}

impl Migration {
    pub fn of(path: &Path, configuration: &Configuration, from: Generation) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            rewritten: as_written(configuration)?,
            from,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn contents(&self) -> &str {
        &self.rewritten
    }
}

impl Display for Migration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} from generation {} to generation {BUILD_GENERATION}",
            self.path.display(),
            self.from
        )
    }
}

fn as_written(configuration: &Configuration) -> Result<String> {
    let mut written = Vec::new();
    let mut serializer =
        serde_json::Serializer::with_formatter(&mut written, PrettyFormatter::with_indent(b"    "));
    serde::Serialize::serialize(configuration, &mut serializer)
        .context("Could not write the migrated configuration")?;
    written.push(b'\n');

    String::from_utf8(written).context("The migrated configuration is not valid UTF-8")
}

#[cfg(test)]
#[path = "migration/tests.rs"]
mod tests;

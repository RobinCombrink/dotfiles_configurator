use {
    anyhow::{Context as _, anyhow},
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::fmt::Display,
};

// ADR 0031
#[path = "configuration/context.rs"]
pub mod context;
#[path = "configuration/estate.rs"]
pub mod estate;
#[path = "configuration/generation.rs"]
pub mod generation;
#[path = "configuration/identity.rs"]
pub mod identity;
#[path = "configuration/migration.rs"]
pub mod migration;
#[path = "configuration/names.rs"]
pub mod names;
#[path = "configuration/package_currency.rs"]
pub mod package_currency;
#[path = "configuration/path_folding.rs"]
pub(crate) mod path_folding;
#[path = "configuration/presence_check.rs"]
pub mod presence_check;
#[path = "configuration/requirement.rs"]
pub mod requirement;
#[path = "configuration/resource.rs"]
pub mod resource;
#[path = "configuration/slots_per_bank.rs"]
pub mod slots_per_bank;
#[path = "configuration/unreadable.rs"]
pub mod unreadable;
#[path = "configuration/workspace.rs"]
pub mod workspace;

pub use {
    context::Context,
    estate::{EstateConflict, EstateDeclaration, resolve_estates},
    generation::{BUILD_GENERATION, Generation, OLDEST_READABLE_GENERATION},
    identity::{Identity, LinkPath},
    migration::Migration,
    names::{
        ApplicationName, BinaryName, ConfigurationName, CrateName, CrateVersion, DartPackageName,
        GitCommit, GitHubAccount, GitReferenceName, GitRemoteUrl, McpServerName, PythonInterpreter,
        RepositoryName, RepositoryOwner, RepositorySubdirectory, UvToolName, UvToolVersion,
        VariableName, VariableValue, WingetPackageId, WingetVersion,
    },
    package_currency::PackageCurrency,
    presence_check::{Answer, Candidates, PresenceCheck},
    requirement::{Requirement, Tool},
    resource::{
        Application, ApplicationSource, ArchiveEntry, AssetPattern, CargoPackage, CargoSource,
        ClaudeMcpServer, CloneDepth, Command, DartPackage, DartReference, DartSource,
        EnvironmentVariable, GitHubRepository, Installer, McpScope, Package, Registration,
        ReleasedBinary, RenderedManifest, RepositoryClone, Resource, ResourceKind,
        SearchPathDirectory, SearchPathEntry, Shell, Symlink, UnreportedVersion, UvToolPackage,
        Variable, VersionInvocation, VersionWord, WingetPackage, recorded_run_in,
    },
    slots_per_bank::{SlotsPerBankDeclaredTwice, resolve_slots_per_bank},
    unreadable::Unreadable,
    workspace::CargoWorkspace,
};

pub use machine_manifest_document::{
    EstateName, Estates, EstatesRefused, MachineClass, MachineManifest, NotRooted, RecordedRun,
    RecordedSource, RepositoriesDirectory, SlotsPerBank,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(
    description = "A configuration as it is written down: the desired state of a machine, \
                          as a set of resources and the notices that accompany them, and which \
                          machines it is for."
)]
pub struct Configuration {
    #[schemars(
        description = "The lowest generation of dotfiles_configurator that can read this \
                       configuration, written as a whole number. A build reads any configuration \
                       at or below its own generation and refuses one above."
    )]
    pub version: Generation,
    #[schemars(
        description = "Which machines this configuration is for. One declaring \"everywhere\" \
                       applies to every machine; any other applies only to a machine an \
                       invocation names as being that."
    )]
    pub applies_to: Context,
    #[schemars(
        description = "The GitHub account every resource this configuration declares acts as. \
                       Not a repository's owner — a public repository is read by any account at \
                       all."
    )]
    pub github_account: GitHubAccount,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estate: Option<EstateDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "How many session slots one bank of this machine's screen holds, recorded \
                       in the machine manifest. Declared by at most one of the configurations a \
                       machine applies."
    )]
    pub slots_per_bank: Option<SlotsPerBank>,
    #[serde(default)]
    pub workspaces: Vec<CargoWorkspace>,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub notices: Vec<DeclaredNotice>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[schemars(
    description = "A message for a person about something the tool cannot do. Not a resource — \
                   it never\n\
                   converges, is reported by every change set, and is never applied."
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct DeclaredNotice(String);

impl From<String> for DeclaredNotice {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for DeclaredNotice {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl Display for DeclaredNotice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Everything a run has to tell a person that is not a change to the machine, carried as the
/// facts the site that raised it held and rendered to text only where it is shown.
///
/// ```
/// # use dotfiles_configurator::configuration::Notice;
/// assert!(Notice::EnvironmentChanged.to_string().contains("open a new one"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Notice {
    Declared(DeclaredNotice),
    // ADR 0022
    SupersededImage(std::path::PathBuf),
    // ADR 0017
    EnvironmentChanged,
    // ADR 0026
    SourceCannotBeRewritten {
        source: ConfigurationName,
        from: Generation,
    },
}

impl From<DeclaredNotice> for Notice {
    fn from(declared: DeclaredNotice) -> Self {
        Notice::Declared(declared)
    }
}

impl Display for Notice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Notice::Declared(declared) => Display::fmt(declared, formatter),
            Notice::SupersededImage(path) => write!(
                formatter,
                "{} is a binary that was replaced while it was being executed; an apply removes \
                 it once nothing is running it",
                path.display()
            ),
            Notice::EnvironmentChanged => formatter.write_str(
                "The environment changed. The shell this run was started from reads its \
                 environment once, at launch, so it will not see the change — open a new one.",
            ),
            Notice::SourceCannotBeRewritten { source, from } => write!(
                formatter,
                "{source} states generation {from} of dotfiles_configurator and was read as \
                 generation {BUILD_GENERATION}. This source cannot be written, so rewrite it \
                 there before generation {OLDEST_READABLE_GENERATION} stops being read."
            ),
        }
    }
}

#[derive(Debug)]
pub struct Reading {
    pub configuration: Configuration,
    pub migrated_from: Option<Generation>,
}

pub fn parse_configuration(
    contents: &str,
    source: &ConfigurationName,
) -> Result<Reading, Unreadable> {
    let document: serde_json::Value =
        serde_json::from_str(contents).with_context(|| format!("{source} is not valid JSON"))?;

    let stated = match document.get("version") {
        None => Err(anyhow!(
            "{source} declares no version. A configuration's version is the lowest generation of \
             dotfiles_configurator that can read it, written as a quoted whole number, and this \
             build is generation {BUILD_GENERATION}."
        )),
        Some(unquoted) => unquoted.as_str().ok_or_else(|| {
            anyhow!(
                "{source} declares the version {unquoted}, which a configuration writes as a \
                 quoted whole number. This build is generation {BUILD_GENERATION}."
            )
        }),
    }?;

    let required = Generation::try_from(stated).map_err(|fault| {
        anyhow!(
            "{source}: {fault}. This build reads generation {BUILD_GENERATION}, so rewrite the \
             configuration as a generation {BUILD_GENERATION} document."
        )
    })?;

    if !required.is_met_by(BUILD_GENERATION) {
        return Err(Unreadable::TooNew {
            source: source.clone(),
            required,
            available: BUILD_GENERATION,
        });
    }

    if required.is_outgrown_by(OLDEST_READABLE_GENERATION) {
        return Err(Unreadable::TooOld {
            source: source.clone(),
            stated: required,
            oldest_readable: OLDEST_READABLE_GENERATION,
        });
    }

    if required == OLDEST_READABLE_GENERATION {
        let outgoing: migration::OutgoingConfiguration = serde_path_to_error::deserialize(document)
            .with_context(|| format!("{source} is not a valid generation {required} configuration"))
            .map_err(Unreadable::Malformed)?;

        return Ok(Reading {
            migrated_from: Some(outgoing.stated_generation()),
            configuration: outgoing.into(),
        });
    }

    serde_path_to_error::deserialize(document)
        .with_context(|| {
            format!("{source} is not a valid generation {BUILD_GENERATION} configuration")
        })
        .map_err(Unreadable::Malformed)
        .map(|configuration| Reading {
            configuration,
            migrated_from: None,
        })
}

#[cfg(test)]
#[path = "configuration/tests.rs"]
mod tests;

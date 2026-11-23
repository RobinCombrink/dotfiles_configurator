use {
    super::{
        Configuration,
        context::Context,
        estate::EstateDeclaration,
        generation::{BUILD_GENERATION, Generation},
        names::GitHubAccount,
        resource::Resource,
        workspace::CargoWorkspace,
    },
    crate::configuration::DeclaredNotice,
    anyhow::{Context as _, Result},
    machine_manifest_document::SlotsPerBank,
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
    estate: Option<EstateDeclaration>,
    #[serde(default)]
    slots_per_bank: Option<SlotsPerBank>,
    #[serde(default)]
    workspaces: Vec<CargoWorkspace>,
    #[serde(default)]
    resources: Vec<Resource>,
    #[serde(default)]
    notices: Vec<DeclaredNotice>,
}

impl OutgoingConfiguration {
    pub fn migrated(self) -> (Configuration, Generation) {
        (
            Configuration {
                version: BUILD_GENERATION,
                applies_to: self.applies_to,
                github_account: self.github_account,
                estate: self.estate,
                slots_per_bank: self.slots_per_bank,
                workspaces: self.workspaces,
                resources: self.resources,
                notices: self.notices,
            },
            self.version,
        )
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

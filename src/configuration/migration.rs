use {
    super::{
        Configuration,
        context::Context,
        generation::{BUILD_GENERATION, Generation, OLDEST_READABLE_GENERATION},
        names::GitHubAccount,
        resource::Resource,
        workspace::CargoWorkspace,
    },
    crate::configuration::Notice,
    anyhow::{Context as _, Result},
    serde::Deserialize,
    serde_json::ser::PrettyFormatter,
    std::{
        fmt::Display,
        fs,
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
    workspaces: Vec<CargoWorkspace>,
    #[serde(default)]
    resources: Vec<Resource>,
    #[serde(default)]
    notices: Vec<Notice>,
}

impl From<OutgoingConfiguration> for Configuration {
    fn from(outgoing: OutgoingConfiguration) -> Self {
        Self {
            version: BUILD_GENERATION,
            applies_to: outgoing.applies_to,
            github_account: outgoing.github_account,
            workspaces: outgoing.workspaces,
            resources: outgoing.resources,
            notices: outgoing.notices,
        }
    }
}

impl OutgoingConfiguration {
    pub fn stated_generation(&self) -> Generation {
        self.version
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

    pub fn perform(&self) -> Result<()> {
        fs::write(&self.path, &self.rewritten)
            .with_context(|| format!("Could not rewrite {}", self.path.display()))
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

pub fn announcement(source: &str, from: Generation) -> Notice {
    Notice::from(format!(
        "{source} states generation {from} of dotfiles_configurator and was read as generation \
         {BUILD_GENERATION}. This source cannot be written, so rewrite it there before generation \
         {OLDEST_READABLE_GENERATION} stops being read."
    ))
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

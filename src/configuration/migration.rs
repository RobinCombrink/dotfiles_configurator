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
    serde::{Deserialize, Deserializer},
    serde_json::{Value, ser::PrettyFormatter},
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
    workspaces: Vec<CargoWorkspace>,
    #[serde(default, deserialize_with = "resources_checking_one_path")]
    resources: Vec<Resource>,
    #[serde(default)]
    notices: Vec<DeclaredNotice>,
}

fn resources_checking_one_path<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Resource>, D::Error> {
    Vec::<Value>::deserialize(deserializer)?
        .into_iter()
        .map(|mut resource| {
            path_checks_as_candidates(&mut resource);
            serde_json::from_value(resource).map_err(serde::de::Error::custom)
        })
        .collect()
}

fn path_checks_as_candidates(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            if fields.get("check").and_then(Value::as_str) == Some("path_exists")
                && let Some(path) = fields.remove("path")
            {
                fields.insert("paths".to_owned(), Value::Array(vec![path]));
            }
            fields.values_mut().for_each(path_checks_as_candidates);
        }
        Value::Array(items) => items.iter_mut().for_each(path_checks_as_candidates),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

impl From<OutgoingConfiguration> for Configuration {
    fn from(outgoing: OutgoingConfiguration) -> Self {
        Self {
            version: BUILD_GENERATION,
            applies_to: outgoing.applies_to,
            github_account: outgoing.github_account,
            estate: outgoing.estate,
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

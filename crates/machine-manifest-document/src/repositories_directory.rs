use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

/// ```
/// use std::path::PathBuf;
///
/// use machine_manifest_document::RepositoriesDirectory;
///
/// assert!(RepositoriesDirectory::try_from(PathBuf::from("/repositories/Personal")).is_ok());
/// assert!(RepositoriesDirectory::try_from(PathBuf::from("Repositories")).is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct RepositoriesDirectory(PathBuf);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "the repositories directory {} is not rooted: a relative directory means a different place \
     to each tool that reads it",
    .0.display()
)]
pub struct NotRooted(pub PathBuf);

impl TryFrom<PathBuf> for RepositoriesDirectory {
    type Error = NotRooted;

    fn try_from(directory: PathBuf) -> Result<Self, Self::Error> {
        if !directory.has_root() {
            return Err(NotRooted(directory));
        }
        Ok(Self(directory))
    }
}

impl RepositoriesDirectory {
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RepositoriesDirectory {
    fn deserialize<Input: Deserializer<'de>>(deserializer: Input) -> Result<Self, Input::Error> {
        let directory = PathBuf::deserialize(deserializer)?;
        Self::try_from(directory).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for RepositoriesDirectory {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RepositoriesDirectory".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::RepositoriesDirectory").into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "The directory this machine's class of repositories is cloned \
                            into, rooted at a filesystem root; the directory above it is where \
                            discovery starts.",
        })
    }
}

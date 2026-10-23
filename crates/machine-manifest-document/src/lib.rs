#![cfg_attr(test, allow(clippy::disallowed_macros))]

mod estates;
mod machine_class;
mod repositories_directory;
mod slots_per_bank;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use estates::{Account, EstateName, Estates, EstatesRefused, NotAnEstateName};
pub use machine_class::MachineClass;
pub use repositories_directory::{NotRooted, RepositoriesDirectory};
pub use slots_per_bank::{SlotsPerBank, UnsupportedSlotsPerBank};

pub const MANIFEST_DIRECTORY: &str = ".dotconfig";
pub const MANIFEST_FILE: &str = "machine.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(
        title = "Machine manifest",
        description = "What dotfiles_configurator apply records about this machine, for the tools \
                       that read it."
    )
)]
pub struct Document {
    pub machine: MachineManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MachineManifest {
    pub repositories_directory_path: RepositoriesDirectory,
    pub estates: Estates,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots_per_bank: Option<SlotsPerBank>,
    #[serde(flatten)]
    pub recorded_run: RecordedRun,
}

/// ```
/// use std::path::Path;
///
/// use machine_manifest_document::MachineManifest;
///
/// assert_eq!(
///     MachineManifest::path_within(Path::new("/home/alice")),
///     Path::new("/home/alice").join(".dotconfig").join("machine.json")
/// );
/// ```
impl MachineManifest {
    pub fn path_within(home_directory: &Path) -> PathBuf {
        home_directory.join(MANIFEST_DIRECTORY).join(MANIFEST_FILE)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RecordedRun {
    pub class: MachineClass,
    #[cfg_attr(
        feature = "schema",
        schemars(
            description = "The configuration sources the run that wrote this manifest was given, \
                           as it was given them."
        )
    )]
    pub configuration_sources: Vec<RecordedSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
#[repr(transparent)]
pub struct RecordedSource(String);

impl RecordedSource {
    pub fn as_written(&self) -> &str {
        &self.0
    }
}

impl From<String> for RecordedSource {
    fn from(written: String) -> Self {
        Self(written)
    }
}

#[cfg(test)]
mod tests;

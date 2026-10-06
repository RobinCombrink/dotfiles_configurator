use {
    crate::{
        configuration::{MachineClass, MachineManifest, RecordedRun, RecordedSource},
        configuration_source::{ConfigurationSource, DEFAULT_SOURCE},
        machine::ReadMachine,
    },
    std::{
        fmt::{Display, Formatter},
        path::PathBuf,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRun {
    pub machine: MachineClass,
    pub sources: Vec<ConfigurationSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvingRun {
    Plan,
    Apply,
}

impl ResolvingRun {
    fn naming_no_machine(self) -> &'static str {
        match self {
            ResolvingRun::Plan => "A plan naming no machine plans",
            ResolvingRun::Apply => "An apply naming no machine applies",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnrecordedRun {
    NoManifest {
        run: ResolvingRun,
        manifest: PathBuf,
    },
    ManifestUnreadable {
        manifest: PathBuf,
        cause: String,
    },
    NoRecordedRun {
        manifest: PathBuf,
        cause: String,
    },
    SourceUnreadable {
        manifest: PathBuf,
        cause: String,
    },
}

const NAMING_THE_MACHINE: &str =
    "name the machine with --machine, or apply once so that the run is recorded";

impl Display for UnrecordedRun {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            UnrecordedRun::NoManifest { run, manifest } => write!(
                formatter,
                "{} what the last apply recorded, and this machine holds no manifest at {}; \
                 {NAMING_THE_MACHINE}",
                run.naming_no_machine(),
                manifest.display()
            ),
            UnrecordedRun::ManifestUnreadable { manifest, cause } => write!(
                formatter,
                "The manifest at {} could not be read: {cause}; {NAMING_THE_MACHINE}",
                manifest.display()
            ),
            UnrecordedRun::NoRecordedRun { manifest, cause } => write!(
                formatter,
                "The manifest at {} records no run: {cause}; {NAMING_THE_MACHINE}",
                manifest.display()
            ),
            UnrecordedRun::SourceUnreadable { manifest, cause } => write!(
                formatter,
                "The manifest at {} records a source that cannot be read: {cause}; \
                 {NAMING_THE_MACHINE}",
                manifest.display()
            ),
        }
    }
}

impl std::error::Error for UnrecordedRun {}

impl PlannedRun {
    pub fn resolved(
        run: ResolvingRun,
        named_machine: Option<MachineClass>,
        named_sources: Vec<ConfigurationSource>,
        machine: &impl ReadMachine,
    ) -> Result<Self, UnrecordedRun> {
        if let Some(named) = named_machine {
            let sources = match named_sources.is_empty() {
                true => vec![default_source()],
                false => named_sources,
            };
            return Ok(Self {
                machine: named,
                sources,
            });
        }

        let recorded = Self::recorded_on(run, machine)?;
        match named_sources.is_empty() {
            true => Ok(recorded),
            false => Ok(Self {
                machine: recorded.machine,
                sources: named_sources,
            }),
        }
    }

    fn recorded_on(run: ResolvingRun, machine: &impl ReadMachine) -> Result<Self, UnrecordedRun> {
        let manifest = MachineManifest::path_within(machine.home_directory());
        let document = match machine.text_file_at(&manifest) {
            Ok(Some(document)) => document,
            Ok(None) => return Err(UnrecordedRun::NoManifest { run, manifest }),
            Err(error) => {
                return Err(UnrecordedRun::ManifestUnreadable {
                    manifest,
                    cause: format!("{error:#}"),
                });
            }
        };
        let recorded =
            RecordedRun::read_from(&document).map_err(|error| UnrecordedRun::NoRecordedRun {
                manifest: manifest.clone(),
                cause: error.to_string(),
            })?;
        let sources = recorded
            .configuration_sources
            .iter()
            .map(ConfigurationSource::of_recorded)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|cause| UnrecordedRun::SourceUnreadable {
                manifest: manifest.clone(),
                cause,
            })?;
        Ok(Self {
            machine: recorded.class,
            sources,
        })
    }
}

fn default_source() -> ConfigurationSource {
    ConfigurationSource::of_recorded(&RecordedSource::from(DEFAULT_SOURCE.to_owned()))
        .expect("the default source is a GitHub source that parses")
}

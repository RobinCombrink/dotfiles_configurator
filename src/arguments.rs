use {
    crate::{
        configuration::MachineClass,
        configuration_source::{AbsoluteDirectory, ConfigurationSource},
        currency::this_build,
        machine::{ElevatedBatch, local::elevation},
        version::Version,
    },
    clap::{Args, Parser, Subcommand},
    std::{ffi::OsString, path::PathBuf},
};

fn source_named_in_the_working_directory(value: &str) -> Result<ConfigurationSource, String> {
    let working_directory = std::env::current_dir()
        .map_err(|failure| format!("{value:?} cannot be resolved: {failure}"))?;
    let working_directory = AbsoluteDirectory::of(working_directory).ok_or_else(|| {
        format!("{value:?} cannot be resolved against a working directory that is not absolute")
    })?;
    ConfigurationSource::named(value, &working_directory)
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct RunArguments {
    #[arg(
        short = 'm',
        long = "machine",
        value_name = "MACHINE",
        help = "Which class of machine this is — `personal` or `work`. A configuration applies \
                when it declares this class, or `everywhere`. Named nowhere, the class the last \
                apply recorded in the machine manifest is used."
    )]
    pub machine: Option<MachineClass>,
    #[arg(
        short = 's',
        long = "source",
        value_name = "SOURCE",
        value_parser = source_named_in_the_working_directory,
        help = "Where to read configurations from, as `local:<directory>` or \
                `github:<owner>/<repo>/<directory>`. Repeatable; read in the order given. Named \
                nowhere, the sources the last apply recorded are read, or the default source \
                where a machine is named."
    )]
    pub sources: Vec<ConfigurationSource>,
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct ApplyArguments {
    #[command(flatten)]
    pub run: RunArguments,
    #[arg(
        long = "yes",
        num_args = 0,
        help = "Enact the change set without being asked to confirm it. The change set is \
                printed either way, and a run with no terminal to ask at needs this."
    )]
    pub yes: bool,
    #[arg(
        long = "replaced",
        hide = true,
        value_name = "VERSION",
        value_parser = version_named,
        help = "The build this one replaced earlier in the same run, which it carries on from."
    )]
    pub replaced: Option<Version>,
}

fn version_named(spelled: &str) -> Result<Version, String> {
    Version::try_from(spelled)
}

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct ElevatedBatchArguments {
    #[arg(
        long = elevation::BATCH,
        value_parser = batch_named,
        help = "The entries to settle, as the apply that asked for elevation wrote them."
    )]
    pub batch: ElevatedBatch<()>,
    #[arg(
        long = elevation::RESULTS,
        help = "Where to write what each entry settled to, for the apply that asked to read."
    )]
    pub results: PathBuf,
}

fn batch_named(written: &str) -> Result<ElevatedBatch<()>, String> {
    serde_json::from_str(written).map_err(|fault| format!("not an elevated batch: {fault}"))
}

#[derive(Subcommand, Debug)]
pub enum Task {
    #[command(
        about = "Report the change set that would close every drift, without touching the machine"
    )]
    Plan(RunArguments),
    #[command(about = "Show the change set, ask once, then enact it until a pass changes nothing")]
    Apply(ApplyArguments),
    #[command(
        name = elevation::ELEVATED_BATCH,
        hide = true,
        about = "Settle the entries an apply was refused for want of elevation, elevated"
    )]
    ElevatedBatch(ElevatedBatchArguments),
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    #[command(subcommand)]
    pub task: Task,
    #[arg(
        global = true,
        long = "debug",
        num_args = 0,
        help = "Enable debug logging"
    )]
    pub debug: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarriedAnswer {
    ChangeSetConfirmed,
    Nothing,
}

pub fn successor_arguments(
    own_arguments: impl IntoIterator<Item = OsString>,
    arguments: &ApplyArguments,
    carried: CarriedAnswer,
) -> Vec<OsString> {
    let mut successor: Vec<OsString> = own_arguments.into_iter().collect();
    successor.push(OsString::from("--replaced"));
    successor.push(OsString::from(this_build().to_string()));
    if carried == CarriedAnswer::ChangeSetConfirmed && !arguments.yes {
        successor.push(OsString::from("--yes"));
    }
    successor
}

#[cfg(test)]
mod tests;

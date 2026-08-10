use {
    anyhow::Result,
    clap::{Args, Parser, Subcommand},
    dotfiles_configurator::{
        configuration::MachineClass,
        configuration_source::{ConfigurationSource, load_desired_state},
        convergence::{apply::apply, plan},
        desired_state::DesiredState,
        machine::local::LocalMachine,
        reporting::{RunKind, RunReport},
    },
    log::{LevelFilter, trace},
    std::{io::Write, path::PathBuf, process::ExitCode},
};

#[cfg(test)]
use std::str::FromStr;

/// Where configurations are read from when none is named.
const DEFAULT_SOURCE: &str = "github:RobinCombrink/dotfiles/config";

#[derive(Args, Debug, Clone, PartialEq, Eq)]
struct ConfigurationArguments {
    #[arg(
        short = 'm',
        long = "machine",
        value_name = "MACHINE",
        help = "Which class of machine this is — `personal` or `work`. A configuration applies \
                when it declares this class, or `everywhere`."
    )]
    machine: MachineClass,
    #[arg(
        short = 's',
        long = "source",
        value_name = "SOURCE",
        default_value = DEFAULT_SOURCE,
        help = "Where to read configurations from, as `local:<directory>` or \
                `github:<owner>/<repo>/<directory>`. Repeatable; read in the order given."
    )]
    sources: Vec<ConfigurationSource>,
}

#[derive(Subcommand, Debug)]
enum Task {
    /// Report the change set that would close every drift, without touching the machine.
    Plan(ConfigurationArguments),
    /// Enact the change set, repeating until a pass changes nothing.
    Apply(ConfigurationArguments),
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Arguments {
    #[command(subcommand)]
    task: Task,
    #[arg(
        global = true,
        long = "debug",
        num_args = 0,
        help = "Enable debug logging"
    )]
    debug: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = Arguments::parse();

    match arguments.debug {
        true => setup_logging(LevelFilter::Info),
        false => setup_logging(LevelFilter::Error),
    }
    trace!("Logging setup successful");

    match run(arguments.task).await {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(task: Task) -> Result<ExitCode> {
    match task {
        Task::Plan(arguments) => {
            let report = RunReport::open(RunKind::Plan)?;
            let (desired_state, machine) = prepare(&arguments, &report).await?;
            let change_set = plan(&desired_state, &machine, &report).await?;
            println!("{change_set}");
            Ok(exit_code_for(change_set.is_converged()))
        }
        Task::Apply(arguments) => {
            let report = RunReport::open(RunKind::Apply)?;
            let (desired_state, machine) = prepare(&arguments, &report).await?;
            let outcome = apply(&desired_state, &machine, &report).await?;
            println!("{outcome}");
            Ok(exit_code_for(outcome.is_converged()))
        }
    }
}

async fn prepare<'report>(
    arguments: &ConfigurationArguments,
    report: &'report RunReport,
) -> Result<(DesiredState, LocalMachine<'report>)> {
    let desired_state =
        load_desired_state(&arguments.sources, arguments.machine, &repositories_root()?).await?;
    let machine = LocalMachine::new(desired_state.account().clone(), report)?;
    Ok((desired_state, machine))
}

// ADR 0025
#[cfg(target_family = "windows")]
fn repositories_root() -> Result<PathBuf> {
    Ok(PathBuf::from("C:\\Repositories"))
}

#[cfg(target_family = "unix")]
fn repositories_root() -> Result<PathBuf> {
    std::env::home_dir()
        .map(|home| home.join("Repositories"))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not find the home directory to resolve the repositories root against"
            )
        })
}

/// A run that leaves the machine unconverged exits non-zero, whether that is because something
/// drifted, failed, or could not be read at all. See ADR 0004.
fn exit_code_for(converged: bool) -> ExitCode {
    match converged {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    }
}

fn setup_logging(level_filter: LevelFilter) {
    env_logger::builder()
        .filter_level(level_filter)
        .parse_default_env()
        .format(|buffer, record| {
            writeln!(
                buffer,
                "{}:{} {} [{}] - {}",
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                chrono::Local::now().format("%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> ConfigurationArguments {
        let parsed = Arguments::try_parse_from(
            std::iter::once("dotfiles_configurator").chain(arguments.iter().copied()),
        )
        .unwrap();
        match parsed.task {
            Task::Plan(configuration) | Task::Apply(configuration) => configuration,
        }
    }

    fn sources_from(arguments: &[&str]) -> Vec<ConfigurationSource> {
        parse(arguments).sources
    }

    #[test]
    fn naming_a_directory_reads_that_directory_and_nothing_else() {
        assert_eq!(
            sources_from(&["plan", "--machine", "personal", "--source", "local:config"]),
            vec![ConfigurationSource::LocalDirectory("config".into())]
        );
    }

    #[test]
    fn naming_no_source_reads_the_default_one() {
        assert_eq!(
            sources_from(&["plan", "--machine", "personal"]),
            vec![ConfigurationSource::from_str(DEFAULT_SOURCE).unwrap()]
        );
    }

    #[test]
    fn sources_are_read_in_the_order_they_were_named() {
        assert_eq!(
            sources_from(&[
                "plan",
                "--machine",
                "personal",
                "--source",
                "github:Alice/dotfiles/config",
                "--source",
                "local:config",
            ]),
            vec![
                ConfigurationSource::GitHubRepository {
                    owner: "Alice".into(),
                    repository: "dotfiles".into(),
                    directory: "config".to_owned(),
                },
                ConfigurationSource::LocalDirectory("config".into()),
            ]
        );
    }

    #[test]
    fn a_windows_directory_keeps_the_colon_in_its_drive_letter() {
        assert_eq!(
            ConfigurationSource::from_str("local:C:\\Repositories\\dotfiles\\config").unwrap(),
            ConfigurationSource::LocalDirectory("C:\\Repositories\\dotfiles\\config".into())
        );
    }

    #[test]
    fn a_source_naming_no_kind_is_rejected_with_the_shapes_it_expected() {
        let error = ConfigurationSource::from_str("config").unwrap_err();

        assert!(
            error.contains("local:") && error.contains("github:"),
            "{error}"
        );
    }

    #[test]
    fn the_machine_named_decides_which_configurations_apply() {
        assert_eq!(
            parse(&["plan", "--machine", "work"]).machine,
            MachineClass::Work
        );
    }

    #[test]
    fn an_invocation_naming_no_machine_is_refused() {
        assert!(Arguments::try_parse_from(["dotfiles_configurator", "plan"]).is_err());
    }
}

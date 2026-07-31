use {
    anyhow::Result,
    clap::{Args, Parser, Subcommand},
    dotfiles::{
        configuration::DesiredState,
        configuration_source::{ConfigurationSource, load_desired_state},
        convergence::{apply::apply, plan},
        machine::local::LocalMachine,
    },
    log::{LevelFilter, trace},
    std::{io::Write, process::ExitCode},
};

#[cfg(test)]
use std::str::FromStr;

/// Where configurations are read from when none is named.
const DEFAULT_SOURCE: &str =
    "github:RobinCombrink/dotfiles/config/everywhere.dotconfig.json,config/personal.dotconfig.json";

#[derive(Args, Debug, Clone, PartialEq, Eq)]
struct SourceArguments {
    /// Where to read configurations from, as `local:<directory>` or
    /// `github:<owner>/<repo>/<path>[,<path>...]`. Repeatable; read in the order given.
    #[arg(
        short = 's',
        long = "source",
        value_name = "SOURCE",
        default_value = DEFAULT_SOURCE,
    )]
    sources: Vec<ConfigurationSource>,
}

#[derive(Subcommand, Debug)]
enum Task {
    /// Report the change set that would close every drift, without touching the machine.
    Plan(SourceArguments),
    /// Enact the change set, repeating until a pass changes nothing.
    Apply(SourceArguments),
}

#[derive(Parser, Debug)]
#[clap(name = "Dotfiles")]
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
            let (desired_state, machine) = prepare(&arguments).await?;
            let change_set = plan(&desired_state, &machine);
            println!("{change_set}");
            Ok(exit_code_for(change_set.is_converged()))
        }
        Task::Apply(arguments) => {
            let (desired_state, machine) = prepare(&arguments).await?;
            let outcome = apply(&desired_state, &machine).await;
            println!("{outcome}");
            Ok(exit_code_for(outcome.is_converged()))
        }
    }
}

async fn prepare(arguments: &SourceArguments) -> Result<(DesiredState, LocalMachine)> {
    let desired_state = load_desired_state(&arguments.sources).await?;
    let machine = LocalMachine::new(&desired_state.machine)?;
    Ok((desired_state, machine))
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

    fn sources_from(arguments: &[&str]) -> Vec<ConfigurationSource> {
        let parsed =
            Arguments::try_parse_from(std::iter::once("dotfiles").chain(arguments.iter().copied()))
                .unwrap();
        match parsed.task {
            Task::Plan(source) | Task::Apply(source) => source.sources,
        }
    }

    #[test]
    fn naming_a_directory_reads_that_directory_and_nothing_else() {
        assert_eq!(
            sources_from(&["plan", "--source", "local:config"]),
            vec![ConfigurationSource::LocalDirectory("config".into())]
        );
    }

    #[test]
    fn naming_no_source_reads_the_default_one() {
        assert_eq!(
            sources_from(&["plan"]),
            vec![ConfigurationSource::from_str(DEFAULT_SOURCE).unwrap()]
        );
    }

    #[test]
    fn sources_are_read_in_the_order_they_were_named() {
        assert_eq!(
            sources_from(&[
                "plan",
                "--source",
                "github:Alice/dotfiles/config/one.json",
                "--source",
                "local:config",
            ]),
            vec![
                ConfigurationSource::GitHubRepository {
                    owner: "Alice".into(),
                    repository: "dotfiles".into(),
                    file_paths: vec!["config/one.json".to_owned()],
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
}

use {
    anyhow::{Result, anyhow},
    clap::{Args, Parser, Subcommand},
    dotfiles::{
        configuration::DesiredState,
        configuration_source::{ConfigurationSource, load_desired_state},
        convergence::{apply::apply, plan},
        machine::local::LocalMachine,
    },
    log::{LevelFilter, trace},
    std::{io::Write, path::PathBuf, process::ExitCode, str::FromStr},
};

#[derive(Args, Debug, Clone, PartialEq, Eq)]
struct SourceArguments {
    /// A directory of configuration files to read.
    #[arg(short = 'd', long = "directory")]
    directory: Option<PathBuf>,
    /// A GitHub repository to read configurations from, as `owner,repo,path;path`.
    #[arg(
        short = 'r',
        long = "remote",
        default_value = "RobinCombrink,dotfiles,config/everywhere.dotconfig.json;config/personal.dotconfig.json"
    )]
    remote: Option<RemoteConfiguration>,
}

impl SourceArguments {
    fn sources(&self) -> Vec<ConfigurationSource> {
        let mut sources = Vec::new();
        if let Some(directory) = &self.directory {
            sources.push(ConfigurationSource::LocalDirectory(directory.clone()));
        }
        if let Some(remote) = &self.remote {
            sources.push(ConfigurationSource::GitHubRepository {
                owner: remote.owner.clone(),
                repo: remote.repo.clone(),
                file_paths: remote.file_paths.clone(),
            });
        }
        sources
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteConfiguration {
    owner: String,
    repo: String,
    file_paths: Vec<String>,
}

impl FromStr for RemoteConfiguration {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = value.split(',').collect();
        let [owner, repo, file_paths] = parts.as_slice() else {
            return Err("Expected `owner,repo,path;path`".to_owned());
        };

        Ok(RemoteConfiguration {
            owner: (*owner).to_owned(),
            repo: (*repo).to_owned(),
            file_paths: file_paths.split(';').map(str::to_owned).collect(),
        })
    }
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
    let sources = arguments.sources();
    if sources.is_empty() {
        return Err(anyhow!(
            "No configuration source was given. Pass --directory, --remote, or both."
        ));
    }

    let desired_state = load_desired_state(&sources).await?;
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

    #[test]
    fn a_remote_is_read_as_an_owner_a_repository_and_semicolon_separated_paths() {
        let remote =
            RemoteConfiguration::from_str("Alice,dotfiles,config/one.json;config/two.json")
                .unwrap();

        assert_eq!(
            remote,
            RemoteConfiguration {
                owner: "Alice".to_owned(),
                repo: "dotfiles".to_owned(),
                file_paths: vec!["config/one.json".to_owned(), "config/two.json".to_owned()],
            }
        );
    }

    #[test]
    fn a_remote_missing_a_field_is_rejected_with_the_shape_it_expected() {
        let error = RemoteConfiguration::from_str("Alice,dotfiles").unwrap_err();

        assert!(error.contains("owner,repo,path"));
    }

    #[test]
    fn a_directory_and_a_remote_given_together_are_both_read() {
        let arguments = SourceArguments {
            directory: Some("config".into()),
            remote: Some(RemoteConfiguration::from_str("Alice,dotfiles,config/one.json").unwrap()),
        };

        assert_eq!(arguments.sources().len(), 2);
    }
}

use {
    anyhow::{Result, anyhow, bail},
    clap::{Args, Parser, Subcommand},
    dotfiles_configurator::{
        configuration::{GitHubAccount, MachineClass},
        configuration_source::{ConfigurationSource, LoadFailure, load_desired_state},
        confirmation::{Confirm, Confirmation, Operator},
        convergence::{
            apply::{Enactment, apply},
            install_release, plan,
        },
        currency::{RELEASE_OWNER, own_currency, own_release_repository},
        desired_state::DesiredState,
        github::GitHubAccess,
        machine::{Placement, ReadMachine, local::LocalMachine},
        reporting::{RunKind, RunReport},
    },
    log::{LevelFilter, trace},
    std::{
        io::Write,
        path::{Path, PathBuf},
        process::ExitCode,
    },
};

#[cfg(test)]
use {dotfiles_configurator::configuration::GitHubRepository, std::str::FromStr};

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

#[derive(Args, Debug, Clone, PartialEq, Eq)]
struct ApplyArguments {
    #[command(flatten)]
    configuration: ConfigurationArguments,
    #[arg(
        long = "yes",
        num_args = 0,
        help = "Enact the change set without being asked to confirm it. The change set is \
                printed either way, and a run with no terminal to ask at needs this."
    )]
    yes: bool,
}

#[derive(Subcommand, Debug)]
enum Task {
    /// Report the change set that would close every drift, without touching the machine.
    Plan(ConfigurationArguments),
    /// Show the change set, ask once, then enact it until a pass changes nothing.
    Apply(ApplyArguments),
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
    let github = GitHubAccess::new();
    match task {
        Task::Plan(arguments) => {
            let report = RunReport::open(RunKind::Plan)?;
            let desired_state = load(&arguments, &github, &repositories_root()?).await?;
            let machine = LocalMachine::new(&report, &github)?;
            let (change_set, _) = plan(&desired_state, &machine, &report).await?;
            println!("{change_set}");
            Ok(Conclusion::of(change_set.is_converged()).into())
        }
        Task::Apply(arguments) => {
            let report = RunReport::open(RunKind::Apply)?;
            let operator = match Operator::of_this_run(arguments.yes) {
                Ok(operator) => operator,
                Err(refusal) => {
                    report.announce(&refusal.to_string());
                    return Ok(Conclusion::DidNothing.into());
                }
            };
            let machine = LocalMachine::new(&report, &github)?;
            let desired_state = match load_after_updating_if_it_must(
                &arguments.configuration,
                &machine,
                &report,
                &github,
                &operator,
            )
            .await?
            {
                Loaded::Read(desired_state) => desired_state,
                Loaded::DeclinedTheNewerBuild => return Ok(Conclusion::DidNothing.into()),
            };

            match apply(&desired_state, &machine, &report, &operator).await? {
                Enactment::Enacted(outcome) => {
                    println!("{outcome}");
                    Ok(Conclusion::of(outcome.is_converged()).into())
                }
                Enactment::Declined => Ok(Conclusion::DidNothing.into()),
            }
        }
    }
}

async fn load(
    arguments: &ConfigurationArguments,
    github: &GitHubAccess,
    repositories_root: &Path,
) -> Result<DesiredState, LoadFailure> {
    load_desired_state(
        &arguments.sources,
        arguments.machine,
        repositories_root,
        github,
    )
    .await
}

/// A configuration stating a generation above this build cannot be read, and a resource declaring
/// where a newer build comes from would be inside it — so the origin is carried instead. Apply
/// obtains a newer build once and reads again; a newest release that still does not meet the floor
/// ends the run saying so rather than trying again. See ADR 0019.
async fn load_after_updating_if_it_must(
    arguments: &ConfigurationArguments,
    machine: &LocalMachine<'_, '_>,
    report: &RunReport,
    github: &GitHubAccess,
    operator: &impl Confirm,
) -> Result<Loaded> {
    let repositories_root = repositories_root()?;
    let refusal = match load(arguments, github, &repositories_root).await {
        Ok(desired_state) => return Ok(Loaded::Read(desired_state)),
        Err(refusal) => refusal,
    };

    if !refusal.is_answered_by_a_newer_build() {
        return Err(refusal.into());
    }

    report.announce(&format!(
        "{refusal}
A newer build from {} can read it, and installing it replaces the one running now.",
        own_release_repository()
    ));

    if operator.confirmation(OBTAIN_A_NEWER_BUILD) == Confirmation::Declined {
        report.announce("Declined. Nothing on this machine was changed.");
        return Ok(Loaded::DeclinedTheNewerBuild);
    }

    obtain_a_newer_build(machine).await?;
    Ok(Loaded::Read(
        load(arguments, github, &repositories_root).await?,
    ))
}

const OBTAIN_A_NEWER_BUILD: &str = "Obtain the newer build and read again?";

enum Loaded {
    Read(DesiredState),
    DeclinedTheNewerBuild,
}

async fn obtain_a_newer_build(machine: &LocalMachine<'_, '_>) -> Result<()> {
    let binary = own_currency();
    let released = machine
        .latest_release(&binary.repository, &GitHubAccount::from(RELEASE_OWNER))
        .await?
        .ok_or_else(|| anyhow!("{} has published no release", binary.repository))?;

    match install_release(&binary, &released, machine).await? {
        Placement::Placed => Ok(()),
        Placement::Held(path) => bail!(
            "{} is running and could not be moved aside to install the newer build",
            path.display()
        ),
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Conclusion {
    Converged,
    Unconverged,
    DidNothing,
}

impl Conclusion {
    fn of(converged: bool) -> Self {
        match converged {
            true => Conclusion::Converged,
            false => Conclusion::Unconverged,
        }
    }

    // ADR 0004, ADR 0013
    fn status(self) -> u8 {
        match self {
            Conclusion::Converged => 0,
            Conclusion::Unconverged => 1,
            Conclusion::DidNothing => 2,
        }
    }
}

impl From<Conclusion> for ExitCode {
    fn from(conclusion: Conclusion) -> Self {
        ExitCode::from(conclusion.status())
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
            Task::Plan(configuration) => configuration,
            Task::Apply(apply) => apply.configuration,
        }
    }

    fn apply_arguments(arguments: &[&str]) -> ApplyArguments {
        let parsed = Arguments::try_parse_from(
            std::iter::once("dotfiles_configurator").chain(arguments.iter().copied()),
        )
        .unwrap();
        match parsed.task {
            Task::Apply(apply) => apply,
            Task::Plan(_) => panic!("the arguments named plan rather than apply"),
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
                    repository: GitHubRepository {
                        owner: "Alice".into(),
                        repository: "dotfiles".into(),
                    },
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

    #[test]
    fn an_apply_is_asked_to_confirm_unless_it_was_answered_in_advance() {
        assert!(!apply_arguments(&["apply", "--machine", "personal"]).yes);
        assert!(apply_arguments(&["apply", "--machine", "personal", "--yes"]).yes);
    }

    #[test]
    fn a_plan_has_nothing_to_answer_in_advance_and_refuses_the_flag() {
        assert!(
            Arguments::try_parse_from(["dotfiles_configurator", "plan", "-m", "personal", "--yes"])
                .is_err()
        );
    }

    #[test]
    fn a_converged_machine_exits_zero_and_an_unconverged_one_exits_non_zero() {
        assert_eq!(Conclusion::of(true).status(), 0);
        assert_eq!(Conclusion::of(false).status(), 1);
    }

    #[test]
    fn a_run_that_changed_nothing_exits_as_neither_converged_nor_failed() {
        assert_eq!(Conclusion::DidNothing.status(), 2);
    }
}

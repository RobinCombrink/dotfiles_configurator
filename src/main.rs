use {
    anyhow::{Context, Result, anyhow, bail},
    clap::{Args, Parser, Subcommand},
    dotfiles_configurator::{
        configuration::{GitHubAccount, MachineClass},
        configuration_source::{
            AbsoluteDirectory, ConfigurationSource, LoadFailure, load_desired_state,
        },
        confirmation::{Confirm, Confirmation, Operator},
        convergence::{
            apply::{Enactment, apply},
            install_release, plan,
        },
        currency::{
            RELEASE_OWNER, SelfReplacement, own_currency, own_release_repository, this_build,
        },
        desired_state::DesiredState,
        github::GitHubAccess,
        machine::{Placement, ReadMachine, local::LocalMachine},
        reporting::{RunKind, RunReport},
        version::Version,
    },
    log::{LevelFilter, trace},
    std::{
        ffi::OsString,
        fmt::Display,
        io::Write,
        path::{Path, PathBuf},
        process::ExitCode,
    },
};

#[cfg(test)]
use dotfiles_configurator::configuration::GitHubRepository;

/// Where configurations are read from when none is named.
const DEFAULT_SOURCE: &str = "github:RobinCombrink/dotfiles/config";

fn source_named_in_the_working_directory(value: &str) -> Result<ConfigurationSource, String> {
    let working_directory = std::env::current_dir()
        .map_err(|failure| format!("{value:?} cannot be resolved: {failure}"))?;
    let working_directory = AbsoluteDirectory::of(working_directory).ok_or_else(|| {
        format!("{value:?} cannot be resolved against a working directory that is not absolute")
    })?;
    ConfigurationSource::named(value, &working_directory)
}

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
        value_parser = source_named_in_the_working_directory,
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
    #[arg(
        long = "replaced",
        hide = true,
        value_name = "VERSION",
        value_parser = version_named,
        help = "The build this one replaced earlier in the same run, which it carries on from."
    )]
    replaced: Option<Version>,
}

fn version_named(spelled: &str) -> Result<Version, String> {
    Version::try_from(spelled)
}

#[derive(Subcommand, Debug)]
enum Task {
    #[command(
        about = "Report the change set that would close every drift, without touching the machine"
    )]
    Plan(ConfigurationArguments),
    #[command(about = "Show the change set, ask once, then enact it until a pass changes nothing")]
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
        Ok(Ending::Concluded(conclusion)) => conclusion.into(),
        Ok(Ending::HandedOver { exit_code }) => std::process::exit(exit_code),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

enum Ending {
    Concluded(Conclusion),
    HandedOver { exit_code: i32 },
}

async fn run(task: Task) -> Result<Ending> {
    let github = GitHubAccess::new();
    match task {
        Task::Plan(arguments) => {
            let report = RunReport::open(RunKind::Plan)?;
            let desired_state = load(&arguments, &github, &repositories_root()?).await?;
            let machine = LocalMachine::new(&report, &github)?;
            let (change_set, _) = plan(&desired_state, &machine, &report).await?;
            println!("{change_set}");
            Ok(Ending::Concluded(Conclusion::of(change_set.is_converged())))
        }
        Task::Apply(arguments) => {
            let report = RunReport::open(RunKind::Apply)?;
            let operator = match Operator::of_this_run(arguments.yes) {
                Ok(operator) => operator,
                Err(refusal) => {
                    report.announce(&refusal.to_string());
                    return Ok(Ending::Concluded(Conclusion::DidNothing));
                }
            };
            let machine = LocalMachine::new(&report, &github)?;
            let desired_state = match load_after_updating_if_it_must(
                &arguments, &machine, &report, &github, &operator,
            )
            .await?
            {
                Loaded::Read(desired_state) => desired_state,
                Loaded::DeclinedTheNewerBuild => {
                    return Ok(Ending::Concluded(Conclusion::DidNothing));
                }
                Loaded::ObtainedANewerBuild => {
                    return hand_over_to_the_installed_build(
                        &machine,
                        successor_arguments(
                            std::env::args_os().skip(1),
                            &arguments,
                            CarriedAnswer::Nothing,
                        ),
                    );
                }
            };

            let self_replacement = SelfReplacement::from(arguments.replaced.clone());
            match apply(
                &desired_state,
                &machine,
                &report,
                &operator,
                &self_replacement,
            )
            .await?
            {
                Enactment::Enacted(outcome) => {
                    println!("{outcome}");
                    Ok(Ending::Concluded(Conclusion::of(outcome.is_converged())))
                }
                Enactment::Declined => Ok(Ending::Concluded(Conclusion::DidNothing)),
                Enactment::ReplacedItself => hand_over_to_the_installed_build(
                    &machine,
                    successor_arguments(
                        std::env::args_os().skip(1),
                        &arguments,
                        CarriedAnswer::ChangeSetConfirmed,
                    ),
                ),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CarriedAnswer {
    ChangeSetConfirmed,
    Nothing,
}

fn successor_arguments(
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

fn hand_over_to_the_installed_build(
    machine: &LocalMachine<'_, '_>,
    successor_arguments: Vec<OsString>,
) -> Result<Ending> {
    let installed = machine
        .binaries_directory()
        .join(own_currency().installed_name().file_name());
    let status = std::process::Command::new(&installed)
        .args(successor_arguments)
        .status()
        .with_context(|| format!("Could not start {} to carry on", installed.display()))?;

    status
        .code()
        .map(|exit_code| Ending::HandedOver { exit_code })
        .ok_or_else(|| anyhow!("{} ended without an exit status", installed.display()))
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

// ADR 0019
async fn load_after_updating_if_it_must(
    arguments: &ApplyArguments,
    machine: &LocalMachine<'_, '_>,
    report: &RunReport,
    github: &GitHubAccess,
    operator: &impl Confirm,
) -> Result<Loaded> {
    let repositories_root = repositories_root()?;
    let refusal = match load(&arguments.configuration, github, &repositories_root).await {
        Ok(desired_state) => return Ok(Loaded::Read(desired_state)),
        Err(refusal) => refusal,
    };

    if !refusal.is_answered_by_a_newer_build() {
        return Err(refusal.into());
    }

    if let Some(replaced) = &arguments.replaced {
        bail!(
            "{}",
            still_unreadable_by_the_newest_build(&refusal, replaced)
        );
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
    Ok(Loaded::ObtainedANewerBuild)
}

fn still_unreadable_by_the_newest_build(refusal: &impl Display, replaced: &Version) -> String {
    format!(
        "{refusal}
{} replaced {replaced} earlier in this run and cannot read it either, so no release of {} does \
         yet.",
        this_build(),
        own_release_repository()
    )
}

const OBTAIN_A_NEWER_BUILD: &str = "Obtain the newer build and read again?";

enum Loaded {
    Read(DesiredState),
    DeclinedTheNewerBuild,
    ObtainedANewerBuild,
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

    fn working_directory() -> AbsoluteDirectory {
        AbsoluteDirectory::of(std::env::current_dir().unwrap())
            .expect("the working directory is absolute")
    }

    fn under_the_working_directory(directory: &str) -> ConfigurationSource {
        ConfigurationSource::LocalDirectory(
            AbsoluteDirectory::of(std::env::current_dir().unwrap().join(directory))
                .expect("the working directory is absolute"),
        )
    }

    #[test]
    fn naming_a_directory_reads_that_directory_under_the_working_directory_and_nothing_else() {
        assert_eq!(
            sources_from(&["plan", "--machine", "personal", "--source", "local:config"]),
            vec![under_the_working_directory("config")]
        );
    }

    #[test]
    fn naming_no_source_reads_the_default_one() {
        assert_eq!(
            sources_from(&["plan", "--machine", "personal"]),
            vec![ConfigurationSource::named(DEFAULT_SOURCE, &working_directory()).unwrap()]
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
                under_the_working_directory("config"),
            ]
        );
    }

    #[test]
    fn a_windows_directory_keeps_the_colon_in_its_drive_letter() {
        let source = ConfigurationSource::named(
            "local:C:\\Repositories\\dotfiles\\config",
            &working_directory(),
        )
        .unwrap();

        assert!(
            source
                .to_string()
                .ends_with("C:\\Repositories\\dotfiles\\config"),
            "{source}"
        );
    }

    #[test]
    fn a_source_naming_no_kind_is_rejected_with_the_shapes_it_expected() {
        let error = ConfigurationSource::named("config", &working_directory()).unwrap_err();

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

    fn successor_of(own_arguments: &[&str]) -> Vec<OsString> {
        successor_carrying(own_arguments, CarriedAnswer::ChangeSetConfirmed)
    }

    fn successor_carrying(own_arguments: &[&str], carried: CarriedAnswer) -> Vec<OsString> {
        successor_arguments(
            own_arguments.iter().map(OsString::from),
            &apply_arguments(own_arguments),
            carried,
        )
    }

    fn parsed_successor_of(own_arguments: &[&str]) -> ApplyArguments {
        parsed(successor_of(own_arguments))
    }

    fn parsed(successor: Vec<OsString>) -> ApplyArguments {
        match Arguments::try_parse_from(
            std::iter::once(OsString::from("dotfiles_configurator")).chain(successor),
        )
        .unwrap()
        .task
        {
            Task::Apply(apply) => apply,
            Task::Plan(_) => panic!("the successor was started to plan rather than to apply"),
        }
    }

    #[test]
    fn the_build_that_replaced_a_run_is_started_with_the_arguments_that_run_was_given() {
        let successor =
            parsed_successor_of(&["apply", "--machine", "work", "--source", "local:config"]);

        assert_eq!(
            successor.configuration,
            parse(&["apply", "--machine", "work", "--source", "local:config"])
        );
    }

    #[test]
    fn the_build_that_replaced_a_run_is_not_asked_to_confirm_the_change_set_again() {
        assert!(parsed_successor_of(&["apply", "--machine", "personal"]).yes);
    }

    #[test]
    fn a_run_answered_in_advance_hands_its_successor_the_answer_once() {
        let successor = successor_of(&["apply", "--machine", "personal", "--yes"]);

        assert_eq!(
            successor
                .iter()
                .filter(|argument| *argument == "--yes")
                .count(),
            1,
            "{successor:?}"
        );
    }

    #[test]
    fn the_build_that_replaced_a_run_may_not_replace_itself_again() {
        assert_eq!(
            SelfReplacement::from(
                parsed_successor_of(&["apply", "--machine", "personal"]).replaced
            ),
            SelfReplacement::Spent {
                replaced: this_build()
            }
        );
    }

    #[test]
    fn a_build_obtained_to_read_a_newer_configuration_still_asks_to_enact_its_change_set() {
        assert!(
            !parsed(successor_carrying(
                &["apply", "--machine", "personal"],
                CarriedAnswer::Nothing
            ))
            .yes
        );
    }

    #[test]
    fn a_build_obtained_to_read_a_newer_configuration_may_not_obtain_another() {
        assert_eq!(
            SelfReplacement::from(
                parsed(successor_carrying(
                    &["apply", "--machine", "personal"],
                    CarriedAnswer::Nothing
                ))
                .replaced
            ),
            SelfReplacement::Spent {
                replaced: this_build()
            }
        );
    }

    #[test]
    fn a_configuration_the_newest_build_cannot_read_either_names_the_build_it_replaced() {
        let message = still_unreadable_by_the_newest_build(
            &"personal.dotconfig.json needs generation 99",
            &Version::try_from("3.18.0").unwrap(),
        );

        assert!(message.contains("replaced 3.18.0"), "{message}");
    }

    #[test]
    fn a_run_nothing_replaced_may_replace_itself() {
        assert_eq!(
            SelfReplacement::from(apply_arguments(&["apply", "--machine", "personal"]).replaced),
            SelfReplacement::Available
        );
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

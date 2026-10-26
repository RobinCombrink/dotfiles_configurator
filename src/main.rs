use {
    anyhow::{Context, Result, anyhow},
    clap::Parser,
    dotfiles_configurator::{
        arguments::{Arguments, CarriedAnswer, Task, successor_arguments},
        conclusion::Conclusion,
        configuration_source::load_desired_state,
        confirmation::Operator,
        convergence::{
            apply::{Enactment, apply},
            plan,
        },
        currency::{SelfReplacement, own_currency},
        github::GitHubAccess,
        machine::{
            ReadMachine,
            local::{LocalMachine, elevation},
        },
        newer_build::{Loaded, load_after_updating_if_it_must},
        planned_run::{PlannedRun, ResolvingRun},
        reporting::{RunKind, RunReport},
        tool_directory_within,
    },
    log::{LevelFilter, trace},
    std::{ffi::OsString, io::Write, path::PathBuf, process::ExitCode},
};

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
    let tool_directory = tool_directory_within(
        &std::env::home_dir().ok_or_else(|| anyhow!("Could not find the home directory"))?,
    );
    match task {
        Task::Plan(arguments) => {
            let report = RunReport::open(&tool_directory, RunKind::Plan)?;
            let machine = LocalMachine::new(&report, &github, tool_directory.clone())?;
            let planned = PlannedRun::resolved(
                ResolvingRun::Plan,
                arguments.machine,
                arguments.sources,
                &machine,
            )?;
            let desired_state = load_desired_state(
                &planned.sources,
                planned.machine,
                &repositories_root()?,
                &github,
            )
            .await?;
            let (change_set, _) = plan(&desired_state, &machine, &report).await?;
            println!("{change_set}");
            Ok(Ending::Concluded(Conclusion::of(change_set.is_converged())))
        }
        Task::Apply(arguments) => {
            let report = RunReport::open(&tool_directory, RunKind::Apply)?;
            let operator = match Operator::of_this_run(arguments.yes) {
                Ok(operator) => operator,
                Err(refusal) => {
                    report.announce(&refusal.to_string());
                    return Ok(Ending::Concluded(Conclusion::DidNothing));
                }
            };
            let machine = LocalMachine::new(&report, &github, tool_directory.clone())?;
            let applied = PlannedRun::resolved(
                ResolvingRun::Apply,
                arguments.run.machine,
                arguments.run.sources.clone(),
                &machine,
            )?;
            #[cfg(target_family = "windows")]
            machine.note_the_privileges_it_holds();
            let desired_state = match load_after_updating_if_it_must(
                arguments.replaced.as_ref(),
                &applied,
                &machine,
                &repositories_root()?,
                &github,
                &report,
                &operator,
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
        Task::ElevatedBatch(arguments) => {
            let report = RunReport::open(&tool_directory, RunKind::Elevated)?;
            elevation::perform(arguments.batch, &arguments.results, &report).await?;
            Ok(Ending::Concluded(Conclusion::Settled))
        }
    }
}

fn hand_over_to_the_installed_build(
    machine: &LocalMachine<'_, '_>,
    successor_arguments: Vec<OsString>,
) -> Result<Ending> {
    let installed = own_currency().path_within(&machine.binaries_directory());
    let status = std::process::Command::new(&installed)
        .args(successor_arguments)
        .status()
        .with_context(|| format!("Could not start {} to carry on", installed.display()))?;

    status
        .code()
        .map(|exit_code| Ending::HandedOver { exit_code })
        .ok_or_else(|| anyhow!("{} ended without an exit status", installed.display()))
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

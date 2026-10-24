use super::*;
use crate::{
    configuration::GitHubRepository,
    currency::SelfReplacement,
    machine::{ContentDigest, ElevatedWork},
};

fn parse(arguments: &[&str]) -> RunArguments {
    apply_arguments(arguments).run
}

fn plan_arguments(arguments: &[&str]) -> RunArguments {
    let parsed = Arguments::try_parse_from(
        std::iter::once("dotfiles_configurator").chain(arguments.iter().copied()),
    )
    .unwrap();
    match parsed.task {
        Task::Plan(plan) => plan,
        Task::Apply(_) | Task::ElevatedBatch(_) => {
            panic!("the arguments named something other than plan")
        }
    }
}

fn apply_arguments(arguments: &[&str]) -> ApplyArguments {
    let parsed = Arguments::try_parse_from(
        std::iter::once("dotfiles_configurator").chain(arguments.iter().copied()),
    )
    .unwrap();
    match parsed.task {
        Task::Apply(apply) => apply,
        Task::Plan(_) | Task::ElevatedBatch(_) => {
            panic!("the arguments named something other than apply")
        }
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
        sources_from(&["apply", "--machine", "personal", "--source", "local:config"]),
        vec![under_the_working_directory("config")]
    );
}

#[test]
fn sources_are_read_in_the_order_they_were_named() {
    assert_eq!(
        sources_from(&[
            "apply",
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
        parse(&["apply", "--machine", "work"]).machine,
        Some(MachineClass::Work)
    );
}

#[test]
fn an_apply_naming_neither_a_machine_nor_a_source_leaves_both_to_the_record() {
    assert_eq!(
        parse(&["apply"]),
        RunArguments {
            machine: None,
            sources: Vec::new(),
        }
    );
}

#[test]
fn a_plan_naming_neither_a_machine_nor_a_source_leaves_both_to_the_record() {
    assert_eq!(
        plan_arguments(&["plan"]),
        RunArguments {
            machine: None,
            sources: Vec::new(),
        }
    );
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
        Task::Plan(_) | Task::ElevatedBatch(_) => {
            panic!("the successor was started for something other than to apply")
        }
    }
}

#[test]
fn the_build_that_replaced_a_run_is_started_with_the_arguments_that_run_was_given() {
    let successor =
        parsed_successor_of(&["apply", "--machine", "work", "--source", "local:config"]);

    assert_eq!(
        successor.run,
        parse(&["apply", "--machine", "work", "--source", "local:config"])
    );
}

#[test]
fn the_build_that_replaced_an_apply_naming_nothing_leaves_its_run_to_the_same_record() {
    assert_eq!(parsed_successor_of(&["apply"]).run, parse(&["apply"]));
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
        SelfReplacement::from(parsed_successor_of(&["apply", "--machine", "personal"]).replaced),
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
fn an_elevated_batch_reads_back_the_entries_it_was_started_with() {
    let batch = ElevatedBatch::of([ElevatedWork::Installer {
        installer_path: PathBuf::from("SteamSetup.exe"),
        digest: ContentDigest::of(&b"Steam's installer"[..]).unwrap(),
    }]);
    let written = serde_json::to_string(&batch).unwrap();

    let parsed = Arguments::try_parse_from([
        "dotfiles_configurator",
        elevation::ELEVATED_BATCH,
        &format!("--{}", elevation::BATCH),
        &written,
        &format!("--{}", elevation::RESULTS),
        "results.json",
    ])
    .unwrap();

    match parsed.task {
        Task::ElevatedBatch(arguments) => assert_eq!(arguments.batch, batch),
        Task::Plan(_) | Task::Apply(_) => panic!("the arguments named another subcommand"),
    }
}

use super::*;

// 2026-08-10: four of the eleven entries the user hive of a Windows 11 machine held, so the
// parser is exercised against the shape the registry stores rather than one assumed for it.
const USER_PATH: &str = concat!(
    "%USERPROFILE%\\.cargo\\bin;",
    "C:\\Program Files\\Git\\cmd;",
    "%LOCALAPPDATA%\\Microsoft\\WindowsApps;",
    "C:\\Program Files\\Neovim\\bin",
);

#[test]
fn an_entry_holding_an_unexpanded_reference_is_read_back_holding_it() {
    assert_eq!(entries_of(USER_PATH)[0], "%USERPROFILE%\\.cargo\\bin");
}

#[test]
fn a_value_ending_in_a_separator_names_one_directory_per_entry_and_no_empty_one() {
    assert_eq!(entries_of("C:\\Windows;C:\\Temp;").len(), 2);
}

#[test]
fn a_value_naming_nothing_names_no_directories() {
    assert!(entries_of("").is_empty());
}

#[test]
fn carrying_a_new_directory_leaves_every_character_already_stored_where_it_was() {
    let carrying = carrying(USER_PATH, Path::new("C:\\tools\\bin"));

    assert!(carrying.starts_with(USER_PATH), "{carrying}");
}

#[test]
fn a_value_that_names_nothing_comes_to_carry_the_directory_alone() {
    assert_eq!(carrying("", Path::new("C:\\tools\\bin")), "C:\\tools\\bin");
}

#[test]
fn a_value_ending_in_a_separator_fills_that_slot_rather_than_growing_another() {
    assert_eq!(
        carrying("C:\\Windows;", Path::new("C:\\tools\\bin")),
        "C:\\Windows;C:\\tools\\bin"
    );
}

#[test]
fn a_value_that_already_carries_the_directory_is_returned_unchanged() {
    assert_eq!(
        carrying(USER_PATH, Path::new("C:\\Program Files\\Git\\cmd")),
        USER_PATH
    );
}

#[test]
fn a_directory_stored_in_another_case_is_not_added_a_second_time() {
    assert_eq!(
        carrying(USER_PATH, Path::new("c:\\program files\\GIT\\cmd\\")),
        USER_PATH
    );
}

#[test]
fn a_directory_stored_as_an_unexpanded_reference_is_not_added_again_under_that_reference() {
    assert_eq!(
        carrying(USER_PATH, Path::new("%USERPROFILE%\\.cargo\\bin")),
        USER_PATH
    );
}

const CARGO_BINARIES: &str = "C:\\Users\\Alice\\.cargo\\bin";

// 2026-10-26: the first two entries and the GitHub CLI entry the Machine hive of the personal
// machine held, Windows 11.
const MACHINE_PATH: &str = concat!(
    "C:\\Program Files\\Common Files\\Oracle\\Java\\javapath;",
    "%SystemRoot%\\system32;",
    "C:\\Program Files\\GitHub CLI\\",
);

fn unexpanded(entry: &str) -> PathBuf {
    PathBuf::from(entry)
}

fn expanding_the_user_profile(entry: &str) -> PathBuf {
    PathBuf::from(entry.replace("%USERPROFILE%", "C:\\Users\\Alice"))
}

#[test]
fn fronting_puts_the_directory_before_every_character_already_stored() {
    assert_eq!(
        fronting(MACHINE_PATH, Path::new(CARGO_BINARIES), unexpanded),
        format!("{CARGO_BINARIES};{MACHINE_PATH}")
    );
}

#[test]
fn fronting_keeps_an_unexpanded_reference_unexpanded() {
    let fronted = fronting(MACHINE_PATH, Path::new(CARGO_BINARIES), unexpanded);

    assert_eq!(entries_of(&fronted)[2], "%SystemRoot%\\system32");
}

#[test]
fn fronting_a_value_already_starting_with_the_directory_returns_it_unchanged() {
    let stored = format!("{CARGO_BINARIES}\\;{MACHINE_PATH}");

    assert_eq!(
        fronting(&stored, Path::new(CARGO_BINARIES), unexpanded),
        stored
    );
}

#[test]
fn fronting_a_value_whose_first_entry_expands_to_the_directory_returns_it_unchanged() {
    let stored = format!("%USERPROFILE%\\.cargo\\bin;{MACHINE_PATH}");

    assert_eq!(
        fronting(
            &stored,
            Path::new(CARGO_BINARIES),
            expanding_the_user_profile
        ),
        stored
    );
}

#[test]
fn fronting_leaves_a_later_entry_for_the_same_directory_in_place() {
    let stored = format!("{MACHINE_PATH};{CARGO_BINARIES}");

    assert_eq!(
        fronting(&stored, Path::new(CARGO_BINARIES), unexpanded),
        format!("{CARGO_BINARIES};{stored}")
    );
}

#[test]
fn fronting_twice_equals_fronting_once() {
    let duplicated = format!("{MACHINE_PATH};{CARGO_BINARIES}");
    for stored in ["", MACHINE_PATH, "C:\\Windows;", duplicated.as_str()] {
        let once = fronting(stored, Path::new(CARGO_BINARIES), unexpanded);

        assert_eq!(fronting(&once, Path::new(CARGO_BINARIES), unexpanded), once);
    }
}

#[test]
fn fronting_a_value_naming_nothing_gives_the_directory_alone() {
    assert_eq!(
        fronting("", Path::new(CARGO_BINARIES), unexpanded),
        CARGO_BINARIES
    );
}

#[test]
fn a_machine_search_path_starting_with_the_directory_holds_it_first() {
    let reading = MachineSearchPath::of([
        PathBuf::from(CARGO_BINARIES),
        PathBuf::from("C:\\Program Files\\GitHub CLI\\"),
    ]);

    assert_eq!(
        reading.place_of(Path::new(CARGO_BINARIES)),
        SearchPathPlace::First
    );
}

#[test]
fn a_machine_search_path_naming_the_directory_after_another_holds_it_later() {
    let reading = MachineSearchPath::of([
        PathBuf::from("C:\\Program Files\\GitHub CLI\\"),
        PathBuf::from(CARGO_BINARIES),
    ]);

    assert_eq!(
        reading.place_of(Path::new(CARGO_BINARIES)),
        SearchPathPlace::Later
    );
}

#[test]
fn a_machine_search_path_naming_nothing_holds_no_directory() {
    assert_eq!(
        MachineSearchPath::of([]).place_of(Path::new(CARGO_BINARIES)),
        SearchPathPlace::Absent
    );
}

#[test]
fn a_directory_the_user_hive_names_is_carried() {
    let reading = SearchPathReading::of([PathBuf::from("C:\\Program Files\\Git\\cmd")]);

    assert!(reading.carries(Path::new("C:\\Program Files\\Git\\cmd")));
}

#[test]
fn a_directory_stored_in_another_case_is_carried_because_the_file_system_agrees() {
    let reading = SearchPathReading::of([PathBuf::from("C:\\Program Files\\Git\\cmd")]);

    assert!(reading.carries(Path::new("c:\\program files\\git\\CMD")));
}

#[test]
fn a_directory_stored_with_a_trailing_separator_is_carried() {
    let reading = SearchPathReading::of([PathBuf::from("C:\\tools\\bin\\")]);

    assert!(reading.carries(Path::new("C:\\tools\\bin")));
}

#[test]
fn a_directory_written_with_the_other_separator_is_carried_because_both_name_one_directory() {
    let reading = SearchPathReading::of([PathBuf::from("C:\\tools\\bin")]);

    assert!(reading.carries(Path::new("C:/tools/bin")));
}

#[test]
fn a_directory_written_with_the_other_separator_is_not_added_a_second_time() {
    assert_eq!(
        carrying("C:\\tools\\bin", Path::new("C:/tools/bin")),
        "C:\\tools\\bin"
    );
}

#[test]
fn a_directory_no_entry_names_is_not_carried() {
    let reading = SearchPathReading::of([PathBuf::from("C:\\tools\\bin")]);

    assert!(!reading.carries(Path::new("C:\\tools")));
}

#[test]
fn a_search_path_naming_nothing_carries_nothing() {
    let reading = SearchPathReading::of([]);

    assert!(!reading.carries(Path::new("C:\\tools\\bin")));
}

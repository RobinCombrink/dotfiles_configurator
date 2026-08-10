use std::path::{Path, PathBuf};

const SEPARATOR: char = ';';

// ADR 0017
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPathReading {
    entries: Vec<PathBuf>,
}

impl SearchPathReading {
    pub fn of(entries: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// Whether a directory is already on the search path. A directory the machine-wide hive names
    /// is carried as much as one the user's names, so appending it to the user's would produce a
    /// duplicate that works and should not exist.
    ///
    /// ```
    /// # use dotfiles_configurator::machine::environment_reading::SearchPathReading;
    /// # use std::path::{Path, PathBuf};
    /// let reading = SearchPathReading::of([PathBuf::from("C:\\Program Files\\Git\\cmd\\")]);
    ///
    /// assert!(reading.carries(Path::new("C:\\program files\\git\\cmd")));
    /// assert!(!reading.carries(Path::new("C:\\Program Files\\Git")));
    /// ```
    pub fn carries(&self, directory: &Path) -> bool {
        let wanted = comparable(directory);
        self.entries.iter().any(|entry| comparable(entry) == wanted)
    }
}

fn comparable(directory: &Path) -> String {
    directory
        .display()
        .to_string()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// The directories a raw search path value names, each still exactly as it is stored.
///
/// ```
/// # use dotfiles_configurator::machine::environment_reading::entries_of;
/// assert_eq!(
///     entries_of("%USERPROFILE%\\.cargo\\bin;C:\\Windows;"),
///     vec!["%USERPROFILE%\\.cargo\\bin", "C:\\Windows"]
/// );
/// ```
pub fn entries_of(raw: &str) -> Vec<&str> {
    raw.split(SEPARATOR)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// The value that puts a directory on a search path, leaving every character already there exactly
/// where it was: a value carrying unexpanded references still carries them, and one already ending
/// in a separator fills that slot rather than growing a second one.
///
/// ```
/// # use dotfiles_configurator::machine::environment_reading::appended;
/// # use std::path::Path;
/// let directory = Path::new("C:\\Users\\Alice\\.dotfiles_configurator\\bin");
///
/// assert_eq!(
///     appended("%USERPROFILE%\\.cargo\\bin;C:\\Windows", directory),
///     "%USERPROFILE%\\.cargo\\bin;C:\\Windows;C:\\Users\\Alice\\.dotfiles_configurator\\bin"
/// );
/// assert_eq!(
///     appended("C:\\Windows;", directory),
///     "C:\\Windows;C:\\Users\\Alice\\.dotfiles_configurator\\bin"
/// );
/// ```
pub fn appended(raw: &str, directory: &Path) -> String {
    let directory = directory.display().to_string();

    if raw.is_empty() {
        return directory;
    }
    if raw.ends_with(SEPARATOR) {
        return format!("{raw}{directory}");
    }

    format!("{raw}{SEPARATOR}{directory}")
}

#[cfg(test)]
mod tests {
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
    fn appending_leaves_every_character_already_stored_exactly_where_it_was() {
        let appended = appended(USER_PATH, Path::new("C:\\tools\\bin"));

        assert!(appended.starts_with(USER_PATH), "{appended}");
    }

    #[test]
    fn appending_to_a_value_that_names_nothing_stores_the_directory_alone() {
        assert_eq!(appended("", Path::new("C:\\tools\\bin")), "C:\\tools\\bin");
    }

    #[test]
    fn appending_to_a_value_ending_in_a_separator_fills_that_slot_rather_than_adding_another() {
        assert_eq!(
            appended("C:\\Windows;", Path::new("C:\\tools\\bin")),
            "C:\\Windows;C:\\tools\\bin"
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
    fn a_directory_no_entry_names_is_not_carried() {
        let reading = SearchPathReading::of([PathBuf::from("C:\\tools\\bin")]);

        assert!(!reading.carries(Path::new("C:\\tools")));
    }

    #[test]
    fn a_search_path_naming_nothing_carries_nothing() {
        let reading = SearchPathReading::of([]);

        assert!(!reading.carries(Path::new("C:\\tools\\bin")));
    }
}

use {
    crate::configuration::path_folding,
    std::path::{Path, PathBuf},
};

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
        let wanted = path_folding::comparable(directory);
        self.entries
            .iter()
            .any(|entry| path_folding::comparable(entry) == wanted)
    }

    /// The file a name resolves to: the first entry, in search order, that holds a file of that
    /// name.
    ///
    /// ```
    /// # use dotfiles_configurator::machine::environment_reading::SearchPathReading;
    /// # use std::path::{Path, PathBuf};
    /// let reading = SearchPathReading::of([
    ///     PathBuf::from("C:\\Windows"),
    ///     PathBuf::from("C:\\tools"),
    ///     PathBuf::from("C:\\Users\\Alice\\.cargo\\bin"),
    /// ]);
    /// let held = [
    ///     Path::new("C:\\tools").join("stop-gate.exe"),
    ///     Path::new("C:\\Users\\Alice\\.cargo\\bin").join("stop-gate.exe"),
    /// ];
    ///
    /// assert_eq!(
    ///     reading.resolving("stop-gate.exe", |path| held.iter().any(|file| file == path)),
    ///     Some(Path::new("C:\\tools").join("stop-gate.exe"))
    /// );
    /// assert_eq!(reading.resolving("sweep.exe", |path| held.iter().any(|file| file == path)), None);
    /// ```
    pub fn resolving(&self, file_name: &str, holds: impl Fn(&Path) -> bool) -> Option<PathBuf> {
        self.entries
            .iter()
            .map(|entry| entry.join(file_name))
            .find(|candidate| holds(candidate))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchPathPlace {
    First,
    Later,
    Absent,
}

/// The Machine hive's search path alone, each entry expanded, in the order it is stored.
///
/// ```
/// # use dotfiles_configurator::machine::environment_reading::MachineSearchPath;
/// # use dotfiles_configurator::machine::environment_reading::SearchPathPlace;
/// # use std::path::{Path, PathBuf};
/// let reading = MachineSearchPath::of([
///     PathBuf::from("C:\\Users\\Alice\\.cargo\\bin\\"),
///     PathBuf::from("C:\\Program Files\\GitHub CLI"),
/// ]);
///
/// assert_eq!(
///     reading.place_of(Path::new("c:\\users\\alice\\.cargo\\bin")),
///     SearchPathPlace::First
/// );
/// assert_eq!(
///     reading.place_of(Path::new("C:\\Program Files\\GitHub CLI")),
///     SearchPathPlace::Later
/// );
/// assert_eq!(reading.place_of(Path::new("C:\\Windows")), SearchPathPlace::Absent);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineSearchPath {
    entries: Vec<PathBuf>,
}

impl MachineSearchPath {
    pub fn of(entries: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    pub fn place_of(&self, directory: &Path) -> SearchPathPlace {
        let wanted = path_folding::comparable(directory);
        match self
            .entries
            .iter()
            .position(|entry| path_folding::comparable(entry) == wanted)
        {
            Some(0) => SearchPathPlace::First,
            Some(_) => SearchPathPlace::Later,
            None => SearchPathPlace::Absent,
        }
    }
}

/// A stored Machine search path value that starts with a directory, given the value stored now
/// and how the machine expands an entry.
///
/// A value whose first entry expands to the directory comes back unchanged. Otherwise the
/// directory, spelled as given, goes before the first entry, and every character already stored
/// stays as it was, so a later entry naming the same directory is left in place.
///
/// ```
/// # use dotfiles_configurator::machine::environment_reading::fronting;
/// # use std::path::{Path, PathBuf};
/// let directory = Path::new("C:\\Users\\Alice\\.cargo\\bin");
/// let unexpanded = |entry: &str| PathBuf::from(entry);
///
/// assert_eq!(
///     fronting("%SystemRoot%\\system32;C:\\Program Files\\GitHub CLI\\", directory, unexpanded),
///     "C:\\Users\\Alice\\.cargo\\bin;%SystemRoot%\\system32;C:\\Program Files\\GitHub CLI\\"
/// );
///
/// let once = fronting("C:\\Windows", directory, unexpanded);
/// assert_eq!(fronting(&once, directory, unexpanded), once);
/// ```
pub fn fronting(raw: &str, directory: &Path, expanded: impl Fn(&str) -> PathBuf) -> String {
    let stored = MachineSearchPath::of(entries_of(raw).into_iter().map(expanded));
    if stored.place_of(directory) == SearchPathPlace::First {
        return raw.to_owned();
    }

    let directory = directory.display().to_string();
    if raw.is_empty() {
        return directory;
    }

    format!("{directory}{SEPARATOR}{raw}")
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

/// A search path value that carries a directory, given the value that is stored now.
///
/// The postcondition is membership rather than an append, so applying it to a value that already
/// names the directory returns that value unchanged — which is what stops two resources resolving
/// to one directory from each adding it. Every character already stored stays exactly where it
/// was: a value carrying unexpanded references still carries them, and one already ending in a
/// separator fills that slot rather than growing a second one.
///
/// ```
/// # use dotfiles_configurator::machine::environment_reading::carrying;
/// # use std::path::Path;
/// let directory = Path::new("C:\\Users\\Alice\\.dotfiles_configurator\\bin");
///
/// assert_eq!(
///     carrying("%USERPROFILE%\\.cargo\\bin;C:\\Windows", directory),
///     "%USERPROFILE%\\.cargo\\bin;C:\\Windows;C:\\Users\\Alice\\.dotfiles_configurator\\bin"
/// );
/// assert_eq!(
///     carrying("C:\\Windows;", directory),
///     "C:\\Windows;C:\\Users\\Alice\\.dotfiles_configurator\\bin"
/// );
///
/// let once = carrying("C:\\Windows", directory);
/// assert_eq!(carrying(&once, directory), once);
/// ```
pub fn carrying(raw: &str, directory: &Path) -> String {
    if already_carries(raw, directory) {
        return raw.to_owned();
    }

    let directory = directory.display().to_string();
    if raw.is_empty() {
        return directory;
    }
    if raw.ends_with(SEPARATOR) {
        return format!("{raw}{directory}");
    }

    format!("{raw}{SEPARATOR}{directory}")
}

fn already_carries(raw: &str, directory: &Path) -> bool {
    let wanted = path_folding::comparable(directory);
    entries_of(raw)
        .into_iter()
        .any(|entry| path_folding::comparable(Path::new(entry)) == wanted)
}

#[cfg(test)]
mod tests;

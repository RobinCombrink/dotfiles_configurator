use {
    crate::{
        configuration::{GitHubRepository, Tool},
        convergence::Impediment,
    },
    std::{fmt::Display, path::PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnreadableReason(String);

impl From<String> for UnreadableReason {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for UnreadableReason {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl Display for UnreadableReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadSource {
    Tool(Tool),
    SearchPath,
    CargoWorkspace(PathBuf),
    LatestRelease(GitHubRepository),
}

impl ReadSource {
    pub fn was_not_read(&self) -> Impediment {
        Impediment::ActualStateUnreadable(UnreadableReason::from(format!(
            "{self} was not read for this change set"
        )))
    }
}

impl Display for ReadSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadSource::Tool(tool) => Display::fmt(tool, formatter),
            ReadSource::SearchPath => formatter.write_str("the search path"),
            ReadSource::CargoWorkspace(clone_directory) => write!(
                formatter,
                "the cargo workspace at {}",
                clone_directory.display()
            ),
            ReadSource::LatestRelease(repository) => {
                write!(formatter, "the latest release of {repository}")
            }
        }
    }
}

/// The three states one source can be in for a change set, so that a reader cannot reach the
/// value without having answered for the two states where there is none. The impediment is the
/// only thing the non-read states yield, which is what keeps an unreadable actual state out of
/// [`crate::convergence::Assessment::Drifted`].
///
/// ```
/// # use dotfiles_configurator::convergence::{Impediment, source_reading::SourceReading};
/// let reading: SourceReading<String> = SourceReading::Unreadable("winget exited 1".into());
///
/// let Err(impediment) = reading.read() else {
///     panic!("a reading that could not be read answered a value");
/// };
/// assert_eq!(impediment.to_string(), "winget exited 1");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceReading<T> {
    Read(T),
    Unreadable(UnreadableReason),
    NotRequested(ReadSource),
}

impl<T> SourceReading<T> {
    pub fn read(&self) -> Result<&T, Impediment> {
        match self {
            SourceReading::Read(value) => Ok(value),
            SourceReading::Unreadable(reason) => {
                Err(Impediment::ActualStateUnreadable(reason.clone()))
            }
            SourceReading::NotRequested(source) => Err(source.was_not_read()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_nothing_asked_for_is_an_impediment_naming_the_source() {
        let reading: SourceReading<String> =
            SourceReading::NotRequested(ReadSource::Tool(Tool::Winget));

        let impediment = reading.read().expect_err("a reading that holds no value");

        assert_eq!(
            impediment.to_string(),
            "winget was not read for this change set"
        );
    }

    #[test]
    fn a_workspace_nothing_asked_for_names_the_directory_it_would_have_been_read_from() {
        let reading: SourceReading<String> = SourceReading::NotRequested(
            ReadSource::CargoWorkspace(PathBuf::from("/repositories/Personal/dotfiles")),
        );

        let impediment = reading.read().expect_err("a reading that holds no value");

        assert!(impediment.to_string().contains("dotfiles"), "{impediment}");
    }

    #[test]
    fn a_source_that_was_read_answers_the_value_it_read() {
        let reading = SourceReading::Read("a listing".to_owned());

        assert_eq!(
            reading.read().expect("a reading that holds a value"),
            "a listing"
        );
    }
}

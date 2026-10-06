use {
    crate::configuration::{DartPackageName, GitCommit},
    std::{fmt::Display, path::PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartLocations {
    pub install_directory: PathBuf,
    pub pub_cache_directory: PathBuf,
}

impl DartLocations {
    /// ```
    /// # use {
    /// #     dotfiles_configurator::{
    /// #         configuration::DartPackageName, machine::dart_reading::DartLocations,
    /// #     },
    /// #     std::path::PathBuf,
    /// # };
    /// let locations = DartLocations {
    ///     install_directory: PathBuf::from("/install"),
    ///     pub_cache_directory: PathBuf::from("/cache"),
    /// };
    ///
    /// assert_eq!(
    ///     locations.bundles_of(&DartPackageName::from("coderabbit_findings")),
    ///     PathBuf::from("/install/app-bundles/coderabbit_findings/git")
    /// );
    /// assert_eq!(locations.mirrors(), PathBuf::from("/cache/git/cache"));
    /// ```
    pub fn bundles_of(&self, name: &DartPackageName) -> PathBuf {
        self.install_directory
            .join("app-bundles")
            .join(name.as_ref())
            .join("git")
    }

    pub fn mirrors(&self) -> PathBuf {
        self.pub_cache_directory.join("git").join("cache")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartReading {
    Current,
    Drifted(DartDrift),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartDrift {
    NoBundle,
    SeveralBundles(usize),
    InstalledFromElsewhere(String),
    NoMirror,
    AtAnotherCommit {
        installed: GitCommit,
        desired: GitCommit,
    },
}

impl Display for DartDrift {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DartDrift::NoBundle => formatter.write_str("dart has not installed it"),
            DartDrift::SeveralBundles(count) => {
                write!(
                    formatter,
                    "dart holds {count} bundles of it where it keeps one"
                )
            }
            DartDrift::InstalledFromElsewhere(source) => {
                write!(formatter, "installed from {source}")
            }
            DartDrift::NoMirror => {
                formatter.write_str("pub holds no copy of the repository it is installed from")
            }
            DartDrift::AtAnotherCommit { installed, desired } => write!(
                formatter,
                "installed from {installed}, and the declared reference names {desired}"
            ),
        }
    }
}

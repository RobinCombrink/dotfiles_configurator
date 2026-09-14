use {
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::fmt::Display,
};

/// Declares a domain identifier: a transparent wrapper that is a string only at the wire
/// boundary, so one kind of name cannot be passed where another is meant.
macro_rules! name {
    ($(#[$documentation:meta])* $name:ident) => {
        $(#[$documentation])*
        #[derive(
            Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
        )]
        #[serde(transparent)]
        #[repr(transparent)]
        pub struct $name(String);

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

name!(
    /// The account a repository belongs to on GitHub.
    RepositoryOwner
);

name!(
    /// A repository's own name, which is also the name of the directory it is cloned into.
    RepositoryName
);

name!(
    /// The account a configuration acts as, inherited by every resource it declares.
    GitHubAccount
);

name!(
    /// The document a configuration was read out of, as a person is told it when two of them
    /// disagree.
    ///
    /// ```
    /// # use {dotfiles_configurator::configuration::ConfigurationName, std::path::Path};
    /// let path = Path::new("/config/personal.dotconfig.json");
    ///
    /// assert_eq!(
    ///     ConfigurationName::from(path).to_string(),
    ///     path.display().to_string()
    /// );
    /// ```
    ConfigurationName
);

impl From<&std::path::Path> for ConfigurationName {
    fn from(path: &std::path::Path) -> Self {
        Self(path.display().to_string())
    }
}

name!(
    /// The name an application is known by.
    ApplicationName
);

name!(BinaryName);

impl BinaryName {
    /// The file a binary of this name occupies on this platform. The name itself is the bare
    /// stem, so one binary compares equal to itself wherever it is read.
    ///
    /// ```
    /// # use dotfiles_configurator::configuration::BinaryName;
    /// assert_eq!(
    ///     BinaryName::from("stop-gate").file_name(),
    ///     format!("stop-gate{}", std::env::consts::EXE_SUFFIX)
    /// );
    /// ```
    pub fn file_name(&self) -> String {
        format!("{self}{}", std::env::consts::EXE_SUFFIX)
    }
}

name!(
    /// The identifier winget knows a package by.
    WingetPackageId
);

name!(
    /// The name Cargo knows a crate by.
    CrateName
);

name!(
    /// The name Claude Code holds an MCP server under.
    McpServerName
);

name!(VariableValue);

impl VariableValue {
    /// Whether the value names another variable rather than spelling its content out, which
    /// decides how it has to be stored: written as a literal, the reference is discarded
    /// permanently.
    ///
    /// ```
    /// # use dotfiles_configurator::configuration::VariableValue;
    /// assert!(VariableValue::from("%USERPROFILE%\\bin").carries_a_reference());
    /// assert!(!VariableValue::from("nvim").carries_a_reference());
    /// ```
    pub fn carries_a_reference(&self) -> bool {
        self.0.contains(REFERENCE_MARKER)
    }
}

const REFERENCE_MARKER: char = '%';

/// The name of a variable a configuration may set, which is every name except the one the search
/// path entry shape owns. Setting that one would replace a value whose desired state is membership
/// with a value of its own, so it is refused where a name is made rather than where one is used.
///
/// ```
/// # use dotfiles_configurator::configuration::VariableName;
/// assert!(VariableName::try_from("EDITOR").is_ok());
/// assert!(VariableName::try_from("Path").is_err());
/// assert!(VariableName::try_from("pAtH").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct VariableName(String);

const SEARCH_PATH_NAME: &str = "Path";

impl TryFrom<String> for VariableName {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.trim().is_empty() {
            return Err("a variable is named, and \"\" is not a name".to_owned());
        }
        if name.eq_ignore_ascii_case(SEARCH_PATH_NAME) {
            return Err(format!(
                "{name} is the search path, whose desired state is membership rather than a whole \
                 value. Declare a search path entry instead of a variable."
            ));
        }

        Ok(Self(name))
    }
}

impl TryFrom<&str> for VariableName {
    type Error = String;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::try_from(name.to_owned())
    }
}

impl Display for VariableName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for VariableName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for VariableName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for VariableName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::try_from(name).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for VariableName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "VariableName".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::VariableName").into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "description": "The name of a variable to set. Every name except Path, which a search \
                            path entry declares instead.",
        })
    }
}

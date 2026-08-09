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
    /// The name an application is known by.
    ApplicationName
);

name!(BinaryName);

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

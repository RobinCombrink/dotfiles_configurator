use {
    schemars::{JsonSchema, Schema, SchemaGenerator},
    serde::{Deserialize, Deserializer, Serialize, Serializer},
    std::{borrow::Cow, fmt::Display},
};

/// The version a package manager keeps a package at: whichever is latest, or exactly one.
/// A declaration naming no version declares the latest.
///
/// ```
/// # use dotfiles_configurator::configuration::{CrateVersion, PackageCurrency};
/// let pinned: PackageCurrency<CrateVersion> = serde_json::from_str(r#""27.1.0""#).unwrap();
///
/// assert_eq!(
///     pinned,
///     PackageCurrency::Exactly(CrateVersion::try_from("27.1.0").unwrap())
/// );
/// assert!(PackageCurrency::<CrateVersion>::default().is_latest());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum PackageCurrency<Version> {
    #[default]
    Latest,
    Exactly(Version),
}

impl<Version> PackageCurrency<Version> {
    pub fn is_latest(&self) -> bool {
        match self {
            PackageCurrency::Latest => true,
            PackageCurrency::Exactly(_) => false,
        }
    }

    pub fn exact(&self) -> Option<&Version> {
        match self {
            PackageCurrency::Latest => None,
            PackageCurrency::Exactly(version) => Some(version),
        }
    }
}

impl<Version> From<Option<Version>> for PackageCurrency<Version> {
    fn from(stated: Option<Version>) -> Self {
        match stated {
            None => PackageCurrency::Latest,
            Some(version) => PackageCurrency::Exactly(version),
        }
    }
}

impl<Version: Display> Display for PackageCurrency<Version> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageCurrency::Latest => formatter.write_str("the latest version"),
            PackageCurrency::Exactly(version) => write!(formatter, "exactly {version}"),
        }
    }
}

impl<Version: Serialize> Serialize for PackageCurrency<Version> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.exact().serialize(serializer)
    }
}

impl<'de, Version: Deserialize<'de>> Deserialize<'de> for PackageCurrency<Version> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<Version>::deserialize(deserializer).map(Self::from)
    }
}

impl<Version: JsonSchema> JsonSchema for PackageCurrency<Version> {
    fn inline_schema() -> bool {
        Option::<Version>::inline_schema()
    }

    fn schema_name() -> Cow<'static, str> {
        Option::<Version>::schema_name()
    }

    fn schema_id() -> Cow<'static, str> {
        Option::<Version>::schema_id()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        Option::<Version>::json_schema(generator)
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::configuration::CrateVersion};

    fn read(stated: &str) -> Result<PackageCurrency<CrateVersion>, serde_json::Error> {
        serde_json::from_str(stated)
    }

    #[test]
    fn a_stated_version_is_the_one_version_the_package_is_kept_at() {
        assert_eq!(
            read(r#""1.2.3""#).unwrap(),
            PackageCurrency::Exactly(CrateVersion::try_from("1.2.3").unwrap())
        );
    }

    #[test]
    fn a_version_stated_as_nothing_is_the_latest() {
        assert_eq!(read("null").unwrap(), PackageCurrency::Latest);
    }

    #[test]
    fn a_version_the_manager_cannot_read_is_refused_rather_than_read_as_the_latest() {
        assert!(read(r#""^1""#).is_err());
    }

    #[test]
    fn the_latest_is_written_back_as_no_version_at_all() {
        assert_eq!(
            serde_json::to_string(&PackageCurrency::<CrateVersion>::Latest).unwrap(),
            "null"
        );
    }
}

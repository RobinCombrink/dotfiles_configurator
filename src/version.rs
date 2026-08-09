use std::fmt::Display;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Version(String);

// ADR 0016
impl TryFrom<&str> for Version {
    type Error = String;

    /// ```
    /// use dotfiles_configurator::version::Version;
    ///
    /// assert_eq!(Version::try_from("v3.2.0"), Version::try_from("3.2.0"));
    /// assert!(Version::try_from("version").is_err());
    /// ```
    fn try_from(spelled: &str) -> Result<Self, Self::Error> {
        let without_prefix = spelled.strip_prefix('v').unwrap_or(spelled);

        match without_prefix.starts_with(|character: char| character.is_ascii_digit()) {
            true => Ok(Self(without_prefix.to_owned())),
            false => Err(format!(
                "{spelled:?} is not a version; a version starts with a digit"
            )),
        }
    }
}

impl Display for Version {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_tag_and_the_version_a_binary_reports_are_the_same_version() {
        assert_eq!(
            Version::try_from("v3.2.0").unwrap(),
            Version::try_from("3.2.0").unwrap()
        );
    }

    #[test]
    fn a_version_carrying_more_than_three_numbers_is_kept_whole() {
        assert_eq!(
            Version::try_from("2.52.0.windows.1").unwrap().to_string(),
            "2.52.0.windows.1"
        );
    }

    #[test]
    fn the_version_a_build_stamps_when_nothing_overrides_it_is_no_release() {
        assert_ne!(
            Version::try_from("0.0.0-dev").unwrap(),
            Version::try_from("v0.0.0").unwrap()
        );
    }

    #[test]
    fn a_word_that_is_not_a_version_is_refused_by_naming_what_it_was() {
        let refusal = Version::try_from("version").unwrap_err();

        assert!(refusal.contains("version"), "{refusal}");
    }

    #[test]
    fn a_date_printed_beside_a_version_is_not_itself_a_version() {
        assert!(Version::try_from("(2025-09-23)").is_err());
    }
}

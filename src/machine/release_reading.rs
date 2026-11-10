use {
    crate::{
        configuration::{AssetPattern, GitHubRepository},
        configuration_source::EnvironmentFailure,
        version::Version,
    },
    std::{fmt::Display, sync::Arc},
    url::Url,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: Url,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseReading {
    pub version: Version,
    pub assets: Vec<ReleaseAsset>,
}

impl ReleaseReading {
    pub fn asset_matching(&self, pattern: &AssetPattern) -> Result<&ReleaseAsset, NoMatchingAsset> {
        self.assets
            .iter()
            .find(|asset| pattern.matches(&asset.name))
            .ok_or_else(|| NoMatchingAsset {
                release: self.clone(),
                pattern: pattern.clone(),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoMatchingAsset {
    pub release: ReleaseReading,
    pub pattern: AssetPattern,
}

impl Display for NoMatchingAsset {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "no asset of release {} matches {}. It carries {}",
            self.release.version,
            self.pattern,
            NamedAssets(&self.release.assets)
        )
    }
}

impl std::error::Error for NoMatchingAsset {}

struct NamedAssets<'assets>(&'assets [ReleaseAsset]);

impl Display for NamedAssets<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.is_empty() {
            true => formatter.write_str("no assets at all"),
            false => {
                let names: Vec<&str> = self.0.iter().map(|asset| asset.name.as_str()).collect();
                write!(formatter, "{}", names.join(", "))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum ReleaseReadFailure {
    AccountUnavailable(Arc<EnvironmentFailure>),
    Unasked {
        repository: GitHubRepository,
        error: Arc<octocrab::Error>,
    },
    TaggedAnUnreadableVersion {
        repository: GitHubRepository,
        tag: String,
    },
}

impl Display for ReleaseReadFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleaseReadFailure::AccountUnavailable(failure) => Display::fmt(failure, formatter),
            ReleaseReadFailure::Unasked { repository, error } => write!(
                formatter,
                "Could not read the latest release of {repository}: {error}"
            ),
            ReleaseReadFailure::TaggedAnUnreadableVersion { repository, tag } => write!(
                formatter,
                "the latest release of {repository} is tagged {tag:?}, which is not a version; a \
                 version starts with a digit"
            ),
        }
    }
}

impl std::error::Error for ReleaseReadFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReleaseReadFailure::AccountUnavailable(failure) => Some(failure.as_ref()),
            ReleaseReadFailure::Unasked { error, .. } => Some(error.as_ref()),
            ReleaseReadFailure::TaggedAnUnreadableVersion { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;

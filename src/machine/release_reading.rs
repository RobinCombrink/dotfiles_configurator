use {
    crate::{configuration::AssetPattern, version::Version},
    std::fmt::Display,
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

#[cfg(test)]
mod tests;

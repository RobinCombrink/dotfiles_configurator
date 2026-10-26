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
    pub fn asset_matching(&self, pattern: &AssetPattern) -> Result<&ReleaseAsset, String> {
        self.assets
            .iter()
            .find(|asset| pattern.matches(&asset.name))
            .ok_or_else(|| {
                format!(
                    "no asset of release {} matches {pattern}. It carries {}",
                    self.version,
                    NamedAssets(&self.assets)
                )
            })
    }
}

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

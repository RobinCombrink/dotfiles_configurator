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
mod tests {
    use super::*;

    fn release_carrying(names: &[&str]) -> ReleaseReading {
        ReleaseReading {
            version: Version::try_from("3.2.0").unwrap(),
            assets: names
                .iter()
                .map(|name| ReleaseAsset {
                    name: (*name).to_owned(),
                    download_url: Url::parse(&format!("https://example.invalid/{name}")).unwrap(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_asset_a_pattern_picks_is_the_one_matching_it() {
        let release = release_carrying(&["notes.txt", "tool-windows-x86_64.zip"]);

        let matched = release
            .asset_matching(&AssetPattern::EndsWith(".zip".to_owned()))
            .unwrap();

        assert_eq!(matched.name, "tool-windows-x86_64.zip");
    }

    #[test]
    fn a_release_carrying_nothing_the_pattern_matches_names_what_it_does_carry() {
        let release = release_carrying(&["notes.txt"]);

        let refusal = release
            .asset_matching(&AssetPattern::EndsWith(".zip".to_owned()))
            .unwrap_err();

        assert!(refusal.contains("notes.txt"), "{refusal}");
    }

    #[test]
    fn a_release_carrying_no_assets_says_so_rather_than_listing_nothing() {
        let release = release_carrying(&[]);

        let refusal = release
            .asset_matching(&AssetPattern::EndsWith(".zip".to_owned()))
            .unwrap_err();

        assert!(refusal.contains("no assets at all"), "{refusal}");
    }
}

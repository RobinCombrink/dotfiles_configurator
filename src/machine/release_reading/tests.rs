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

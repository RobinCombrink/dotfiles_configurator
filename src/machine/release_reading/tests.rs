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

fn zip() -> AssetPattern {
    AssetPattern::EndsWith(".zip".to_owned())
}

#[test]
fn the_asset_a_pattern_picks_is_the_one_matching_it() {
    let release = release_carrying(&["notes.txt", "tool-windows-x86_64.zip"]);

    let matched = release.asset_matching(&zip()).unwrap();

    assert_eq!(matched.name, "tool-windows-x86_64.zip");
}

#[test]
fn a_release_carrying_nothing_the_pattern_matches_is_refused_with_what_it_does_carry() {
    let release = release_carrying(&["notes.txt"]);

    assert_eq!(
        release.asset_matching(&zip()),
        Err(NoMatchingAsset {
            release: release.clone(),
            pattern: zip(),
        })
    );
}

#[test]
fn a_release_carrying_no_assets_is_refused_with_the_release_it_read() {
    let release = release_carrying(&[]);

    assert_eq!(
        release.asset_matching(&zip()),
        Err(NoMatchingAsset {
            release: release.clone(),
            pattern: zip(),
        })
    );
}

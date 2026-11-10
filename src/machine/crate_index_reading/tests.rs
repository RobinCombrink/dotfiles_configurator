use super::*;

fn ripgrep() -> CrateName {
    CrateName::from("ripgrep")
}

fn listed(lines: &[(&str, bool)]) -> String {
    lines
        .iter()
        .map(|(version, yanked)| {
            format!(
                "{{\"name\":\"ripgrep\",\"vers\":\"{version}\",\"deps\":[],\"cksum\":\"00\",\
                 \"features\":{{}},\"yanked\":{yanked}}}\n"
            )
        })
        .collect()
}

fn version(spelled: &str) -> CrateVersion {
    CrateVersion::try_from(spelled).unwrap()
}

#[test]
fn the_newest_release_is_the_highest_version_rather_than_the_last_one_listed() {
    let index = listed(&[("14.1.1", false), ("15.1.0", false), ("14.1.2", false)]);

    assert_eq!(
        newest_listed(&ripgrep(), &index).unwrap(),
        version("15.1.0")
    );
}

#[test]
fn a_yanked_version_is_never_the_newest_release() {
    let index = listed(&[("14.1.1", false), ("15.1.0", true)]);

    assert_eq!(
        newest_listed(&ripgrep(), &index).unwrap(),
        version("14.1.1")
    );
}

#[test]
fn a_pre_release_is_never_the_newest_release() {
    let index = listed(&[("14.1.1", false), ("15.0.0-beta.1", false)]);

    assert_eq!(
        newest_listed(&ripgrep(), &index).unwrap(),
        version("14.1.1")
    );
}

#[test]
fn a_crate_with_nothing_but_yanked_versions_has_no_newest_release() {
    let refusal = newest_listed(&ripgrep(), &listed(&[("14.1.1", true)])).unwrap_err();

    let CrateIndexReadFailure::NoRelease { crate_name } = refusal else {
        panic!("{refusal:?}");
    };
    assert_eq!(crate_name, ripgrep());
}

#[test]
fn a_line_that_is_not_an_index_entry_is_refused_rather_than_skipped() {
    let index = format!("{}not json\n", listed(&[("14.1.1", false)]));

    let refusal = newest_listed(&ripgrep(), &index).unwrap_err();

    let CrateIndexReadFailure::EntryUnparsed { .. } = refusal else {
        panic!("{refusal:?}");
    };
}

#[test]
fn a_listed_version_that_is_not_one_is_refused_holding_what_was_listed() {
    let refusal = newest_listed(&ripgrep(), &listed(&[("fourteen", false)])).unwrap_err();

    let CrateIndexReadFailure::ListsAnUnreadableVersion { listed, .. } = refusal else {
        panic!("{refusal:?}");
    };
    assert_eq!(listed, "fourteen");
}

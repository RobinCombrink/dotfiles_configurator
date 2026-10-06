use {
    crate::configuration::{CrateName, CrateVersion},
    serde::Deserialize,
};

pub const SPARSE_INDEX: &str = "https://index.crates.io";

/// The file of the crates.io sparse index that lists every published version of a crate.
///
/// ```
/// # use dotfiles_configurator::{
/// #     configuration::CrateName, machine::crate_index_reading::index_file_of,
/// # };
/// assert_eq!(index_file_of(&CrateName::from("a")), "1/a");
/// assert_eq!(index_file_of(&CrateName::from("jq")), "2/jq");
/// assert_eq!(index_file_of(&CrateName::from("Cfg")), "3/c/cfg");
/// assert_eq!(index_file_of(&CrateName::from("ripgrep")), "ri/pg/ripgrep");
/// ```
pub fn index_file_of(crate_name: &CrateName) -> String {
    let name = crate_name.as_ref().to_ascii_lowercase();

    match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    }
}

#[derive(Deserialize)]
struct ListedVersion {
    vers: String,
    yanked: bool,
}

// 2026-10-06: `https://index.crates.io/ri/pg/ripgrep` held one JSON object per line, one per
// published version, carrying the version as `vers` and a boolean `yanked`, and a crate crates.io
// does not hold answered 404.
pub fn newest_listed(crate_name: &CrateName, index_file: &str) -> Result<CrateVersion, String> {
    let mut newest: Option<semver::Version> = None;
    for line in index_file.lines().filter(|line| !line.trim().is_empty()) {
        let listed: ListedVersion = serde_json::from_str(line).map_err(|error| {
            format!("the crates.io index entry for {crate_name} could not be read: {error}")
        })?;
        let version = semver::Version::parse(&listed.vers).map_err(|error| {
            format!(
                "the crates.io index lists {crate_name} at {:?}, which is not a version: {error}",
                listed.vers
            )
        })?;
        if listed.yanked || !version.pre.is_empty() {
            continue;
        }
        if newest.as_ref().is_none_or(|held| version > *held) {
            newest = Some(version);
        }
    }

    let Some(newest) = newest else {
        return Err(format!(
            "crates.io lists no release of {crate_name} that is neither yanked nor a pre-release"
        ));
    };
    CrateVersion::try_from(newest.to_string().as_str())
}

#[cfg(test)]
mod tests {
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

        assert_eq!(newest_listed(&ripgrep(), &index), Ok(version("15.1.0")));
    }

    #[test]
    fn a_yanked_version_is_never_the_newest_release() {
        let index = listed(&[("14.1.1", false), ("15.1.0", true)]);

        assert_eq!(newest_listed(&ripgrep(), &index), Ok(version("14.1.1")));
    }

    #[test]
    fn a_pre_release_is_never_the_newest_release() {
        let index = listed(&[("14.1.1", false), ("15.0.0-beta.1", false)]);

        assert_eq!(newest_listed(&ripgrep(), &index), Ok(version("14.1.1")));
    }

    #[test]
    fn a_crate_with_nothing_but_yanked_versions_has_no_newest_release() {
        let refusal = newest_listed(&ripgrep(), &listed(&[("14.1.1", true)])).unwrap_err();

        assert!(refusal.contains("ripgrep"), "{refusal}");
    }

    #[test]
    fn a_line_that_is_not_an_index_entry_is_refused_rather_than_skipped() {
        let index = format!("{}not json\n", listed(&[("14.1.1", false)]));

        assert!(newest_listed(&ripgrep(), &index).is_err());
    }
}

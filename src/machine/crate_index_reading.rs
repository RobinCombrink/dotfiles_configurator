use {
    crate::configuration::{CrateName, CrateVersion},
    reqwest::StatusCode,
    serde::Deserialize,
    std::{fmt::Display, sync::Arc},
};

pub const SPARSE_INDEX: &str = "https://index.crates.io";

#[derive(Debug, Clone)]
pub enum CrateIndexReadFailure {
    Unasked {
        crate_name: CrateName,
        error: Arc<reqwest::Error>,
    },
    NoCrateNamed {
        crate_name: CrateName,
    },
    Refused {
        crate_name: CrateName,
        status: StatusCode,
    },
    ListingUnread {
        crate_name: CrateName,
        error: Arc<reqwest::Error>,
    },
    EntryUnparsed {
        crate_name: CrateName,
        error: Arc<serde_json::Error>,
    },
    ListsAnUnreadableVersion {
        crate_name: CrateName,
        listed: String,
        error: Arc<semver::Error>,
    },
    NoRelease {
        crate_name: CrateName,
    },
}

impl Display for CrateIndexReadFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrateIndexReadFailure::Unasked { crate_name, error } => {
                write!(
                    formatter,
                    "Could not ask crates.io for {crate_name}: {error}"
                )
            }
            CrateIndexReadFailure::NoCrateNamed { crate_name } => {
                write!(formatter, "crates.io holds no crate named {crate_name}")
            }
            CrateIndexReadFailure::Refused { crate_name, status } => {
                write!(
                    formatter,
                    "crates.io refused to list {crate_name}: {status}"
                )
            }
            CrateIndexReadFailure::ListingUnread { crate_name, error } => write!(
                formatter,
                "Could not read what crates.io lists for {crate_name}: {error}"
            ),
            CrateIndexReadFailure::EntryUnparsed { crate_name, error } => write!(
                formatter,
                "the crates.io index entry for {crate_name} could not be read: {error}"
            ),
            CrateIndexReadFailure::ListsAnUnreadableVersion {
                crate_name,
                listed,
                error,
            } => write!(
                formatter,
                "the crates.io index lists {crate_name} at {listed:?}, which is not a version: \
                 {error}"
            ),
            CrateIndexReadFailure::NoRelease { crate_name } => write!(
                formatter,
                "crates.io lists no release of {crate_name} that is neither yanked nor a \
                 pre-release"
            ),
        }
    }
}

impl std::error::Error for CrateIndexReadFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CrateIndexReadFailure::Unasked { error, .. }
            | CrateIndexReadFailure::ListingUnread { error, .. } => Some(error.as_ref()),
            CrateIndexReadFailure::EntryUnparsed { error, .. } => Some(error.as_ref()),
            CrateIndexReadFailure::ListsAnUnreadableVersion { error, .. } => Some(error.as_ref()),
            CrateIndexReadFailure::NoCrateNamed { .. }
            | CrateIndexReadFailure::Refused { .. }
            | CrateIndexReadFailure::NoRelease { .. } => None,
        }
    }
}

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
pub fn newest_listed(
    crate_name: &CrateName,
    index_file: &str,
) -> Result<CrateVersion, CrateIndexReadFailure> {
    let mut newest: Option<semver::Version> = None;
    for line in index_file.lines().filter(|line| !line.trim().is_empty()) {
        let listed: ListedVersion =
            serde_json::from_str(line).map_err(|error| CrateIndexReadFailure::EntryUnparsed {
                crate_name: crate_name.clone(),
                error: Arc::new(error),
            })?;
        let version = match semver::Version::parse(&listed.vers) {
            Ok(version) => version,
            Err(error) => {
                return Err(CrateIndexReadFailure::ListsAnUnreadableVersion {
                    crate_name: crate_name.clone(),
                    listed: listed.vers,
                    error: Arc::new(error),
                });
            }
        };
        if listed.yanked || !version.pre.is_empty() {
            continue;
        }
        if newest.as_ref().is_none_or(|held| version > *held) {
            newest = Some(version);
        }
    }

    newest
        .map(CrateVersion::from)
        .ok_or_else(|| CrateIndexReadFailure::NoRelease {
            crate_name: crate_name.clone(),
        })
}

#[cfg(test)]
mod tests;

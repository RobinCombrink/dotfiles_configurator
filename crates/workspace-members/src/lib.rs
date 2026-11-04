#![cfg_attr(test, allow(clippy::disallowed_macros))]

use {
    serde::Deserialize,
    std::fmt::{Debug, Display, Formatter},
};

#[cfg(test)]
mod tests;

const EVERY_CHILD: &str = "/*";
const GLOB_CHARACTERS: [char; 3] = ['*', '?', '['];

#[derive(Debug)]
pub enum MembersRefusal<ListingError> {
    ManifestUnreadable(toml::de::Error),
    NoWorkspace,
    MemberIsAGlob(String),
    ChildrenUnlisted { parent: String, cause: ListingError },
}

impl<ListingError: Display> Display for MembersRefusal<ListingError> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ManifestUnreadable(error) => {
                write!(formatter, "its Cargo.toml could not be read: {error}")
            }
            Self::NoWorkspace => formatter.write_str("its Cargo.toml declares no [workspace]"),
            Self::MemberIsAGlob(member) => write!(
                formatter,
                "its [workspace] members entry \"{member}\" is a glob other than every \
                 directory under one parent, which is not resolved"
            ),
            Self::ChildrenUnlisted { parent, cause } => write!(
                formatter,
                "the crates under \"{parent}\" could not be listed: {cause}"
            ),
        }
    }
}

impl<ListingError: Debug + Display> std::error::Error for MembersRefusal<ListingError> {}

#[derive(Deserialize)]
struct WorkspaceDocument {
    workspace: Option<WorkspaceSection>,
}

#[derive(Deserialize)]
struct WorkspaceSection {
    #[serde(default)]
    members: Vec<String>,
}

pub fn members_of<ListingError>(
    workspace_manifest: &str,
    crates_under: impl Fn(&str) -> Result<Vec<String>, ListingError>,
) -> Result<Vec<String>, MembersRefusal<ListingError>> {
    let document: WorkspaceDocument =
        toml::from_str(workspace_manifest).map_err(MembersRefusal::ManifestUnreadable)?;
    let Some(workspace) = document.workspace else {
        return Err(MembersRefusal::NoWorkspace);
    };

    let mut members = Vec::new();
    for member in workspace.members {
        let parent = member.strip_suffix(EVERY_CHILD).unwrap_or(&member);
        if parent.contains(GLOB_CHARACTERS) {
            return Err(MembersRefusal::MemberIsAGlob(member));
        }
        if parent.len() == member.len() {
            members.push(member);
            continue;
        }
        let mut children =
            crates_under(parent).map_err(|cause| MembersRefusal::ChildrenUnlisted {
                parent: parent.to_owned(),
                cause,
            })?;
        children.sort();
        members.extend(
            children
                .into_iter()
                .map(|child| format!("{parent}/{child}")),
        );
    }
    Ok(members)
}

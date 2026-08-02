use {
    crate::configuration::resource::GitHubRepository,
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::fmt::Display,
};

#[derive(
    Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct CargoWorkspace {
    pub repository: GitHubRepository,
}

impl Display for CargoWorkspace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "the cargo workspace in {}", self.repository)
    }
}

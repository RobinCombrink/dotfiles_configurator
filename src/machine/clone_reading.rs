use std::{fmt::Display, path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
pub enum CloneReadFailure {
    Unopened {
        clone: PathBuf,
        error: Arc<git2::Error>,
    },
}

impl Display for CloneReadFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloneReadFailure::Unopened { clone, error } => write!(
                formatter,
                "Could not open the clone at {}: {error}",
                clone.display()
            ),
        }
    }
}

impl std::error::Error for CloneReadFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CloneReadFailure::Unopened { error, .. } => Some(error.as_ref()),
        }
    }
}

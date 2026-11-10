use {
    crate::machine::clone_reading::CloneReadFailure,
    std::{path::Path, sync::Arc},
};

pub fn is_shallow(clone_directory: &Path) -> Result<bool, CloneReadFailure> {
    let clone =
        git2::Repository::open(clone_directory).map_err(|error| CloneReadFailure::Unopened {
            clone: clone_directory.to_path_buf(),
            error: Arc::new(error),
        })?;
    Ok(clone.is_shallow())
}

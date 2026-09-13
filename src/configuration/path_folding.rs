use std::path::{Path, PathBuf};

pub(crate) fn home_relative_path(home_directory: &Path, declared: &Path) -> PathBuf {
    match declared.is_absolute() {
        true => declared.to_path_buf(),
        false => home_directory.join(declared),
    }
}

pub(crate) fn comparable(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

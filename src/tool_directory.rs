use std::path::{Path, PathBuf};

// ADR 0015
const TOOL_DIRECTORY: &str = concat!(".", env!("CARGO_PKG_NAME"));

pub fn tool_directory_within(home_directory: &Path) -> PathBuf {
    home_directory.join(TOOL_DIRECTORY)
}

#[cfg(test)]
mod tests;

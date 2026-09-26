pub mod configuration;
pub mod configuration_source;
pub mod confirmation;
pub mod convergence;
pub mod currency;
pub mod desired_state;
pub mod github;
pub mod machine;
pub mod reporting;
pub mod version;

// ADR 0015
pub const TOOL_DIRECTORY: &str = concat!(".", env!("CARGO_PKG_NAME"));

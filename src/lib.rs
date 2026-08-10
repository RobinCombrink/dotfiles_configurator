//! Brings a machine into the state a declarative configuration describes by reading what is
//! already true and changing only what is not.

pub mod configuration;
pub mod configuration_source;
pub mod convergence;
pub mod desired_state;
pub mod github;
pub mod machine;
pub mod reporting;
pub mod version;

// ADR 0015
pub const TOOL_DIRECTORY: &str = concat!(".", env!("CARGO_PKG_NAME"));

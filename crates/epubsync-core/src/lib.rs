pub mod config;
pub mod epub;
pub mod kepub;
pub mod library;
pub mod metadata;
pub mod opf;
pub mod sort_name;
pub mod splice;

/// The crate version, shown by the CLI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

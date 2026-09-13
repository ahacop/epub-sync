pub mod config;
pub mod device;
pub mod epub;
pub mod kepub;
pub mod kobo;
pub mod library;
pub mod metadata;
pub mod opf;
pub mod sort_name;
pub mod splice;
pub mod sync;

/// The crate version, shown by the CLI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

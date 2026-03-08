// clipd-core: shared library for clipboard history daemon
//
// Re-exports the core modules so they can be used by both the CLI binary
// and the Tauri app shell.

pub mod clipboard;
pub mod config;
pub mod daemon;
pub mod models;
pub mod store;

pub use config::Config;
pub use models::{ClipEntry, ContentType};
pub use store::Store;

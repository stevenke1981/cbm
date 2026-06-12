pub mod agent;
pub mod cli;
pub mod discover;
pub mod symbol_id;
pub mod error;
pub mod git;
pub mod hooks;
pub mod http;
pub mod install;
pub mod mcp;
pub mod persistence;
pub mod pipeline;
pub mod project;
pub mod rlm;
pub mod runtime;
pub mod semantic;
pub mod store;
pub mod watcher;

pub mod test_lock;

pub use error::{Error, Result};
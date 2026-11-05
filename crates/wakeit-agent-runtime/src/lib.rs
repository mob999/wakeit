use wakeit_plugin_sdk::session_catalog as catalog;
pub use wakeit_plugin_sdk::{protocol, trace};
pub mod agy;
pub mod cline;
pub mod copilot;
pub mod discovery;
pub mod gemini;
pub mod goose;
pub mod kimi;
pub mod kimi_acp;
pub mod qwen_cursor;
pub mod sessions;
pub mod stream;

pub mod official;
pub mod official_args;
mod official_io;

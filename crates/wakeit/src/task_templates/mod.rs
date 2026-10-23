//! Templates are compiled before Task creation, never during execution.
mod compiler;
pub mod schema;
mod types;

pub use compiler::{compile, from_task, validate_definition};
pub use schema::{InputSchema, resolve_inputs};
pub use types::*;

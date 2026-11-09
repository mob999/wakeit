//! Public version 1 plugin contracts, independent of Wakeit hosts and clients.
pub mod adapter;
pub mod agent;
pub mod capture;
pub mod extensions;
pub mod protocol;
pub mod session_catalog;
pub mod sessions;
pub mod trace;

use anyhow::{Result, ensure};

pub fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 80
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "id must use 1–80 letters, numbers, hyphens or underscores"
    );
    Ok(())
}

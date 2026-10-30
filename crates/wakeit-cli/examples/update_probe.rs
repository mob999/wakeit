//! Run only from an isolated copied executable, never from a live installation.
use anyhow::{Context, Result};
#[tokio::main]
async fn main() -> Result<()> {
    let candidate = std::env::args_os()
        .nth(1)
        .context("candidate path required")?;
    let candidate = std::path::Path::new(&candidate);
    let version = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    if candidate.extension().is_some_and(|v| v == "gz") {
        wakeit::update::replace_bundle(&std::fs::read(candidate)?, &version).await
    } else {
        wakeit::update::replace_verified(candidate, &version).await
    }
}

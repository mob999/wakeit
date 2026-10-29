//! Run only from an isolated copied executable, never from a live installation.
use anyhow::{Context, Result};
#[tokio::main]
async fn main() -> Result<()> {
    let candidate = std::env::args_os()
        .nth(1)
        .context("candidate path required")?;
    wakeit::update::replace_verified(
        std::path::Path::new(&candidate),
        &semver::Version::parse(env!("CARGO_PKG_VERSION"))?,
    )
    .await
}

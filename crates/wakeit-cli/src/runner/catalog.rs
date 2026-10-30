//! Read-only metadata discovery. The device, not the Gateway OS, validates native paths.
use crate::{
    config::{AgentConfig, TaskConfig},
    process,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use uuid::Uuid;
#[cfg(test)]
use wakeit_core::session_catalog::Entry;
use wakeit_core::session_catalog::{Catalog, paginate};
pub async fn query(
    agent: &AgentConfig,
    cwd: &str,
    q: Option<&str>,
    cursor: Option<&str>,
) -> Result<Value> {
    let spec = agent
        .session
        .as_ref()
        .and_then(|s| s.list.as_ref())
        .context("adapter has no session list interface")?;
    let cwd = Path::new(cwd).canonicalize()?;
    let task: TaskConfig = serde_json::from_value(
        json!({"version":1,"id":"session-list","name":"Session list","runner":"local","agent":agent.id,"cwd":cwd,"prompt":"","schedule":null,"parameters":crate::config::resolve_parameters(&agent.parameters,&Default::default())?}),
    )?;
    let out = process::execute(
        spec,
        &task,
        Uuid::new_v4(),
        None,
        Duration::from_secs(25),
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await?;
    ensure!(
        out.code == Some(0) && out.stopped.is_none() && out.confirmed,
        "session list command failed or timed out"
    );
    let catalog: Catalog = serde_json::from_str(&out.stdout).context("invalid session catalog")?;
    ensure!(
        catalog.version == 1 && catalog.sessions.len() <= 10000 && catalog.next_cursor.is_none(),
        "session catalog exceeded its limit or is incomplete"
    );
    page(catalog, &cwd, q, cursor)
}
fn page(catalog: Catalog, cwd: &Path, q: Option<&str>, cursor: Option<&str>) -> Result<Value> {
    let cwd = cwd.canonicalize()?;
    let mut ids = std::collections::HashSet::new();
    for entry in &catalog.sessions {
        crate::runner::sessions::validate_id(&entry.id)?;
        ensure!(ids.insert(&entry.id), "duplicate session ID");
        ensure!(
            entry.title.as_ref().is_none_or(|s| s.len() <= 4096),
            "session title is too long"
        );
        ensure!(
            Path::new(&entry.cwd).canonicalize()? == cwd,
            "session catalog includes another directory"
        );
    }
    Ok(serde_json::to_value(paginate(
        catalog.sessions,
        &cwd.to_string_lossy(),
        q,
        cursor,
        20,
    )?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_pages_and_rejects_cross_directory_or_duplicate_ids() {
        let d = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let entries = (0..43)
            .map(|i| Entry {
                id: format!("ses_{i:03}"),
                title: Some(format!("中文 review {i}")),
                cwd: d.path().to_string_lossy().into(),
                updated_at: Some(chrono::Utc::now()),
            })
            .collect::<Vec<_>>();
        let c = || Catalog {
            version: 1,
            sessions: entries.clone(),
            next_cursor: None,
        };
        let first = page(c(), d.path(), Some("中文"), None).unwrap();
        assert_eq!(first["sessions"].as_array().unwrap().len(), 20);
        let second = page(c(), d.path(), Some("中文"), first["next_cursor"].as_str()).unwrap();
        assert_eq!(second["sessions"].as_array().unwrap().len(), 20);
        let third = page(c(), d.path(), Some("中文"), second["next_cursor"].as_str()).unwrap();
        assert_eq!(third["sessions"].as_array().unwrap().len(), 3);
        assert!(third["next_cursor"].is_null());
        assert!(
            page(
                c(),
                d.path(),
                Some("other search"),
                first["next_cursor"].as_str()
            )
            .is_err()
        );
        assert_eq!(
            page(c(), d.path(), Some("missing"), None).unwrap()["sessions"],
            json!([])
        );
        assert!(page(c(), other.path(), None, None).is_err());
        let mut dup = c();
        dup.sessions.push(entries[0].clone());
        assert!(page(dup, d.path(), None, None).is_err());
        assert!(page(c(), d.path(), None, Some("bad-cursor")).is_err());
    }
}

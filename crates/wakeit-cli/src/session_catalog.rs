//! Read-only metadata discovery. The device, not the Gateway OS, validates native paths.
use crate::{
    config::{AgentConfig, TaskConfig},
    process,
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use uuid::Uuid;
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub title: Option<String>,
    pub cwd: String,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub sessions: Vec<Entry>,
    pub next_cursor: Option<String>,
}
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
        crate::sessions::validate_id(&entry.id)?;
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

pub(crate) fn paginate(
    mut entries: Vec<Entry>,
    scope: &str,
    q: Option<&str>,
    cursor: Option<&str>,
    limit: usize,
) -> Result<Catalog> {
    ensure!(
        (1..=10000).contains(&limit),
        "session page size must be between 1 and 10000"
    );
    entries.sort_by(|a, b| (&b.updated_at, &b.id).cmp(&(&a.updated_at, &a.id)));
    let q = q.unwrap_or("").trim().to_lowercase();
    ensure!(q.len() <= 512, "session search is too long");
    ensure!(
        cursor.is_none_or(|c| c.len() <= 1024),
        "invalid session cursor"
    );
    use sha2::{Digest, Sha256};
    let scope = hex::encode(Sha256::digest(serde_json::to_vec(&(scope, &q))?));
    let after: Option<(String, Option<chrono::DateTime<chrono::Utc>>, String)> = cursor
        .map(|c| Ok::<_, anyhow::Error>(serde_json::from_slice(&URL_SAFE_NO_PAD.decode(c)?)?))
        .transpose()?;
    ensure!(
        after.as_ref().is_none_or(|(s, _, _)| s == &scope),
        "session query changed; start from the first page"
    );
    entries.retain(|s| {
        after
            .as_ref()
            .is_none_or(|(_, at, id)| (&s.updated_at, &s.id) < (at, id))
            && (q.is_empty()
                || s.id.to_lowercase().contains(&q)
                || s.title
                    .as_ref()
                    .is_some_and(|v| v.to_lowercase().contains(&q)))
    });
    let next = if entries.len() > limit {
        let last = &entries[limit - 1];
        Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&(&scope, last.updated_at, &last.id))?))
    } else {
        None
    };
    entries.truncate(limit);
    Ok(Catalog {
        version: 1,
        sessions: entries,
        next_cursor: next,
    })
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

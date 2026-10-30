use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
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
pub fn paginate(
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

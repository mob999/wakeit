//! Read-only Kimi Code native session inspection and discovery.
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

/// Metadata read from Kimi's native index and persisted session state only.
#[derive(Debug, Clone)]
pub struct KimiSession {
    pub id: String,
    pub title: Option<String>,
    pub cwd: PathBuf,
    pub updated_at: Option<DateTime<Utc>>,
    session_dir: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexEntry {
    session_id: String,
    work_dir: PathBuf,
    session_dir: PathBuf,
}

pub(crate) fn kimi_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("KIMI_CODE_HOME").filter(|s| !s.is_empty()) {
        return Ok(path.into());
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|path| PathBuf::from(path).join(".kimi-code"))
        .context("user home directory unavailable")
}

/// Never boots Kimi's SDK, reads config/credentials, or opens conversation logs.
/// Current official create/fork paths append the native session_index.jsonl.
/// The newest mapping must still agree with persisted state.json and cwd.
pub async fn session_summaries(cwd: &Path) -> Result<Vec<KimiSession>> {
    tokio::time::timeout(Duration::from_secs(20), sessions_in(&kimi_home()?, cwd))
        .await
        .context("Kimi session listing timed out")?
}

async fn sessions_in(home: &Path, cwd: &Path) -> Result<Vec<KimiSession>> {
    let cwd = std::fs::canonicalize(cwd).context("working directory unavailable")?;
    let file = match tokio::fs::File::open(home.join("session_index.jsonl")).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error).context("read Kimi session index"),
    };
    ensure!(
        file.metadata().await?.is_file(),
        "Kimi session index is not a file"
    );
    let mut reader = BufReader::new(file);
    let mut entries = BTreeMap::new();
    let mut total = 0;
    loop {
        let mut bytes = Vec::new();
        let count = (&mut reader)
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut bytes)
            .await?;
        if count == 0 {
            break;
        }
        total += count;
        ensure!(
            count <= 1024 * 1024 && total <= 64 * 1024 * 1024,
            "Kimi session index exceeded limit"
        );
        // An incomplete trailing append or unrelated corrupt entry is never a
        // source of identity. Later complete mappings supersede earlier ones.
        let Ok(item) = serde_json::from_slice::<IndexEntry>(&bytes) else {
            continue;
        };
        if crate::sessions::validate_id(&item.session_id).is_err() {
            continue;
        }
        entries.insert(item.session_id.clone(), item);
        ensure!(
            entries.len() <= 100_000,
            "Kimi session index exceeded limit"
        );
    }
    let mut sessions = Vec::new();
    for item in entries.into_values() {
        if std::fs::canonicalize(&item.work_dir).ok().as_ref() != Some(&cwd) {
            continue;
        }
        let mut session = KimiSession {
            id: item.session_id,
            title: None,
            cwd: cwd.clone(),
            updated_at: None,
            session_dir: item.session_dir,
        };
        if let Some(metadata) = persisted_metadata(&session).await? {
            session.title = metadata["title"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.chars().take(512).collect());
            session.updated_at = metadata["updatedAt"]
                .as_i64()
                .and_then(DateTime::from_timestamp_millis);
            sessions.push(session);
        }
    }
    Ok(sessions)
}

async fn persisted_metadata(item: &KimiSession) -> Result<Option<Value>> {
    // Kimi's official session lifecycle stores metadata in sessionDir/state.json.
    // Paths come from its native index, never from an external session ID.
    let file = match tokio::fs::File::open(item.session_dir.join("state.json")).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("read Kimi session metadata"),
    };
    const LIMIT: usize = 1024 * 1024;
    let metadata = file.metadata().await?;
    if !metadata.is_file() || metadata.len() > LIMIT as u64 {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > LIMIT {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(None);
    };
    let actual_cwd = value["cwd"].as_str().or(value["workDir"].as_str());
    if value["id"].as_str() != Some(item.id.as_str())
        || actual_cwd
            .and_then(|cwd| std::fs::canonicalize(cwd).ok())
            .as_ref()
            != Some(&item.cwd)
    {
        return Ok(None);
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn native_index_is_project_scoped_and_uses_newest_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let first = home.path().join("first");
        let latest = home.path().join("latest");
        tokio::fs::create_dir(&first).await.unwrap();
        tokio::fs::create_dir(&latest).await.unwrap();
        tokio::fs::write(
            first.join("state.json"),
            serde_json::to_vec(&json!({"id":"session_one","cwd":dir.path(),"title":"Stale"}))
                .unwrap(),
        )
        .await
        .unwrap();
        tokio::fs::write(latest.join("state.json"), serde_json::to_vec(&json!({"id":"session_one","cwd":dir.path(),"updatedAt":2000,"lastPrompt":"private prompt"})).unwrap()).await.unwrap();
        let rows = [
            json!({"sessionId":"session_one","workDir":dir.path(),"sessionDir":first}),
            json!({"sessionId":"session_one","workDir":dir.path(),"sessionDir":latest}),
            json!({"sessionId":"session_moved","workDir":dir.path(),"sessionDir":first}),
            json!({"sessionId":"session_moved","workDir":other.path(),"sessionDir":first}),
            json!({"sessionId":"session_missing","workDir":dir.path(),"sessionDir":home.path().join("missing")}),
            json!({"sessionId":"session_other","workDir":other.path(),"sessionDir":home.path().join("unopened")}),
            json!({"sessionId":"../invalid","workDir":dir.path(),"sessionDir":latest}),
        ];
        let index = rows
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n{partial";
        tokio::fs::write(home.path().join("session_index.jsonl"), index)
            .await
            .unwrap();
        let sessions = sessions_in(home.path(), dir.path()).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "session_one");
        assert_eq!(sessions[0].title, None);
        assert_eq!(sessions[0].updated_at.unwrap().timestamp_millis(), 2000);
        assert!(!home.path().join("config.toml").exists());
        assert!(!home.path().join("unopened").exists());
    }

    #[tokio::test]
    async fn missing_storage_is_empty_and_never_initialized() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("missing-home");
        assert!(sessions_in(&home, dir.path()).await.unwrap().is_empty());
        assert!(!home.exists());
    }

    #[tokio::test]
    async fn persisted_metadata_requires_exact_id_and_directory() {
        let dir = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let item = KimiSession {
            id: "session_one".into(),
            title: None,
            cwd: std::fs::canonicalize(dir.path()).unwrap(),
            updated_at: None,
            session_dir: storage.path().into(),
        };
        assert!(persisted_metadata(&item).await.unwrap().is_none());
        let path = storage.path().join("state.json");
        tokio::fs::write(
            &path,
            serde_json::to_vec(&json!({"id":"session_one","cwd":dir.path()})).unwrap(),
        )
        .await
        .unwrap();
        assert!(persisted_metadata(&item).await.unwrap().is_some());
        tokio::fs::write(
            &path,
            serde_json::to_vec(&json!({"id":"session_other","cwd":dir.path()})).unwrap(),
        )
        .await
        .unwrap();
        assert!(persisted_metadata(&item).await.unwrap().is_none());
        tokio::fs::write(
            &path,
            serde_json::to_vec(&json!({"id":"session_one","cwd":storage.path()})).unwrap(),
        )
        .await
        .unwrap();
        assert!(persisted_metadata(&item).await.unwrap().is_none());
    }
}

//! Authorized chunked device files and versioned plugin artifacts.
use super::*;
use base64::Engine;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub(super) async fn read(runtime: &Runtime, params: &Value) -> Result<Value> {
    let path = std::path::PathBuf::from(params["path"].as_str().context("file path required")?);
    let canonical = path.canonicalize()?;
    let parent = canonical.parent().context("file parent missing")?;
    crate::process::validate_cwd(
        parent.to_str().context("invalid path")?,
        &runtime.config.allowed_roots,
    )?;
    let file = tokio::fs::File::open(&canonical).await?;
    let metadata = file.metadata().await?;
    ensure!(metadata.is_file(), "not a regular file");
    let modified = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let version = hex::encode(Sha256::digest(format!(
        "{}:{}:{modified}",
        canonical.display(),
        metadata.len()
    )));
    if let Some(expected) = params["revision"].as_str() {
        ensure!(expected == version, "file revision changed");
    }
    let descriptor = json!({"runner_id":runtime.config.id,"path":canonical,"size":metadata.len(),"modified_at":modified.to_string(),"revision":version});
    if params["operation"] == "stat" {
        return Ok(descriptor);
    }
    let mut file = file;
    let offset = params["offset"].as_u64().unwrap_or(0);
    let limit = params["limit"]
        .as_u64()
        .unwrap_or(64 * 1024)
        .min(256 * 1024);
    ensure!(offset <= metadata.len(), "file offset exceeds size");
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).await?;
    Ok(
        json!({"reference":descriptor,"data":base64::engine::general_purpose::STANDARD.encode(&bytes),"encoding":"base64","offset":offset,"next_offset":offset+bytes.len() as u64,"eof":offset+bytes.len() as u64>=metadata.len()}),
    )
}
pub(super) async fn register(runtime: &Runtime, instance: Uuid, params: &Value) -> Result<Value> {
    let reference = read(runtime, &json!({"operation":"stat","path":params["path"]})).await?;
    let proposal=runtime.hooks("artifact.export.prepare",json!({"reference":reference,"metadata":params.get("metadata").cloned().unwrap_or_else(||json!({}))}),runtime.stop.child_token()).await?;
    let id = Uuid::new_v4();
    sqlx::query("CREATE TABLE IF NOT EXISTS extension_artifacts(id TEXT PRIMARY KEY,instance TEXT NOT NULL,name TEXT NOT NULL,reference TEXT NOT NULL,metadata TEXT NOT NULL,expires_at TEXT)").execute(&runtime.journal.pool).await?;
    sqlx::query("INSERT INTO extension_artifacts(id,instance,name,reference,metadata,expires_at) VALUES(?,?,?,?,?,?)").bind(id.to_string()).bind(instance.to_string()).bind(params["name"].as_str().unwrap_or("Artifact")).bind(reference.to_string()).bind(proposal["metadata"].to_string()).bind(params["expires_at"].as_str()).execute(&runtime.journal.pool).await?;
    runtime.journal.extension_event("artifact.registered",json!({"artifact_id":id,"runner_id":runtime.config.id,"instance_id":instance,"name":params["name"],"reference":reference,"metadata":proposal["metadata"],"expires_at":params["expires_at"]})).await?;
    Ok(json!({"id":id,"reference":reference,"metadata":proposal["metadata"]}))
}

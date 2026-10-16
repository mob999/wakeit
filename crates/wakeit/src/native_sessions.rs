//! Read-only native session resolution. No inspection path submits a prompt or creates a session.
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

const METADATA_LIMIT: usize = 1024 * 1024;

fn canonical_directory(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = std::fs::canonicalize(path).context("working directory is unavailable")?;
    // Node and native CLIs encode regular Windows paths, not Rust's verbatim prefix.
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(unc) = value.strip_prefix("\\\\?\\UNC\\") {
            return Ok(PathBuf::from(format!("\\\\{unc}")));
        }
        if let Some(value) = value.strip_prefix("\\\\?\\") {
            return Ok(PathBuf::from(value));
        }
    }
    Ok(path)
}

fn validate_id(protocol: &str, id: &str) -> Result<()> {
    ensure!(!id.is_empty() && id.len() <= 256, "invalid session ID");
    match protocol {
        "codex" | "claude-code" => {
            uuid::Uuid::parse_str(id).context("provide a complete session UUID")?;
        }
        "pi" => ensure!(
            id.starts_with(|c: char| c.is_ascii_alphanumeric())
                && id.ends_with(|c: char| c.is_ascii_alphanumeric())
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "provide a complete Pi session ID, not a path"
        ),
        "opencode" => ensure!(
            id.starts_with("ses_")
                && id[4..].bytes().all(|b| b.is_ascii_alphanumeric())
                && id.len() > 4,
            "invalid OpenCode session ID"
        ),
        "kimi-code" => crate::sessions::validate_id(id)?,
        _ => bail!("unsupported session protocol"),
    }
    Ok(())
}

fn home() -> Result<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .context("user home directory is unavailable")
}

fn expand_home(path: PathBuf) -> Result<PathBuf> {
    if let Ok(tail) = path.strip_prefix("~") {
        return Ok(home()?.join(tail));
    }
    Ok(path)
}

async fn pi_settings_session_dir(path: &Path) -> Result<Option<PathBuf>> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("read Pi session storage settings"),
    };
    ensure!(bytes.len() <= METADATA_LIMIT, "Pi settings exceeded limit");
    let settings: Value = serde_json::from_slice(&bytes).context("invalid Pi storage settings")?;
    settings
        .get("sessionDir")
        .filter(|value| !value.is_null())
        .map(|value| {
            let path = value
                .as_str()
                .filter(|s| !s.is_empty())
                .context("invalid Pi sessionDir setting")?;
            expand_home(PathBuf::from(path))
        })
        .transpose()
}

fn normalized_command(command: &str, args: &[String]) -> Result<Command> {
    let mut spec = crate::config::CommandSpec {
        command: command.into(),
        args: args.iter().cloned().map(Into::into).collect(),
        ..Default::default()
    };
    crate::config::normalize_native_command(&mut spec)?;
    let mut command = Command::new(spec.command);
    for arg in spec.args {
        if let crate::config::CommandArg::Literal(value) = arg {
            command.arg(value);
        }
    }
    command.kill_on_drop(true);
    Ok(command)
}

async fn metadata_line(
    reader: &mut (impl tokio::io::AsyncBufRead + Unpin),
) -> Result<Option<Value>> {
    let mut bytes = Vec::new();
    let count = reader
        .take((METADATA_LIMIT + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .await?;
    ensure!(count <= METADATA_LIMIT, "session metadata exceeded limit");
    if count == 0 {
        return Ok(None);
    }
    Ok(Some(
        serde_json::from_slice(&bytes).context("invalid session metadata")?,
    ))
}

fn verify_metadata(id: &str, cwd: &Path, actual_id: &str, actual_cwd: &str) -> Result<Value> {
    ensure!(
        actual_id == id,
        "session ID did not match the requested session"
    );
    let actual =
        canonical_directory(actual_cwd).context("session working directory is unavailable")?;
    ensure!(
        actual == cwd,
        "session belongs to a different working directory"
    );
    Ok(json!({"version":1,"id":id,"cwd":actual,"persisted":true,"title":null}))
}
fn titled(mut metadata: Value, title: Option<&str>) -> Value {
    metadata["title"] = json!(title);
    metadata
}

async fn codex(id: &str, cwd: &Path) -> Result<Value> {
    let mut command = normalized_command("codex", &["app-server".into()])?;
    command
        .current_dir(cwd)
        .env("PWD", cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().context("start Codex session inspection")?;
    let result = codex_reply(&mut child, id, cwd).await;
    child.kill().await.ok();
    let _ = child.wait().await;
    result
}

async fn codex_reply(child: &mut Child, id: &str, cwd: &Path) -> Result<Value> {
    let mut input = child.stdin.take().context("Codex stdin")?;
    let mut reader = BufReader::new(child.stdout.take().context("Codex stdout")?);
    for (request_id, request) in [
        (
            1,
            json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"wakeit","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":false}}}),
        ),
        (
            2,
            json!({"id":2,"method":"thread/read","params":{"threadId":id,"includeTurns":false}}),
        ),
    ] {
        input.write_all(format!("{request}\n").as_bytes()).await?;
        input.flush().await?;
        loop {
            let reply = metadata_line(&mut reader)
                .await?
                .context("Codex inspection exited without a response")?;
            if reply["id"] != request_id {
                continue;
            }
            ensure!(
                reply.get("error").is_none(),
                "Codex session not found or unavailable"
            );
            if request_id == 2 {
                let thread = &reply["result"]["thread"];
                ensure!(thread["ephemeral"] != true, "session is not persisted");
                return Ok(titled(
                    verify_metadata(
                        id,
                        cwd,
                        thread["id"].as_str().context("Codex session ID missing")?,
                        thread["cwd"]
                            .as_str()
                            .context("Codex session directory missing")?,
                    )?,
                    thread["name"].as_str(),
                ));
            }
            break;
        }
        input.write_all(b"{\"method\":\"initialized\"}\n").await?;
        input.flush().await?;
    }
    bail!("Codex session metadata missing")
}

async fn pi_directory(cwd: &Path) -> Result<PathBuf> {
    Ok(
        if let Some(custom) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR") {
            expand_home(PathBuf::from(custom))?
        } else {
            let agent = expand_home(
                std::env::var_os("PI_CODING_AGENT_DIR")
                    .map(PathBuf::from)
                    .unwrap_or(home()?.join(".pi/agent")),
            )?;
            ensure!(
                pi_settings_session_dir(&cwd.join(".pi/settings.json"))
                    .await?
                    .is_none(),
                "set PI_CODING_AGENT_SESSION_DIR explicitly when project settings override session storage"
            );
            if let Some(custom) = pi_settings_session_dir(&agent.join("settings.json")).await? {
                custom
            } else {
                let encoded = cwd.to_string_lossy();
                let encoded = encoded
                    .strip_prefix(['/', '\\'])
                    .unwrap_or(&encoded)
                    .replace(['/', '\\', ':'], "-");
                agent.join("sessions").join(format!("--{encoded}--"))
            }
        },
    )
}

async fn pi(id: &str, cwd: &Path) -> Result<Value> {
    let root = pi_directory(cwd).await?;
    let mut directory = tokio::fs::read_dir(&root)
        .await
        .context("Pi session directory is unavailable")?;
    let suffix = format!("_{id}.jsonl");
    let mut selected = None;
    while let Some(entry) = directory.next_entry().await? {
        if entry.file_name().to_string_lossy().ends_with(&suffix) {
            ensure!(selected.is_none(), "multiple Pi sessions matched the ID");
            selected = Some(entry.path());
        }
    }
    let file =
        tokio::fs::File::open(selected.context("Pi session not found in the selected directory")?)
            .await?;
    let mut reader = BufReader::new(file);
    let header = metadata_line(&mut reader)
        .await?
        .context("Pi session file is empty")?;
    ensure!(header["type"] == "session", "invalid Pi session header");
    let title = session_title(&mut reader, "pi").await?.1;
    Ok(titled(
        verify_metadata(
            id,
            cwd,
            header["id"].as_str().context("Pi session ID missing")?,
            header["cwd"]
                .as_str()
                .context("Pi session directory missing")?,
        )?,
        title.as_deref(),
    ))
}

fn claude_directory(cwd: &Path) -> Result<PathBuf> {
    let root = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".claude"));
    let project = if let Some(name) = std::env::var_os("CLAUDE_CODE_PROJECT_DIR_NAME") {
        let name = PathBuf::from(name);
        ensure!(
            name.components().count() == 1
                && matches!(
                    name.components().next(),
                    Some(std::path::Component::Normal(_))
                ),
            "invalid Claude project directory name"
        );
        name
    } else {
        let encoded: String = cwd
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        ensure!(
            encoded.len() <= 200,
            "Claude session metadata for this directory requires an explicit CLAUDE_CODE_PROJECT_DIR_NAME"
        );
        PathBuf::from(encoded)
    };
    Ok(root.join("projects").join(project))
}

async fn claude(id: &str, cwd: &Path) -> Result<Value> {
    let root = claude_directory(cwd)?;
    let file = tokio::fs::File::open(root.join(format!("{id}.jsonl")))
        .await
        .context("Claude session not found in the selected directory")?;
    let mut reader = BufReader::new(file).take(METADATA_LIMIT as u64);
    while let Some(record) = metadata_line(&mut reader).await? {
        let Some(actual_id) = record["sessionId"].as_str() else {
            continue;
        };
        let Some(actual_cwd) = record["cwd"].as_str() else {
            continue;
        };
        let metadata = verify_metadata(id, cwd, actual_id, actual_cwd)?;
        let mut names =
            BufReader::new(tokio::fs::File::open(root.join(format!("{id}.jsonl"))).await?);
        let (found, title) = session_title(&mut names, "claude-code").await?;
        let title = if found {
            title
        } else {
            claude_titles(&root).await?.remove(id)
        };
        return Ok(titled(metadata, title.as_deref()));
    }
    bail!("Claude session has no valid metadata in its header")
}

async fn opencode(id: &str, cwd: &Path) -> Result<Value> {
    let mut command = normalized_command(
        "opencode",
        &[
            "api".into(),
            "session.get".into(),
            "--param".into(),
            format!("sessionID={id}"),
        ],
    )?;
    command
        .current_dir(cwd)
        .env("PWD", cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .context("start OpenCode session inspection")?;
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .context("OpenCode stdout")?
        .take((METADATA_LIMIT + 1) as u64)
        .read_to_end(&mut output)
        .await?;
    ensure!(
        output.len() <= METADATA_LIMIT,
        "OpenCode metadata exceeded limit"
    );
    ensure!(
        child.wait().await?.success(),
        "OpenCode session not found or unavailable (requires session.get API)"
    );
    let reply: Value =
        serde_json::from_slice(&output).context("invalid OpenCode session metadata")?;
    let session = reply
        .get("data")
        .context("OpenCode session.get metadata missing")?;
    Ok(titled(
        verify_metadata(
            id,
            cwd,
            session["id"]
                .as_str()
                .context("OpenCode session ID missing")?,
            session["location"]["directory"]
                .as_str()
                .context("OpenCode session directory missing")?,
        )?,
        session["title"].as_str(),
    ))
}

/// Resolve one complete native ID, without reading conversation contents or invoking inference.
pub async fn inspect(protocol: &str, id: &str, cwd: &Path) -> Result<Value> {
    validate_id(protocol, id)?;
    let cwd = canonical_directory(cwd)?;
    ensure!(cwd.is_dir(), "working directory is not a directory");
    tokio::time::timeout(Duration::from_secs(15), async {
        match protocol {
            "codex" => codex(id, &cwd).await,
            "claude-code" => claude(id, &cwd).await,
            "pi" => pi(id, &cwd).await,
            "opencode" => opencode(id, &cwd).await,
            "kimi-code" => {
                let session = crate::kimi_adapter::session_summaries(&cwd)
                    .await?
                    .into_iter()
                    .find(|session| session.id == id)
                    .context("Kimi session does not exist in this working directory")?;
                let metadata =
                    verify_metadata(id, &cwd, &session.id, &session.cwd.to_string_lossy())?;
                Ok(titled(metadata, session.title.as_deref()))
            }
            _ => unreachable!(),
        }
    })
    .await
    .context("session inspection timed out")?
}

/// Keep inspection separate from submission; failed binding never falls back to a new session.
pub async fn resume(protocol: &str, id: &str, command: Vec<String>) -> Result<i32> {
    inspect(protocol, id, &std::env::current_dir()?).await?;
    crate::stream_adapter::stream_checked(protocol, &command, Some(id)).await
}

/// Name a newly created native conversation through its official metadata API.
pub async fn rename(protocol: &str, id: &str, title: &str, cwd: &Path) -> Result<()> {
    if protocol == "kimi-code" {
        validate_id(protocol, id)?;
        ensure!(
            !title.trim().is_empty() && title.len() <= 200 && !title.chars().any(char::is_control),
            "invalid session title"
        );
        return crate::kimi_live::rename(id, title, cwd).await;
    }
    ensure!(
        protocol == "codex",
        "native adapter has no session rename interface"
    );
    validate_id(protocol, id)?;
    ensure!(
        !title.trim().is_empty() && title.len() <= 4096,
        "invalid session title"
    );
    let cwd = canonical_directory(cwd)?;
    let mut command = normalized_command("codex", &["app-server".into()])?;
    command
        .current_dir(&cwd)
        .env("PWD", &cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().context("start Codex session naming")?;
    let result=tokio::time::timeout(Duration::from_secs(15),async {
        let mut input=child.stdin.take().context("Codex stdin")?;
        let mut reader=BufReader::new(child.stdout.take().context("Codex stdout")?);
        rpc(&mut input,&mut reader,1,"initialize",json!({"clientInfo":{"name":"wakeit","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":false}})).await?;
        input.write_all(b"{\"method\":\"initialized\"}\n").await?;
        let before=rpc(&mut input,&mut reader,2,"thread/read",json!({"threadId":id,"includeTurns":false})).await?;
        let thread=&before["thread"];
        ensure!(thread["ephemeral"]!=true,"session is not persisted");
        verify_metadata(id,&cwd,thread["id"].as_str().context("Codex session ID missing")?,thread["cwd"].as_str().context("Codex session directory missing")?)?;
        rpc(&mut input,&mut reader,3,"thread/name/set",json!({"threadId":id,"name":title})).await?;
        let after=rpc(&mut input,&mut reader,4,"thread/read",json!({"threadId":id,"includeTurns":false})).await?;
        ensure!(after["thread"]["name"].as_str()==Some(title),"Codex session title was not saved");
        Ok::<(),anyhow::Error>(())
    }).await.context("session naming timed out");
    child.kill().await.ok();
    let _ = child.wait().await;
    result?
}

#[derive(Clone, Serialize)]
struct CatalogEntry {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
    #[serde(skip)]
    timestamp: i64,
}
fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(value) = value.as_i64() {
        return if value.unsigned_abs() > 100_000_000_000 {
            DateTime::from_timestamp_millis(value)
        } else {
            DateTime::from_timestamp(value, 0)
        };
    }
    value
        .as_str()?
        .parse::<DateTime<chrono::FixedOffset>>()
        .ok()
        .map(|time| time.with_timezone(&Utc))
}
fn catalog_entry(
    protocol: &str,
    id: &str,
    cwd: &Path,
    title: Option<&str>,
    time: Option<DateTime<Utc>>,
) -> Result<CatalogEntry> {
    validate_id(protocol, id)?;
    Ok(CatalogEntry {
        id: id.into(),
        title: title
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.chars().take(512).collect()),
        cwd: cwd.to_string_lossy().into_owned(),
        updated_at: time.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        timestamp: time.map_or(0, |t| t.timestamp_millis()),
    })
}
async fn rpc(
    input: &mut tokio::process::ChildStdin,
    reader: &mut (impl tokio::io::AsyncBufRead + Unpin),
    id: i64,
    method: &str,
    params: Value,
) -> Result<Value> {
    input
        .write_all(format!("{}\n", json!({"id":id,"method":method,"params":params})).as_bytes())
        .await?;
    input.flush().await?;
    loop {
        let reply = metadata_line(reader)
            .await?
            .context("session catalog exited without a response")?;
        if reply["id"] != id {
            continue;
        }
        ensure!(
            reply.get("error").is_none(),
            "native session catalog query failed"
        );
        return Ok(reply["result"].clone());
    }
}
async fn codex_catalog(cwd: &Path) -> Result<Vec<CatalogEntry>> {
    let mut command = normalized_command("codex", &["app-server".into()])?;
    command
        .current_dir(cwd)
        .env("PWD", cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().context("start Codex session catalog")?;
    let result=async {
        let mut input=child.stdin.take().context("Codex stdin")?;
        let mut reader=BufReader::new(child.stdout.take().context("Codex stdout")?);
        rpc(&mut input,&mut reader,1,"initialize",json!({"clientInfo":{"name":"wakeit","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":false}})).await?;
        input.write_all(b"{\"method\":\"initialized\"}\n").await?;
        let mut cursor=Value::Null;
        let mut entries=Vec::new();
        for request in 2..=201 {
            let result=rpc(&mut input,&mut reader,request,"thread/list",json!({"cwd":cwd.to_string_lossy(),"limit":100,"cursor":cursor,"archived":false,"sourceKinds":["cli","vscode","exec","appServer"]})).await?;
            for item in result["data"].as_array().context("Codex session catalog data missing")? {
                if item["ephemeral"]==true {continue;}
                let Some(id)=item["id"].as_str() else {continue;};
                if item["cwd"].as_str().and_then(|path|canonical_directory(path).ok()).as_deref()!=Some(cwd) {continue;}
                entries.push(catalog_entry("codex",id,cwd,item["name"].as_str(),timestamp(&item["updatedAt"]))?);
            }
            let next=result["nextCursor"].clone();
            if next.is_null(){return Ok(entries);}
            ensure!(next!=cursor,"Codex session catalog repeated a cursor");
            cursor=next;
        }
        bail!("session catalog exceeded the project limit")
    }.await;
    child.kill().await.ok();
    let _ = child.wait().await;
    result
}
async fn opencode_catalog(cwd: &Path) -> Result<Vec<CatalogEntry>> {
    let mut entries = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..200 {
        let mut args = vec![
            "api".into(),
            "session.list".into(),
            "--param".into(),
            "limit=100".into(),
            "--param".into(),
            format!("directory={}", cwd.to_string_lossy()),
        ];
        if let Some(value) = &cursor {
            args.extend(["--param".into(), format!("cursor={value}")]);
        }
        let mut command = normalized_command("opencode", &args)?;
        command
            .current_dir(cwd)
            .env("PWD", cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().context("start OpenCode session catalog")?;
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .context("OpenCode stdout")?
            .take((METADATA_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(
            bytes.len() <= METADATA_LIMIT,
            "OpenCode session catalog page exceeded limit"
        );
        ensure!(
            child.wait().await?.success(),
            "OpenCode session catalog failed (requires session.list API)"
        );
        let result: Value =
            serde_json::from_slice(&bytes).context("invalid OpenCode session catalog")?;
        for item in result["data"]
            .as_array()
            .context("OpenCode session catalog data missing")?
        {
            let Some(id) = item["id"].as_str() else {
                continue;
            };
            if item["location"]["directory"]
                .as_str()
                .and_then(|p| canonical_directory(p).ok())
                .as_deref()
                != Some(cwd)
            {
                continue;
            }
            entries.push(catalog_entry(
                "opencode",
                id,
                cwd,
                item["title"].as_str(),
                timestamp(&item["time"]["updated"]),
            )?);
        }
        let next = result["cursor"]["next"].as_str().map(str::to_owned);
        if next.is_none() {
            return Ok(entries);
        }
        ensure!(next != cursor, "OpenCode session catalog repeated a cursor");
        cursor = next;
    }
    bail!("session catalog exceeded the project limit")
}
#[derive(Deserialize)]
struct ClaudeIndex {
    entries: Vec<ClaudeIndexEntry>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeIndexEntry {
    session_id: String,
    custom_title: Option<String>,
    summary: Option<String>,
}
async fn claude_titles(root: &Path) -> Result<std::collections::HashMap<String, String>> {
    let file = match tokio::fs::File::open(root.join("sessions-index.json")).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(error).context("read Claude session index"),
    };
    if file.metadata().await?.len() > METADATA_LIMIT as u64 {
        return Ok(Default::default());
    }
    let mut bytes = Vec::new();
    file.take((METADATA_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    let Ok(index) = serde_json::from_slice::<ClaudeIndex>(&bytes) else {
        return Ok(Default::default());
    };
    Ok(index
        .entries
        .into_iter()
        .filter_map(|entry| {
            entry
                .custom_title
                .or(entry.summary)
                .map(|title| (entry.session_id, title))
        })
        .collect())
}
#[derive(Deserialize)]
struct NativeNameMetadata {
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
}
fn title_record(line: &[u8], protocol: &str, title: &mut Option<String>) -> bool {
    let marker = if protocol == "pi" {
        b"\"session_info\"".as_slice()
    } else {
        b"\"custom-title\"".as_slice()
    };
    if !line.windows(marker.len()).any(|part| part == marker) {
        return false;
    }
    if let Ok(record) = serde_json::from_slice::<NativeNameMetadata>(line) {
        if protocol == "pi" && record.kind == "session_info" {
            *title = record.name;
            return true;
        }
        if protocol == "claude-code" && record.kind == "custom-title" {
            *title = record.custom_title;
            return true;
        }
    }
    false
}
/// Extract native name records only, skipping message fields and oversized records.
async fn session_title(
    reader: &mut (impl tokio::io::AsyncBufRead + Unpin),
    protocol: &str,
) -> Result<(bool, Option<String>)> {
    let mut found = false;
    let mut title = None;
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let bytes = reader.fill_buf().await?;
        if bytes.is_empty() {
            if !oversized {
                found = title_record(&line, protocol, &mut title) || found;
            }
            break;
        }
        let end = bytes.iter().position(|byte| *byte == b'\n');
        let length = end.map_or(bytes.len(), |position| position + 1);
        if !oversized {
            if line.len() + length <= METADATA_LIMIT {
                line.extend_from_slice(&bytes[..length]);
            } else {
                oversized = true;
                line.clear();
            }
        }
        reader.consume(length);
        if end.is_some() {
            if !oversized {
                found = title_record(&line, protocol, &mut title) || found;
            }
            line.clear();
            oversized = false;
        }
    }
    Ok((found, title))
}
async fn file_catalog(protocol: &str, cwd: &Path) -> Result<Vec<CatalogEntry>> {
    let root = if protocol == "pi" {
        pi_directory(cwd).await?
    } else {
        claude_directory(cwd)?
    };
    let titles = if protocol == "claude-code" {
        claude_titles(&root).await?
    } else {
        Default::default()
    };
    let mut directory = match tokio::fs::read_dir(&root).await {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error).context("session directory is unavailable"),
    };
    let mut entries = Vec::new();
    let mut scanned = 0;
    while let Some(file) = directory.next_entry().await? {
        scanned += 1;
        ensure!(
            scanned <= 20_000,
            "session catalog exceeded the project limit"
        );
        if !file.file_type().await?.is_file() {
            continue;
        }
        let path = file.path();
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }
        let metadata = file.metadata().await?;
        if metadata.len() == 0 {
            continue;
        }
        let time = metadata.modified().ok().map(DateTime::<Utc>::from);
        if protocol == "pi" {
            let mut reader = BufReader::new(tokio::fs::File::open(&path).await?);
            let header = match metadata_line(&mut reader).await {
                Ok(Some(header)) => header,
                _ => continue,
            };
            if header["type"] != "session" {
                continue;
            }
            if header["cwd"]
                .as_str()
                .and_then(|p| canonical_directory(p).ok())
                .as_deref()
                != Some(cwd)
            {
                continue;
            }
            let Some(id) = header["id"].as_str() else {
                continue;
            };
            if let Ok(entry) = catalog_entry(
                protocol,
                id,
                cwd,
                session_title(&mut reader, "pi").await?.1.as_deref(),
                time,
            ) {
                entries.push(entry);
            }
        } else {
            // Select only native title records; message payloads never become labels.
            let Some(id) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            let mut names = BufReader::new(tokio::fs::File::open(&path).await?);
            let (found, title) = session_title(&mut names, "claude-code").await?;
            let title = if found {
                title.as_deref()
            } else {
                titles.get(id).map(String::as_str)
            };
            if let Ok(entry) = catalog_entry(protocol, id, cwd, title, time) {
                entries.push(entry);
            }
        }
    }
    Ok(entries)
}
#[derive(Serialize, Deserialize)]
struct CatalogCursor {
    scope: String,
    timestamp: i64,
    id: String,
}
/// Project-scoped metadata only, with no model call, transcript export, or full-disk scan.
pub async fn list(
    protocol: &str,
    cwd: &Path,
    cursor: Option<&str>,
    limit: usize,
    query: Option<&str>,
) -> Result<Value> {
    ensure!(
        (1..=10_000).contains(&limit),
        "session page size must be between 1 and 10000"
    );
    ensure!(
        matches!(
            protocol,
            "codex" | "claude-code" | "pi" | "opencode" | "kimi-code"
        ),
        "unsupported session protocol"
    );
    let cwd = canonical_directory(cwd)?;
    ensure!(cwd.is_dir(), "working directory is not a directory");
    let query = query.unwrap_or("").trim().to_lowercase();
    ensure!(query.len() <= 512, "session search is too long");
    use sha2::Digest;
    let scope = hex::encode(sha2::Sha256::digest(serde_json::to_vec(&(
        protocol, &cwd, &query,
    ))?));
    let cursor = cursor
        .map(|value| -> Result<CatalogCursor> {
            ensure!(value.len() <= 1024, "invalid session cursor");
            let value: CatalogCursor = serde_json::from_slice(
                &URL_SAFE_NO_PAD
                    .decode(value)
                    .context("invalid session cursor")?,
            )
            .context("invalid session cursor")?;
            ensure!(
                value.scope == scope,
                "session cursor belongs to a different query"
            );
            Ok(value)
        })
        .transpose()?;
    let mut entries = tokio::time::timeout(Duration::from_secs(20), async {
        match protocol {
            "codex" => codex_catalog(&cwd).await,
            "opencode" => opencode_catalog(&cwd).await,
            "kimi-code" => crate::kimi_adapter::session_summaries(&cwd)
                .await?
                .into_iter()
                .map(|session| {
                    catalog_entry(
                        protocol,
                        &session.id,
                        &cwd,
                        session.title.as_deref(),
                        session.updated_at,
                    )
                })
                .collect(),
            _ => file_catalog(protocol, &cwd).await,
        }
    })
    .await
    .context("session catalog timed out")??;
    entries.retain(|entry| {
        (query.is_empty()
            || entry.id.to_lowercase().contains(&query)
            || entry
                .title
                .as_ref()
                .is_some_and(|v| v.to_lowercase().contains(&query)))
            && cursor.as_ref().is_none_or(|cursor| {
                (entry.timestamp, entry.id.as_str()) < (cursor.timestamp, cursor.id.as_str())
            })
    });
    entries.sort_by(|a, b| (b.timestamp, &b.id).cmp(&(a.timestamp, &a.id)));
    let mut seen = std::collections::HashSet::new();
    entries.retain(|entry| seen.insert(entry.id.clone()));
    let next_cursor = if entries.len() > limit {
        let last = &entries[limit - 1];
        Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&CatalogCursor {
            scope,
            timestamp: last.timestamp,
            id: last.id.clone(),
        })?))
    } else {
        None
    };
    entries.truncate(limit);
    Ok(json!({"version":1,"sessions":entries,"next_cursor":next_cursor}))
}

//! Read-only metadata discovery. The device, not the Gateway OS, validates native paths.
use super::agent_backend::{Agent, AgentOperation, AgentReply};
use crate::{config::TaskConfig, process};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, time::Duration};
use uuid::Uuid;
use wakeit_core::session_catalog::{Catalog, Entry, paginate};

const CATALOG_LIMIT: usize = 10_000;
const SDK_PAGE_LIMIT: usize = 100;
pub async fn query(
    journal: &super::journal::Journal,
    agent: impl Into<Agent>,
    cwd: &str,
    q: Option<&str>,
    cursor: Option<&str>,
) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    let agent = agent.into();
    let cwd = Path::new(cwd).canonicalize()?;
    let task: TaskConfig = serde_json::from_value(
        json!({"version":1,"id":"session-list","name":"Session list","runner":"local","agent":agent.id,"cwd":cwd,"prompt":"","schedule":null,"parameters":crate::config::resolve_parameters(&agent.parameters,&Default::default())?}),
    )?;
    let mut catalog: Catalog = if let Some(config) = agent.legacy() {
        let spec = config
            .session
            .as_ref()
            .and_then(|s| s.list.as_ref())
            .context("adapter has no session list interface")?;
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
        serde_json::from_str(&out.stdout).context("invalid session catalog")?
    } else {
        let mut catalog = Catalog {
            version: 1,
            sessions: Vec::new(),
            next_cursor: None,
        };
        let mut cursor = None;
        let mut cursors = HashSet::new();
        let mut ids = HashSet::new();
        let mut pages = 0;
        loop {
            ensure!(
                pages < SDK_PAGE_LIMIT,
                "session catalog exceeded its page limit"
            );
            pages += 1;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            ensure!(!remaining.is_zero(), "session catalog query timed out");
            let mut request = agent.request(AgentOperation::SessionList, &task, None, remaining);
            // Search stays local: native filtering would hide matches in fallback titles.
            // The Runner's presentation cursor is separate from this native cursor.
            request.input = json!({"limit":CATALOG_LIMIT});
            if let Some(cursor) = &cursor {
                request.input["cursor"] = json!(cursor);
            }
            let response = agent
                .invoke(request, tokio_util::sync::CancellationToken::new(), None)
                .await?;
            ensure!(
                response.confirmed && response.stopped.is_none(),
                "session catalog did not stop cleanly"
            );
            let AgentReply::Sessions(batch) = response.reply else {
                anyhow::bail!("invalid session catalog response")
            };
            ensure!(batch.version == 1, "invalid session catalog version");
            ensure!(
                catalog.sessions.len() + batch.sessions.len() <= CATALOG_LIMIT,
                "session catalog exceeded its item limit"
            );
            validate_sessions(&batch.sessions, &cwd, &mut ids)?;
            catalog.sessions.extend(batch.sessions);
            let Some(next) = batch.next_cursor else {
                break catalog;
            };
            ensure!(
                !next.is_empty() && next.len() <= 1024,
                "invalid session catalog cursor"
            );
            ensure!(
                cursors.insert(next.clone()),
                "repeated session catalog cursor"
            );
            cursor = Some(next);
        }
    };
    ensure!(
        catalog.version == 1
            && catalog.sessions.len() <= CATALOG_LIMIT
            && catalog.next_cursor.is_none(),
        "session catalog exceeded its limit or is incomplete"
    );
    let titles = journal.session_titles(&agent.id, &cwd).await?;
    for entry in &mut catalog.sessions {
        if entry.title.as_ref().is_none_or(|t| t.trim().is_empty()) {
            entry.title = titles.get(&entry.id).cloned();
        }
    }
    page(catalog, &cwd, q, cursor)
}
fn validate_sessions(entries: &[Entry], cwd: &Path, ids: &mut HashSet<String>) -> Result<()> {
    for entry in entries {
        crate::runner::sessions::validate_id(&entry.id)?;
        ensure!(ids.insert(entry.id.clone()), "duplicate session ID");
        ensure!(
            entry.title.as_ref().is_none_or(|s| s.len() <= 4096),
            "session title is too long"
        );
        ensure!(
            Path::new(&entry.cwd).canonicalize()? == cwd,
            "session catalog includes another directory"
        );
    }
    Ok(())
}
fn page(catalog: Catalog, cwd: &Path, q: Option<&str>, cursor: Option<&str>) -> Result<Value> {
    let cwd = cwd.canonicalize()?;
    validate_sessions(&catalog.sessions, &cwd, &mut HashSet::new())?;
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
    use crate::config::AgentConfig;
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
    #[tokio::test]
    async fn local_display_names_are_joined_before_search_and_cursor_pagination() {
        let directory = tempfile::tempdir().unwrap();
        let journal = super::super::journal::Journal::open(directory.path(), Uuid::new_v4())
            .await
            .unwrap();
        let cwd = directory.path().canonicalize().unwrap();
        let sessions=(0..26).map(|i|serde_json::json!({"id":format!("s{i:02}"),"cwd":cwd,"title":if i==0{Some("Native title")}else{None},"updated_at":null})).collect::<Vec<_>>();
        for i in 0..26 {
            sqlx::query(
                "INSERT INTO session_titles(agent,session_id,cwd,title) VALUES('fixture',?,?,?)",
            )
            .bind(format!("s{i:02}"))
            .bind(cwd.to_string_lossy().as_ref())
            .bind(format!("Mapped {i:02}"))
            .execute(&journal.pool)
            .await
            .unwrap();
        }
        let result = serde_json::json!({"version":1,"sessions":sessions,"next_cursor":null});
        let agent:AgentConfig=serde_json::from_value(serde_json::json!({"version":1,"id":"fixture","name":"fixture","mode":"sync","submit":{"command":"unused"},"output":{"format":"events"},"session":{"list":{"command":"node","args":["-e","console.log(process.argv[1])",result.to_string()]},"inspect":{"command":"unused"},"resume":{"command":"unused"},"interactive":{"command":"unused"}}})).unwrap();
        let first = query(
            &journal,
            &agent,
            &cwd.to_string_lossy(),
            Some("Mapped"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(first["sessions"].as_array().unwrap().len(), 20);
        assert!(
            !first["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["id"] == "s00")
        );
        let second = query(
            &journal,
            &agent,
            &cwd.to_string_lossy(),
            Some("Mapped"),
            first["next_cursor"].as_str(),
        )
        .await
        .unwrap();
        assert_eq!(second["sessions"].as_array().unwrap().len(), 5);
        assert!(second["next_cursor"].is_null());
        let original = query(
            &journal,
            &agent,
            &cwd.to_string_lossy(),
            Some("Native"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(original["sessions"][0]["id"], "s00");
    }

    fn sdk_directory() -> tempfile::TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/catalog-pages");
        std::fs::create_dir_all(&root).unwrap();
        tempfile::tempdir_in(root).unwrap()
    }

    fn sdk_peer(root: &Path, pages: Value) -> Agent {
        use super::super::agent_backend::{AgentDescriptor, SdkBackend};
        let root = root.canonicalize().unwrap();
        let sdk = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sdk/typescript/src/index.ts")
            .canonicalize()
            .unwrap();
        let script = root.join("peer.mjs");
        let source = r#"
import { Plugin, Agent } from __SDK__;
import { appendFileSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
const descriptor=JSON.parse(process.argv[2]),root=process.argv[3];
const pages=JSON.parse(readFileSync(join(root,'pages.json'),'utf8'));
const agent=new Agent(descriptor);
for(const operation of descriptor.operations) agent.on(operation,async request=>{
 if(operation!=='session_list') throw new Error('Only synthetic session listing is permitted');
 appendFileSync(join(root,'requests.jsonl'),JSON.stringify(request)+'\n');
 const key=request.input.cursor??'first';
 if(!Object.hasOwn(pages,key)) throw new Error('Unexpected synthetic native cursor');
 // Make renewed per-page deadlines observably different without relying on startup speed.
 if(key==='first') await new Promise(resolve=>setTimeout(resolve,200));
 return {kind:'sessions',value:pages[key]};
});
await new Plugin().agent('synthetic-catalog',agent).serve();
"#;
        std::fs::write(
            &script,
            source.replace(
                "__SDK__",
                &serde_json::to_string(&reqwest::Url::from_file_path(sdk).unwrap().as_str())
                    .unwrap(),
            ),
        )
        .unwrap();
        std::fs::write(root.join("pages.json"), pages.to_string()).unwrap();
        let descriptor: AgentDescriptor = serde_json::from_value(json!({
            "name":"Synthetic paginated catalog","mode":"sync",
            "operations":["detect","execute","session_inspect","session_list"]
        }))
        .unwrap();
        let command = which::which("node").unwrap();
        let args = vec![
            "--experimental-strip-types".into(),
            "--no-warnings".into(),
            wakeit_core::native_command::external_path(&script),
            serde_json::to_string(&descriptor).unwrap(),
            root.to_string_lossy().into_owned(),
        ];
        std::fs::write(
            root.join("wakeit-plugin.json"),
            json!({
                "manifest_version":1,"name":"synthetic-catalog","version":"1.0.0",
                "wakeit":{"protocol":1,"plugins":{"catalog":{"name":"Synthetic catalog",
                    "command":command,"args":args,"agents":{"synthetic-catalog":descriptor}}}}
            })
            .to_string(),
        )
        .unwrap();
        Agent::sdk(SdkBackend {
            instance_id: None,
            runtime_directory: None,
            configuration: None,
            plugin: "catalog".into(),
            id: "synthetic-catalog".into(),
            descriptor,
            source: "synthetic:catalog".into(),
            version: "1.0.0".into(),
            revision: None,
            root,
            command,
            args,
        })
        .unwrap()
    }

    fn sdk_requests(root: &Path) -> Vec<Value> {
        std::fs::read_to_string(root.join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn sdk_pages_preserve_local_search_pagination_titles_and_total_deadline() {
        let directory = sdk_directory();
        let cwd = directory.path().canonicalize().unwrap();
        let journal = super::super::journal::Journal::open(&cwd.join("journal"), Uuid::new_v4())
            .await
            .unwrap();
        let entries: Vec<_> = (0..26)
            .map(|i| {
                json!({"id":format!("s{i:02}"),"cwd":cwd,
                "title":if i==0 {Some("Native title")} else {None},"updated_at":null})
            })
            .collect();
        let agent = sdk_peer(
            &cwd,
            json!({
                "first":{"version":1,"sessions":entries[..13],"next_cursor":"native-second"},
                "native-second":{"version":1,"sessions":entries[13..],"next_cursor":null}
            }),
        );
        for i in 0..26 {
            sqlx::query("INSERT INTO session_titles(agent,session_id,cwd,title) VALUES('synthetic-catalog',?,?,?)")
                .bind(format!("s{i:02}"))
                .bind(cwd.to_string_lossy().as_ref())
                .bind(format!("Mapped {i:02}"))
                .execute(&journal.pool).await.unwrap();
        }
        let first = query(
            &journal,
            &agent,
            &cwd.to_string_lossy(),
            Some("Mapped"),
            None,
        )
        .await
        .unwrap();
        let first_ids: Vec<_> = first["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        let expected: Vec<_> = (6..26).rev().map(|i| format!("s{i:02}")).collect();
        assert_eq!(first_ids, expected);
        assert!(first["next_cursor"].is_string());
        let second = query(
            &journal,
            &agent,
            &cwd.to_string_lossy(),
            Some("Mapped"),
            first["next_cursor"].as_str(),
        )
        .await
        .unwrap();
        let second_ids: Vec<_> = second["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        assert_eq!(second_ids, ["s05", "s04", "s03", "s02", "s01"]);
        assert!(second["next_cursor"].is_null());
        let native = query(
            &journal,
            &agent,
            &cwd.to_string_lossy(),
            Some("Native"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(native["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(native["sessions"][0]["title"], "Native title");
        assert_eq!(native["sessions"][0]["id"], "s00");
        let requests = sdk_requests(&cwd);
        assert_eq!(requests.len(), 6);
        for pair in requests.as_chunks::<2>().0 {
            assert_eq!(pair[0]["input"], json!({"limit":10000}));
            assert_eq!(
                pair[1]["input"],
                json!({"limit":10000,"cursor":"native-second"})
            );
            for request in pair {
                assert_eq!(request["context"]["cwd"], json!(cwd));
                assert!(request["context"]["prompt"].is_null());
            }
            let deadline = |request: &Value| {
                chrono::DateTime::parse_from_rfc3339(
                    request["context"]["deadline"].as_str().unwrap(),
                )
                .unwrap()
            };
            assert!(
                (deadline(&pair[1]) - deadline(&pair[0]))
                    .num_milliseconds()
                    .abs()
                    < 100,
                "next page must retain the original deadline despite delayed first-page output"
            );
        }
    }

    #[tokio::test]
    async fn sdk_pages_reject_cursor_cycles_duplicate_ids_and_cross_directory_entries() {
        for (scenario, expected_error, expected_calls) in [
            ("cycle", "repeated session catalog cursor", 3),
            ("duplicate", "duplicate session ID", 2),
            ("other-directory", "another directory", 2),
            ("empty-cursor", "invalid session catalog cursor", 1),
        ] {
            let directory = sdk_directory();
            let cwd = directory.path().canonicalize().unwrap();
            let other = cwd.join("other");
            std::fs::create_dir(&other).unwrap();
            let journal =
                super::super::journal::Journal::open(&cwd.join("journal"), Uuid::new_v4())
                    .await
                    .unwrap();
            let entry = json!({"id":"session-one","cwd":cwd,"title":null,"updated_at":null});
            let pages = match scenario {
                "cycle" => json!({
                    "first":{"version":1,"sessions":[],"next_cursor":"a"},
                    "a":{"version":1,"sessions":[],"next_cursor":"b"},
                    "b":{"version":1,"sessions":[],"next_cursor":"a"}
                }),
                "duplicate" => json!({
                    "first":{"version":1,"sessions":[entry],"next_cursor":"second"},
                    "second":{"version":1,"sessions":[entry],"next_cursor":null}
                }),
                "other-directory" => json!({
                    "first":{"version":1,"sessions":[],"next_cursor":"second"},
                    "second":{"version":1,"sessions":[{"id":"session-two","cwd":other,"title":null,"updated_at":null}],"next_cursor":null}
                }),
                _ => json!({"first":{"version":1,"sessions":[],"next_cursor":""}}),
            };
            let agent = sdk_peer(&cwd, pages);
            let error = query(&journal, &agent, &cwd.to_string_lossy(), None, None)
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(expected_error),
                "{scenario}: {error:#}"
            );
            assert_eq!(sdk_requests(&cwd).len(), expected_calls, "{scenario}");
        }
    }

    #[tokio::test]
    async fn sdk_catalog_item_limit_applies_across_pages_without_silent_truncation() {
        for total in [10000, 10001] {
            let directory = sdk_directory();
            let cwd = directory.path().canonicalize().unwrap();
            let journal =
                super::super::journal::Journal::open(&cwd.join("journal"), Uuid::new_v4())
                    .await
                    .unwrap();
            let entries: Vec<_> = (0..total)
                .map(|i| json!({"id":format!("session-{i:05}"),"cwd":cwd,"title":null,"updated_at":null}))
                .collect();
            let agent = sdk_peer(
                &cwd,
                json!({
                    "first":{"version":1,"sessions":entries[..5000],"next_cursor":"second"},
                    "second":{"version":1,"sessions":entries[5000..],"next_cursor":null}
                }),
            );
            let result = query(&journal, &agent, &cwd.to_string_lossy(), None, None).await;
            if total == 10000 {
                let result = result.unwrap();
                assert_eq!(result["sessions"].as_array().unwrap().len(), 20);
                assert!(result["next_cursor"].is_string());
            } else {
                assert!(result.unwrap_err().to_string().contains("item limit"));
            }
            assert_eq!(sdk_requests(&cwd).len(), 2);
        }
    }
}

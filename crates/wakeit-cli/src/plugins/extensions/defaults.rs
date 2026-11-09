//! Persistent default selection. A default is an ordinary instance, never a stateless backend.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use uuid::Uuid;
use wakeit_plugin_protocol::extensions::{Extension, validate_schema};

pub async fn initialize(pool: &SqlitePool) -> Result<()> {
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS extension_defaults(source TEXT NOT NULL,extension TEXT NOT NULL,instance TEXT,suppressed INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(source,extension));CREATE TABLE IF NOT EXISTS extension_legacy_agents(backend TEXT PRIMARY KEY,instance TEXT NOT NULL,revision INTEGER NOT NULL);")
        .execute(pool).await?;
    Ok(())
}

fn config_defaults(schema: &Value) -> Option<Value> {
    if let Some(value) = schema.get("default") {
        return Some(value.clone());
    }
    let props = schema.get("properties")?.as_object()?;
    let values = props
        .iter()
        .filter_map(|(key, value)| config_defaults(value).map(|v| (key.clone(), v)))
        .collect::<serde_json::Map<_, _>>();
    (!values.is_empty()).then_some(Value::Object(values))
}

pub async fn ensure(
    pool: &SqlitePool,
    source: &str,
    resource: &str,
    root: &Path,
    version: &str,
    declaration: &Extension,
) -> Result<Option<Uuid>> {
    let mut tx = pool.begin().await?;
    if let Some(row) = sqlx::query(
        "SELECT instance,suppressed FROM extension_defaults WHERE source=? AND extension=?",
    )
    .bind(source)
    .bind(resource)
    .fetch_optional(&mut *tx)
    .await?
    {
        let selected: Option<String> = row.get("instance");
        if row.get::<i64, _>("suppressed") != 0 {
            return Ok(None);
        }
        if let Some(id) = selected {
            let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM extension_instances WHERE id=? AND source=? AND extension=? AND removed=0)")
                .bind(&id).bind(source).bind(resource).fetch_one(&mut *tx).await?;
            if exists {
                return Ok(Some(id.parse()?));
            }
            sqlx::query(
                "UPDATE extension_defaults SET suppressed=1 WHERE source=? AND extension=?",
            )
            .bind(source)
            .bind(resource)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
        }
        return Ok(None);
    }
    let existing = sqlx::query(
        "SELECT id,removed FROM extension_instances WHERE source=? AND extension=? ORDER BY id",
    )
    .bind(source)
    .bind(resource)
    .fetch_all(&mut *tx)
    .await?;
    let (id, suppressed) = if existing.len() == 1 {
        (
            Some(existing[0].get::<String, _>("id")),
            existing[0].get::<i64, _>("removed") != 0,
        )
    } else if !existing.is_empty() {
        // Ambiguity requires an explicit default choice, not an arbitrary first row.
        (None, false)
    } else {
        let id = Uuid::new_v4().to_string();
        let config = config_defaults(&declaration.config).unwrap_or_else(|| json!({}));
        let enabled = declaration.permissions.is_empty()
            && declaration.credentials.is_empty()
            && validate_schema(&declaration.config, &config).is_ok();
        sqlx::query("INSERT INTO extension_instances(id,source,extension,name,root,declaration,enabled,version,config,state) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(&id).bind(source).bind(resource).bind(&declaration.name)
            .bind(root.to_string_lossy().as_ref()).bind(serde_json::to_string(declaration)?)
            .bind(enabled).bind(version).bind(config.to_string())
            .bind(if enabled {"pending"} else {"disabled"}).execute(&mut *tx).await?;
        (Some(id), false)
    };
    sqlx::query(
        "INSERT INTO extension_defaults(source,extension,instance,suppressed) VALUES(?,?,?,?)",
    )
    .bind(source)
    .bind(resource)
    .bind(&id)
    .bind(suppressed)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    if let Some(id) = id.as_deref().filter(|_| !suppressed) {
        let id: Uuid = id.parse()?;
        super::storage::snapshot(pool, id).await?;
        Ok(Some(id))
    } else {
        Ok(None)
    }
}

pub async fn selected(pool: &SqlitePool, source: &str, resource: &str) -> Result<Option<Uuid>> {
    let id:Option<String>=sqlx::query_scalar("SELECT d.instance FROM extension_defaults d JOIN extension_instances i ON i.id=d.instance AND i.source=d.source AND i.extension=d.extension WHERE d.source=? AND d.extension=? AND d.suppressed=0 AND i.removed=0")
        .bind(source).bind(resource).fetch_optional(pool).await?.flatten();
    id.map(|id| id.parse().map_err(Into::into)).transpose()
}

pub async fn choose(pool: &SqlitePool, id: Uuid) -> Result<()> {
    let mut tx = pool.begin().await?;
    let row =
        sqlx::query("SELECT source,extension FROM extension_instances WHERE id=? AND removed=0")
            .bind(id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .context("plugin instance is unavailable")?;
    sqlx::query("INSERT INTO extension_defaults(source,extension,instance,suppressed) VALUES(?,?,?,0) ON CONFLICT(source,extension) DO UPDATE SET instance=excluded.instance,suppressed=0")
        .bind(row.get::<String,_>("source")).bind(row.get::<String,_>("extension"))
        .bind(id.to_string()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn suppress(pool: &SqlitePool, id: Uuid) -> Result<()> {
    sqlx::query("UPDATE extension_defaults SET suppressed=1 WHERE instance=?")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn retain_installed(pool: &SqlitePool, installed: &[(String, String)]) -> Result<()> {
    let defaults =
        sqlx::query("SELECT source,extension,instance FROM extension_defaults WHERE suppressed=0")
            .fetch_all(pool)
            .await?;
    for row in defaults {
        let source: String = row.get("source");
        let resource: String = row.get("extension");
        if installed
            .iter()
            .any(|(s, r)| s == &source && r == &resource)
        {
            continue;
        }
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE extension_defaults SET suppressed=1 WHERE source=? AND extension=?")
            .bind(&source)
            .bind(&resource)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE extension_instances SET enabled=0,state='unavailable',revision=revision+1 WHERE source=? AND extension=? AND enabled=1")
            .bind(&source).bind(&resource).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(())
}

/// Identity-only migration. Original code, empty config and old host-API ceiling remain fixed.
pub async fn migrate_history(pool: &SqlitePool, directory: &Path) -> Result<()> {
    use crate::runner::agent_backend::{Agent, Backend, ConfigurationSnapshot};
    use sha2::{Digest, Sha256};
    let rows=sqlx::query("SELECT id,agent FROM local_runs WHERE json_extract(agent,'$.backend')='sdk_backend' AND json_extract(agent,'$.config.instance_id') IS NULL")
        .fetch_all(pool).await?;
    for row in rows {
        let raw: String = row.get("agent");
        let mut agent: Agent = serde_json::from_str(&raw)?;
        let Backend::SdkBackend(backend) = &mut agent.backend else {
            continue;
        };
        let Some(instance) = selected(pool, &backend.source, &backend.plugin).await? else {
            continue;
        };
        let enabled:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM extension_instances WHERE id=? AND enabled=1 AND removed=0)").bind(instance.to_string()).fetch_one(pool).await?;
        if !enabled {
            continue;
        }
        let manifest = match super::super::manifest::read(&backend.root) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let Some(declaration) = manifest.wakeit.plugins.get(&backend.plugin) else {
            continue;
        };
        let empty = declaration.config == json!({"type":"object","properties":{}})
            || declaration.config == json!({})
            || declaration.config == Value::Bool(true);
        if !empty || !declaration.credentials.is_empty() || !declaration.permissions.is_empty() {
            continue;
        }
        if backend
            .configuration
            .as_ref()
            .is_some_and(|c| c.value != json!({}) || !c.credential_fingerprints.is_empty())
        {
            continue;
        }
        let key = hex::encode(Sha256::digest(raw.as_bytes()));
        let mut tx = pool.begin().await?;
        let previous =
            sqlx::query("SELECT instance,revision FROM extension_legacy_agents WHERE backend=?")
                .bind(&key)
                .fetch_optional(&mut *tx)
                .await?;
        let (instance, revision) = if let Some(previous) = previous {
            (
                previous.get::<String, _>("instance").parse::<Uuid>()?,
                previous.get::<i64, _>("revision"),
            )
        } else {
            let revision:i64=sqlx::query_scalar("SELECT COALESCE(MIN(revision),0)-1 FROM extension_versions WHERE instance=? AND revision<0")
                .bind(instance.to_string()).fetch_one(&mut *tx).await?;
            let snapshot = json!({"root":backend.root,"version":backend.version,"declaration":declaration,"revision":revision,"config":{}});
            sqlx::query("INSERT INTO extension_versions(instance,revision,snapshot) VALUES(?,?,?)")
                .bind(instance.to_string())
                .bind(revision)
                .bind(snapshot.to_string())
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO extension_legacy_agents(backend,instance,revision) VALUES(?,?,?)",
            )
            .bind(&key)
            .bind(instance.to_string())
            .bind(revision)
            .execute(&mut *tx)
            .await?;
            (instance, revision)
        };
        backend.instance_id = Some(instance);
        backend.runtime_directory = Some(directory.to_path_buf());
        backend.legacy_stateless = true;
        backend.configuration = Some(ConfigurationSnapshot {
            revision,
            value: json!({}),
            credential_fingerprints: Default::default(),
        });
        sqlx::query("UPDATE local_runs SET agent=? WHERE id=? AND agent=?")
            .bind(serde_json::to_string(&agent)?)
            .bind(row.get::<String, _>("id"))
            .bind(raw)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
    Ok(())
}

pub async fn can_clear(pool: &SqlitePool, id: Uuid) -> Result<()> {
    let retained:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM local_runs r JOIN extension_instances i ON i.id=? WHERE (json_extract(r.agent,'$.config.instance_id')=i.id OR (json_extract(r.agent,'$.backend')='sdk_backend' AND json_extract(r.agent,'$.config.instance_id') IS NULL AND json_extract(r.agent,'$.config.source')=i.source AND json_extract(r.agent,'$.config.plugin')=i.extension)) AND (r.phase!='done' OR r.session IS NOT NULL))")
        .bind(id.to_string()).fetch_one(pool).await?;
    ensure!(
        !retained,
        "plugin instance is retained by a Run or session; keep its data for recovery"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::journal::Journal;
    async fn setup() -> (tempfile::TempDir, SqlitePool) {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/default-instance-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(base).unwrap();
        let journal = Journal::open(dir.path(), Uuid::new_v4()).await.unwrap();
        super::super::storage::initialize(&journal.pool)
            .await
            .unwrap();
        initialize(&journal.pool).await.unwrap();
        (dir, journal.pool)
    }
    fn declaration() -> Extension {
        serde_json::from_value(json!({"name":"Synthetic plugin","command":"unused","config":{"type":"object","properties":{}}})).unwrap()
    }
    #[tokio::test]
    async fn ordinary_defaults_are_persistent_and_do_not_special_case_official_agents() {
        let (dir, pool) = setup().await;
        let d = declaration();
        let first = ensure(
            &pool,
            "git:example.test/plugin",
            "main",
            dir.path(),
            "1.0.0",
            &d,
        )
        .await
        .unwrap()
        .unwrap();
        let next = ensure(
            &pool,
            "git:example.test/plugin",
            "main",
            dir.path(),
            "1.0.0",
            &d,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(first, next);
        let row = sqlx::query("SELECT enabled,grants,secrets FROM extension_instances WHERE id=?")
            .bind(first.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<i64, _>("enabled"), 1);
        assert_eq!(row.get::<String, _>("grants"), "[]");
        assert_eq!(row.get::<String, _>("secrets"), "{}");
        assert_eq!(
            selected(&pool, "git:example.test/plugin", "main")
                .await
                .unwrap(),
            Some(first)
        );
    }
    #[tokio::test]
    async fn configuration_credentials_or_permissions_never_receive_automatic_authorization() {
        let (dir, pool) = setup().await;
        for (key, patch) in [
            ("permissions", json!(["events:read"])),
            (
                "credentials",
                json!({"account":{"label":"Account","required":true}}),
            ),
            (
                "config",
                json!({"type":"object","properties":{"endpoint":{"type":"string"}},"required":["endpoint"]}),
            ),
        ] {
            let mut d = serde_json::to_value(declaration()).unwrap();
            d[key] = patch;
            let d: Extension = serde_json::from_value(d).unwrap();
            let id = ensure(&pool, "local:synthetic", key, dir.path(), "1.0.0", &d)
                .await
                .unwrap()
                .unwrap();
            let enabled: i64 =
                sqlx::query_scalar("SELECT enabled FROM extension_instances WHERE id=?")
                    .bind(id.to_string())
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(enabled, 0, "{key} needs explicit setup");
        }
    }
    #[tokio::test]
    async fn disabled_or_deleted_defaults_are_never_resurrected_even_after_data_clear() {
        let (dir, pool) = setup().await;
        let d = declaration();
        let id = ensure(&pool, "local:synthetic", "main", dir.path(), "1.0.0", &d)
            .await
            .unwrap()
            .unwrap();
        sqlx::query("UPDATE extension_instances SET enabled=0 WHERE id=?")
            .bind(id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            ensure(&pool, "local:synthetic", "main", dir.path(), "2.0.0", &d)
                .await
                .unwrap(),
            Some(id)
        );
        let enabled: i64 = sqlx::query_scalar("SELECT enabled FROM extension_instances WHERE id=?")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(enabled, 0);
        suppress(&pool, id).await.unwrap();
        sqlx::query("DELETE FROM extension_instances WHERE id=?")
            .bind(id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            ensure(&pool, "local:synthetic", "main", dir.path(), "2.0.0", &d)
                .await
                .unwrap()
                .is_none()
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM extension_instances")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
    #[tokio::test]
    async fn existing_account_is_adopted_without_duplicating_or_modifying_its_settings() {
        let (dir, pool) = setup().await;
        let d = declaration();
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO extension_instances(id,source,extension,name,root,version,declaration,config,secrets,grants,revision,enabled) VALUES(?,'local:synthetic','main','Existing',?,'1.0.0',?,'{\"saved\":true}','{\"account\":\"synthetic-secret\"}','[]',9,0)")
            .bind(id.to_string()).bind(dir.path().to_string_lossy().as_ref()).bind(serde_json::to_string(&d).unwrap()).execute(&pool).await.unwrap();
        assert_eq!(
            ensure(&pool, "local:synthetic", "main", dir.path(), "2.0.0", &d)
                .await
                .unwrap(),
            Some(id)
        );
        let row = sqlx::query(
            "SELECT revision,config,secrets,enabled FROM extension_instances WHERE id=?",
        )
        .bind(id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<i64, _>("revision"), 9);
        assert_eq!(row.get::<i64, _>("enabled"), 0);
        assert_eq!(row.get::<String, _>("config"), "{\"saved\":true}");
        assert_eq!(
            row.get::<String, _>("secrets"),
            "{\"account\":\"synthetic-secret\"}"
        );
    }
    #[tokio::test]
    async fn ambiguous_existing_instances_require_a_persistent_explicit_choice() {
        let (dir, pool) = setup().await;
        let d = declaration();
        let mut ids = vec![];
        for _ in 0..2 {
            let id = Uuid::new_v4();
            ids.push(id);
            sqlx::query("INSERT INTO extension_instances(id,source,extension,name,root,version,declaration) VALUES(?,'local:synthetic','main','Same display name',?,'1.0.0',?)").bind(id.to_string()).bind(dir.path().to_string_lossy().as_ref()).bind(serde_json::to_string(&d).unwrap()).execute(&pool).await.unwrap();
        }
        assert!(
            ensure(&pool, "local:synthetic", "main", dir.path(), "1.0.0", &d)
                .await
                .unwrap()
                .is_none()
        );
        choose(&pool, ids[1]).await.unwrap();
        assert_eq!(
            selected(&pool, "local:synthetic", "main").await.unwrap(),
            Some(ids[1])
        );
        assert_eq!(
            ensure(&pool, "local:synthetic", "main", dir.path(), "1.0.0", &d)
                .await
                .unwrap(),
            Some(ids[1])
        );
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    use crate::{
        protocol::Assignment,
        runner::{
            agent_backend::{Backend, ConfigurationSnapshot},
            journal::Journal,
        },
    };
    #[tokio::test]
    async fn historical_stateless_mapping_keeps_job_deadline_code_and_empty_configuration() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/default-history-tests");
        std::fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        let runner = Uuid::new_v4();
        let journal = Journal::open(root.path(), runner).await.unwrap();
        super::super::storage::initialize(&journal.pool)
            .await
            .unwrap();
        let mut original =
            crate::runner::agent_backend::tests::fixture(root.path(), "async_reconcile_done");
        let Backend::SdkBackend(backend) = &mut original.backend else {
            unreachable!()
        };
        backend.instance_id = None;
        backend.runtime_directory = Some(root.path().to_path_buf());
        backend.configuration = Some(ConfigurationSnapshot {
            revision: 0,
            value: json!({}),
            credential_fingerprints: Default::default(),
        });
        let old_root = backend.root.clone();
        let old_command = backend.command.clone();
        let manifest = super::super::super::manifest::read(root.path()).unwrap();
        let declaration = &manifest.wakeit.plugins["synthetic-entry"];
        let id = ensure(
            &journal.pool,
            "synthetic:test",
            "synthetic-entry",
            root.path(),
            "1.0.0",
            declaration,
        )
        .await
        .unwrap()
        .unwrap();
        let run = Uuid::new_v4();
        let deadline = chrono::Utc::now() + chrono::Duration::minutes(5);
        let task=serde_json::from_value(json!({"version":1,"id":"history","name":"History","runner":"synthetic","agent":"synthetic","cwd":root.path(),"prompt":"synthetic input","parameters":{},"schedule":null})).unwrap();
        journal
            .insert(
                &Assignment {
                    version: 1,
                    run_id: run,
                    runner_id: runner,
                    task,
                    deadline,
                },
                &original,
            )
            .await
            .unwrap();
        sqlx::query("UPDATE local_runs SET phase='unknown',job='original-job' WHERE id=?")
            .bind(run.to_string())
            .execute(&journal.pool)
            .await
            .unwrap();
        migrate_history(&journal.pool, root.path()).await.unwrap();
        let migrated = journal.get(run).await.unwrap().unwrap();
        assert_eq!(migrated.job.as_deref(), Some("original-job"));
        assert_eq!(migrated.assignment.deadline, deadline);
        assert_eq!(migrated.phase, "unknown");
        let Backend::SdkBackend(saved) = &migrated.agent.backend else {
            unreachable!()
        };
        assert_eq!(saved.instance_id, Some(id));
        assert!(saved.legacy_stateless);
        assert_eq!(saved.root, old_root);
        assert_eq!(saved.command, old_command);
        let frozen = saved.configuration.as_ref().unwrap();
        assert!(frozen.revision < 0);
        assert_eq!(frozen.value, json!({}));
        assert!(frozen.credential_fingerprints.is_empty());
        assert!(can_clear(&journal.pool, id).await.is_err());
        let other = Uuid::new_v4();
        sqlx::query("INSERT INTO extension_instances SELECT ?,source,extension,'Other',root,declaration,revision,enabled,removed,version,grants,'{\"other\":true}',secrets,state,applied_revision,error,restart_attempts,next_start FROM extension_instances WHERE id=?")
            .bind(other.to_string()).bind(id.to_string()).execute(&journal.pool).await.unwrap();
        choose(&journal.pool, other).await.unwrap();
        migrate_history(&journal.pool, root.path()).await.unwrap();
        let next = journal.get(run).await.unwrap().unwrap();
        let Backend::SdkBackend(next) = next.agent.backend else {
            unreachable!()
        };
        assert_eq!(next.instance_id, Some(id));
        assert_eq!(next.configuration.unwrap().revision, frozen.revision);
    }
}

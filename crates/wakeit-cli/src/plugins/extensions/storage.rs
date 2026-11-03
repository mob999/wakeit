//! Runner-local instance data. Secrets never appear in catalog or invocation records.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;
use wakeit_core::extensions::{ConfigUpdate, Extension, Instance, SecretUpdate};

pub async fn initialize(pool: &SqlitePool) -> Result<()> {
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS extension_instances(id TEXT PRIMARY KEY,source TEXT NOT NULL,extension TEXT NOT NULL,name TEXT NOT NULL,root TEXT NOT NULL,declaration TEXT NOT NULL,revision INTEGER NOT NULL DEFAULT 1,enabled INTEGER NOT NULL DEFAULT 0,removed INTEGER NOT NULL DEFAULT 0,version TEXT NOT NULL,grants TEXT NOT NULL DEFAULT '[]',config TEXT NOT NULL DEFAULT '{}',secrets TEXT NOT NULL DEFAULT '{}',state TEXT NOT NULL DEFAULT 'disabled',applied_revision INTEGER,error TEXT,restart_attempts INTEGER NOT NULL DEFAULT 0,next_start TEXT);CREATE TABLE IF NOT EXISTS extension_versions(instance TEXT NOT NULL,revision INTEGER NOT NULL,snapshot TEXT NOT NULL,PRIMARY KEY(instance,revision));CREATE TABLE IF NOT EXISTS extension_bindings(id TEXT PRIMARY KEY,position INTEGER NOT NULL,binding TEXT NOT NULL);CREATE TABLE IF NOT EXISTS extension_storage(instance TEXT NOT NULL,key TEXT NOT NULL,value TEXT NOT NULL,revision INTEGER NOT NULL DEFAULT 1,PRIMARY KEY(instance,key));CREATE TABLE IF NOT EXISTS extension_invocations(id TEXT PRIMARY KEY,instance TEXT NOT NULL,kind TEXT NOT NULL,handler TEXT NOT NULL,input TEXT NOT NULL,state TEXT NOT NULL,result TEXT,error TEXT,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,finished_at TEXT);CREATE TABLE IF NOT EXISTS extension_package_versions(source TEXT PRIMARY KEY,version TEXT NOT NULL);CREATE TABLE IF NOT EXISTS extension_operations(id TEXT PRIMARY KEY,instance TEXT NOT NULL,result TEXT NOT NULL);CREATE TABLE IF NOT EXISTS extension_interactions(id TEXT PRIMARY KEY,invocation TEXT NOT NULL,instance TEXT NOT NULL,request TEXT NOT NULL,response TEXT,state TEXT NOT NULL DEFAULT 'waiting');CREATE TABLE IF NOT EXISTS extension_artifacts(id TEXT PRIMARY KEY,instance TEXT NOT NULL,name TEXT NOT NULL,reference TEXT NOT NULL,metadata TEXT NOT NULL,expires_at TEXT);CREATE TABLE IF NOT EXISTS extension_events(id TEXT PRIMARY KEY,event_type TEXT NOT NULL,instance TEXT,data TEXT NOT NULL,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,published INTEGER NOT NULL DEFAULT 0);")
        .execute(pool).await?;
    sqlx::raw_sql(r#"
CREATE TRIGGER IF NOT EXISTS extension_instance_created AFTER INSERT ON extension_instances BEGIN
 INSERT INTO extension_events(id,event_type,data) VALUES(lower(hex(randomblob(16))),'instance.created',json_object('instance_id',NEW.id,'revision',NEW.revision,'state',NEW.state));
END;
CREATE TRIGGER IF NOT EXISTS extension_instance_state AFTER UPDATE OF state ON extension_instances WHEN OLD.state<>NEW.state BEGIN
 INSERT INTO extension_events(id,event_type,data) VALUES(lower(hex(randomblob(16))),CASE NEW.state WHEN 'starting' THEN 'instance.activating' WHEN 'running' THEN CASE WHEN OLD.state='failed' THEN 'instance.recovered' ELSE 'instance.activated' END WHEN 'disabled' THEN 'instance.deactivated' WHEN 'removed' THEN 'instance.deactivated' WHEN 'pending' THEN 'instance.config_changed' ELSE 'instance.failed' END,json_object('instance_id',NEW.id,'revision',NEW.revision,'state',NEW.state));
END;
CREATE TRIGGER IF NOT EXISTS extension_config_changed AFTER UPDATE OF config ON extension_instances WHEN OLD.config<>NEW.config BEGIN
 INSERT INTO extension_events(id,event_type,data) VALUES(lower(hex(randomblob(16))),'instance.config_changed',json_object('instance_id',NEW.id,'revision',NEW.revision));
END;
CREATE TRIGGER IF NOT EXISTS extension_credentials_changed AFTER UPDATE OF secrets ON extension_instances WHEN OLD.secrets<>NEW.secrets BEGIN
 INSERT INTO extension_events(id,event_type,data) VALUES(lower(hex(randomblob(16))),'credential.status_changed',json_object('instance_id',NEW.id,'revision',NEW.revision,'configured_slots',(SELECT json_group_array(key) FROM json_each(NEW.secrets))));
END;
CREATE TRIGGER IF NOT EXISTS extension_action_started AFTER INSERT ON extension_invocations WHEN NEW.kind='action' BEGIN
 INSERT INTO extension_events(id,event_type,data) VALUES(lower(hex(randomblob(16))),'action.started',json_object('instance_id',NEW.instance,'invocation_id',NEW.id,'handler',NEW.handler));
END;
CREATE TRIGGER IF NOT EXISTS extension_action_finished AFTER UPDATE OF state ON extension_invocations WHEN NEW.kind='action' AND OLD.state<>NEW.state AND NEW.state<>'running' BEGIN
 INSERT INTO extension_events(id,event_type,data) VALUES(lower(hex(randomblob(16))),'action.finished',json_object('instance_id',NEW.instance,'invocation_id',NEW.id,'handler',NEW.handler,'state',NEW.state));
END;
"#).execute(pool).await?;
    sqlx::query("UPDATE extension_invocations SET state='uncertain',error='Runner restarted during invocation' WHERE state='running'").execute(pool).await?;
    sqlx::query("UPDATE extension_interactions SET state='cancelled' WHERE state='waiting'")
        .execute(pool)
        .await?;
    Ok(())
}
pub async fn get(
    pool: &SqlitePool,
    id: Uuid,
    runner: Uuid,
    declaration: Extension,
) -> Result<Instance> {
    let row = sqlx::query("SELECT * FROM extension_instances WHERE id=?")
        .bind(id.to_string())
        .fetch_one(pool)
        .await?;
    let secrets: serde_json::Map<String, Value> = serde_json::from_str(row.get("secrets"))?;
    Ok(Instance {
        id,
        runner_id: runner,
        source: row.get("source"),
        extension: row.get("extension"),
        name: row.get("name"),
        revision: row.get("revision"),
        enabled: row.get::<i64, _>("enabled") != 0,
        version: row.get("version"),
        grants: serde_json::from_str(row.get("grants"))?,
        config: serde_json::from_str(row.get("config"))?,
        credential_status: declaration
            .credentials
            .keys()
            .map(|s| (s.clone(), secrets.contains_key(s)))
            .collect(),
        state: row.get("state"),
        applied_revision: row.get("applied_revision"),
        error: row.get("error"),
        declaration,
    })
}
pub async fn save(
    pool: &SqlitePool,
    id: Uuid,
    update: ConfigUpdate,
    declaration: &Extension,
) -> Result<Value> {
    if let Some(result) = sqlx::query_scalar::<_, String>(
        "SELECT result FROM extension_operations WHERE id=? AND instance=?",
    )
    .bind(update.operation_id.to_string())
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?
    {
        return Ok(serde_json::from_str(&result)?);
    }
    snapshot(pool, id).await?;
    wakeit_core::extensions::validate_schema(&declaration.config, &update.config)?;
    let mut tx = pool.begin().await?;
    let row = sqlx::query("SELECT revision,secrets FROM extension_instances WHERE id=?")
        .bind(id.to_string())
        .fetch_one(&mut *tx)
        .await?;
    ensure!(
        row.get::<i64, _>("revision") == update.expected_revision,
        "configuration revision conflict"
    );
    let mut secrets: serde_json::Map<String, Value> = serde_json::from_str(row.get("secrets"))?;
    for (slot, value) in update.secrets {
        ensure!(
            declaration.credentials.contains_key(&slot),
            "undeclared credential slot"
        );
        match value {
            SecretUpdate::Keep => {}
            SecretUpdate::Clear => {
                secrets.remove(&slot);
            }
            SecretUpdate::Replace(value) => {
                ensure!(!value.is_empty(), "empty credential");
                secrets.insert(slot, Value::String(value));
            }
        }
    }
    for (id, slot) in &declaration.credentials {
        ensure!(
            !slot.required || secrets.contains_key(id),
            "required credential is not configured: {id}"
        );
    }
    let revision = update.expected_revision + 1;
    sqlx::query("UPDATE extension_instances SET config=?,secrets=?,revision=?,state=CASE WHEN enabled=1 THEN 'pending' ELSE 'disabled' END,error=NULL WHERE id=?")
        .bind(update.config.to_string()).bind(Value::Object(secrets).to_string()).bind(revision).bind(id.to_string()).execute(&mut *tx).await?;
    let result = json!({"id":update.operation_id,"state":"committed","revision":revision});
    sqlx::query("INSERT INTO extension_operations(id,instance,result) VALUES(?,?,?)")
        .bind(update.operation_id.to_string())
        .bind(id.to_string())
        .bind(result.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    snapshot(pool, id).await?;
    Ok(result)
}
pub async fn storage(
    pool: &SqlitePool,
    instance: Uuid,
    method: &str,
    params: &Value,
) -> Result<Value> {
    let key = params["key"].as_str().unwrap_or("");
    let owner = instance.to_string();
    match method {
        "storage.list" => {
            let prefix = params["prefix"].as_str().unwrap_or("");
            let rows=sqlx::query("SELECT key,value,revision FROM extension_storage WHERE instance=? AND key>? ORDER BY key LIMIT 200").bind(owner).bind(params["cursor"].as_str().unwrap_or("")).fetch_all(pool).await?;
            Ok(
                json!({"items":rows.iter().filter(|r|r.get::<String,_>("key").starts_with(prefix)).map(|r|json!({"key":r.get::<String,_>("key"),"value":serde_json::from_str::<Value>(r.get("value")).unwrap_or(Value::Null),"revision":r.get::<i64,_>("revision")})).collect::<Vec<_>>(),"next_cursor":if rows.len()==200 {rows.last().map(|r|r.get::<String,_>("key"))}else{None}}),
            )
        }
        "storage.get" => {
            let row = sqlx::query(
                "SELECT value,revision FROM extension_storage WHERE instance=? AND key=?",
            )
            .bind(owner)
            .bind(key)
            .fetch_optional(pool)
            .await?;
            Ok(row.map(|r|json!({"value":serde_json::from_str::<Value>(r.get("value")).unwrap_or(Value::Null),"revision":r.get::<i64,_>("revision")})).unwrap_or(Value::Null))
        }
        "storage.set" | "storage.delete" => {
            ensure!(!key.is_empty(), "storage key required");
            let mut tx = pool.begin().await?;
            let revision = sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM extension_storage WHERE instance=? AND key=?",
            )
            .bind(&owner)
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?
            .unwrap_or(0);
            if let Some(expected) = params["expected_revision"].as_i64() {
                ensure!(expected == revision, "storage revision conflict");
            }
            if method == "storage.delete" {
                sqlx::query("DELETE FROM extension_storage WHERE instance=? AND key=?")
                    .bind(owner)
                    .bind(key)
                    .execute(&mut *tx)
                    .await?;
            } else {
                sqlx::query("INSERT INTO extension_storage(instance,key,value,revision) VALUES(?,?,?,?) ON CONFLICT(instance,key) DO UPDATE SET value=excluded.value,revision=excluded.revision").bind(owner).bind(key).bind(params.get("value").context("storage value required")?.to_string()).bind(revision+1).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(json!({"revision":revision+1}))
        }
        _ => anyhow::bail!("unsupported storage operation"),
    }
}

pub async fn snapshot(pool: &SqlitePool, id: Uuid) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO extension_versions(instance,revision,snapshot) SELECT id,revision,json_object('root',root,'declaration',json(declaration),'version',version,'config',json(config),'revision',revision) FROM extension_instances WHERE id=?").bind(id.to_string()).execute(pool).await?;
    Ok(())
}

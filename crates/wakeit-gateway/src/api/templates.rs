use crate::{
    api::{
        self,
        access::user,
        http::{ApiJson, Error, Path},
    },
    auth::workspace::User,
    config::TaskConfig,
    state::App,
    store,
    store::workspaces,
    task_templates::{self as templates, Definition, InstanceRequest, Issue, Preview},
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveTemplate {
    pub definition: Definition,
    pub expected_revision: Option<i64>,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub expected_revision: i64,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DraftPreview {
    pub definition: Definition,
    pub request: InstanceRequest,
}
#[derive(Deserialize, Default)]
pub struct List {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub q: Option<String>,
    #[serde(default)]
    pub archived: bool,
}
type Result<T> = api::http::Result<T>;

fn invalid(issues: Vec<Issue>) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({"error":"taskTemplate.validation","issues":issues})),
    )
        .into_response()
}
async fn authorize(
    tx: &mut Transaction<'_, Postgres>,
    actor: &User,
    workspace: Uuid,
) -> Result<()> {
    workspaces::lock(tx, workspace).await?;
    crate::auth::workspace::check_member(tx, &actor.credential, workspace, "operator")
        .await
        .map(|_| ())
        .map_err(|_| {
            Error(
                StatusCode::FORBIDDEN,
                "taskTemplate.permissionChanged".into(),
            )
        })
}
async fn record(s: &App, workspace: Uuid, id: &str) -> Result<Value> {
    sqlx::query_scalar("SELECT to_jsonb(t.*) FROM task_templates t WHERE workspace_id=$1 AND id=$2")
        .bind(workspace)
        .bind(id)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(Error::missing)
}
pub(crate) async fn list(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<List>,
) -> Result<Json<Value>> {
    let actor = user(&s, &h, "viewer", false).await?;
    let workspace = actor.workspace()?;
    let (page, size) = api::http::pagination(q.page, q.page_size)?;
    let value=sqlx::query_scalar("WITH filtered AS (SELECT * FROM task_templates WHERE workspace_id=$1 AND (archived_at IS NOT NULL)=$2 AND ($3::text IS NULL OR concat_ws(' ',definition->>'name',definition->>'description',id) ILIKE $3)), paged AS (SELECT to_jsonb(t.*) AS item FROM filtered t ORDER BY updated_at DESC,id LIMIT $4 OFFSET $5) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(item) FROM paged),'[]'::jsonb),'total',(SELECT count(*) FROM filtered),'page',$6::bigint,'page_size',$7::bigint)")
        .bind(workspace).bind(q.archived).bind(api::http::search_pattern(q.q)?).bind(size).bind((page-1)*size).bind(page).bind(size).fetch_one(&s.pool).await?;
    Ok(Json(value))
}
pub(crate) async fn get(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let actor = user(&s, &h, "viewer", false).await?;
    Ok(Json(record(&s, actor.workspace()?, &id).await?))
}
pub(crate) async fn from_task(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Definition>> {
    let actor = user(&s, &h, "viewer", false).await?;
    let config: Value =
        sqlx::query_scalar("SELECT config FROM tasks WHERE workspace_id=$1 AND id=$2")
            .bind(actor.workspace()?)
            .bind(id)
            .fetch_optional(&s.pool)
            .await?
            .ok_or_else(Error::missing)?;
    Ok(Json(templates::from_task(&serde_json::from_value(
        config,
    )?)?))
}
pub(crate) async fn save(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<SaveTemplate>,
) -> Result<Response> {
    let actor = user(&s, &h, "operator", true).await?;
    let workspace = actor.workspace()?;
    if body.definition.id != id {
        return Err(Error::bad("taskTemplate.idMismatch"));
    }
    let definition = body.definition.clone();
    let issues = tokio::task::spawn_blocking(move || templates::validate_definition(&definition))
        .await
        .map_err(anyhow::Error::from)?;
    if !issues.is_empty() {
        return Ok(invalid(issues));
    }
    let mut tx = s.pool.begin().await?;
    authorize(&mut tx, &actor, workspace).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{workspace}:template:{id}"))
        .execute(&mut *tx)
        .await?;
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM task_templates WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(workspace)
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?;
    if current != body.expected_revision
        && !(current.is_none() && body.expected_revision == Some(0))
    {
        return Err(Error::conflict("taskTemplate.revisionConflict"));
    }
    let value:Value=sqlx::query_scalar("INSERT INTO task_templates(workspace_id,id,definition,created_by,updated_by) VALUES($1,$2,$3,$4,$4) ON CONFLICT(workspace_id,id) DO UPDATE SET definition=excluded.definition,updated_by=excluded.updated_by,revision=task_templates.revision+1,updated_at=now() RETURNING to_jsonb(task_templates.*)")
        .bind(workspace).bind(&id).bind(serde_json::to_value(body.definition)?).bind(actor.id).fetch_one(&mut *tx).await?;
    store::audit(&mut tx, actor.id, Some(workspace), "template.apply", &id).await?;
    tx.commit().await?;
    Ok(Json(value).into_response())
}
async fn availability(
    s: App,
    h: HeaderMap,
    id: String,
    revision: i64,
    archived: bool,
) -> Result<Json<Value>> {
    let actor = user(&s, &h, "operator", true).await?;
    let workspace = actor.workspace()?;
    let mut tx = s.pool.begin().await?;
    authorize(&mut tx, &actor, workspace).await?;
    let row = sqlx::query(
        "SELECT revision FROM task_templates WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(workspace)
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::missing)?;
    if row.get::<i64, _>("revision") != revision {
        return Err(Error::conflict("taskTemplate.revisionConflict"));
    }
    let value=sqlx::query_scalar("UPDATE task_templates SET archived_at=CASE WHEN $3 THEN now() ELSE NULL END,revision=revision+1,updated_by=$4,updated_at=now() WHERE workspace_id=$1 AND id=$2 RETURNING to_jsonb(task_templates.*)")
        .bind(workspace).bind(&id).bind(archived).bind(actor.id).fetch_one(&mut *tx).await?;
    store::audit(
        &mut tx,
        actor.id,
        Some(workspace),
        if archived {
            "template.archive"
        } else {
            "template.restore"
        },
        &id,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(value))
}
pub(crate) async fn archive(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<Revision>,
) -> Result<Json<Value>> {
    availability(s, h, id, body.expected_revision, true).await
}
pub(crate) async fn restore(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<Revision>,
) -> Result<Json<Value>> {
    availability(s, h, id, body.expected_revision, false).await
}

fn check_revision(record: &Value, request: &InstanceRequest) -> Result<()> {
    if !record["archived_at"].is_null() {
        return Err(Error::conflict("taskTemplate.archived"));
    }
    if record["revision"].as_i64() != Some(request.expected_revision) {
        return Err(Error::conflict("taskTemplate.revisionConflict"));
    }
    Ok(())
}
async fn compile(
    s: &App,
    workspace: Uuid,
    mut definition: Definition,
    mut request: InstanceRequest,
) -> Result<Preview> {
    let settings: Value = sqlx::query_scalar("SELECT data FROM settings WHERE workspace_id=$1")
        .bind(workspace)
        .fetch_one(&s.pool)
        .await?;
    let timezone = settings["timezone"].as_str().unwrap_or("Asia/Shanghai");
    if let Some(schedule) = &mut definition.task.schedule
        && schedule.timezone.is_empty()
    {
        schedule.timezone = timezone.into();
    }
    if let Some(Value::Object(schedule)) = &mut request.schedule
        && schedule
            .get("timezone")
            .is_none_or(|v| v.as_str() == Some(""))
    {
        schedule.insert("timezone".into(), json!(timezone));
    }

    tokio::task::spawn_blocking(move || templates::compile(&definition, &request))
        .await
        .map_err(anyhow::Error::from)
        .map_err(Error::from)
}
pub(crate) async fn preview(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<InstanceRequest>,
) -> Result<Json<Preview>> {
    let actor = user(&s, &h, "viewer", false).await?;
    let workspace = actor.workspace()?;
    let record = record(&s, workspace, &id).await?;
    check_revision(&record, &request)?;
    let mut preview = compile(
        &s,
        workspace,
        serde_json::from_value(record["definition"].clone())?,
        request,
    )
    .await?;
    validate_preview(&s, workspace, &mut preview).await?;
    Ok(Json(preview))
}
pub(crate) async fn draft_preview(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(body): ApiJson<DraftPreview>,
) -> Result<Json<Preview>> {
    let actor = user(&s, &h, "viewer", false).await?;
    let mut preview = compile(&s, actor.workspace()?, body.definition, body.request).await?;
    validate_preview(&s, actor.workspace()?, &mut preview).await?;
    Ok(Json(preview))
}
async fn validate_preview(s: &App, workspace: Uuid, preview: &mut Preview) -> Result<()> {
    if let Some(mut config) = preview.config.take() {
        let mut tx = s.pool.begin().await?;
        preview
            .issues
            .extend(validate_bindings(&mut tx, workspace, &mut config, preview).await?);
        preview.graph = config.dag.clone();
        if preview.issues.is_empty() {
            preview.config = Some(config);
        }
    }
    Ok(())
}
async fn validate_bindings(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    config: &mut TaskConfig,
    preview: &Preview,
) -> Result<Vec<Issue>> {
    let executions = if let Some(dag) = &config.dag {
        dag.nodes
            .iter()
            .map(|n| {
                (
                    Some(n.id.clone()),
                    n.execution(config, n.prompt.display_text()),
                )
            })
            .collect::<Vec<_>>()
    } else {
        vec![(None, config.clone())]
    };
    let mut issues = vec![];
    for (node, execution) in executions {
        let path = node
            .as_ref()
            .and_then(|id| preview.source_map.get(id))
            .map(|source| format!("bindings.{}", source.binding))
            .unwrap_or_else(|| {
                format!(
                    "bindings.{}",
                    preview
                        .used_bindings
                        .first()
                        .map(String::as_str)
                        .unwrap_or("default")
                )
            });
        let runner = match crate::store::runners::runner(tx, workspace, &execution).await {
            Ok(runner) => runner,
            Err(e) if e.is::<crate::store::runners::RunnerBindingError>() => {
                let mut issue = Issue::new("binding", &path, e.to_string());
                issue.node_id = node;
                issues.push(issue);
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        if let Some(id) = &node {
            if let Some(n) = config
                .dag
                .as_mut()
                .and_then(|d| d.nodes.iter_mut().find(|n| n.id == *id))
            {
                n.runner = runner.to_string();
            }
        } else {
            config.runner = runner.to_string();
        }
        let row = sqlx::query(
            "SELECT directory_roots,info FROM runners WHERE workspace_id=$1 AND id=$2 FOR SHARE",
        )
        .bind(workspace)
        .bind(runner)
        .fetch_one(&mut **tx)
        .await?;
        let roots: Value = row.get("directory_roots");
        let info: Value = row.get("info");
        if !directory_in_roots(
            &execution.cwd,
            &roots,
            info["os"].as_str() == Some("windows"),
        ) {
            let mut issue = Issue::new(
                "directoryScope",
                format!("{path}.cwd"),
                "Working directory is outside the Runner's advertised execution roots",
            )
            .param("directory", execution.cwd.clone());
            issue.node_id = node;
            issues.push(issue);
        }
    }
    Ok(issues)
}
/// An advertised-root check without accessing a different device's filesystem.
/// Runner canonicalization remains the final symlink/existence authorization.
pub fn directory_in_roots(path: &str, roots: &Value, windows: bool) -> bool {
    fn normalize(path: &str, windows: bool) -> Option<String> {
        if path.contains('\0') {
            return None;
        }
        let value = if windows {
            path.replace('\\', "/").to_lowercase()
        } else {
            path.to_owned()
        };
        let absolute = value.starts_with('/')
            || (windows
                && value.as_bytes().get(1) == Some(&b':')
                && value.as_bytes().get(2) == Some(&b'/'));
        if !absolute {
            return None;
        }
        let mut parts = vec![];
        for part in value.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop()?;
                }
                other => parts.push(other),
            }
        }
        Some(format!(
            "{}{}",
            if value.starts_with('/') { "/" } else { "" },
            parts.join("/")
        ))
    }
    let Some(path) = normalize(path, windows) else {
        return false;
    };
    roots.as_array().is_some_and(|roots| {
        roots
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|r| normalize(r, windows))
            .any(|root| root == "/" || path == root || path.starts_with(&(root + "/")))
    })
}
fn fingerprint(id: &str, request: &InstanceRequest) -> Result<String> {
    let mut request = request.clone();
    request.request_id = None;
    request.unset.sort();
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
        id, request,
    ))?)))
}
async fn prior(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    actor: Uuid,
    key: &str,
    hash: &str,
) -> Result<Option<Value>> {
    let row=sqlx::query("SELECT request_hash,response FROM template_operations WHERE workspace_id=$1 AND actor_id=$2 AND operation='instantiate' AND request_key=$3")
        .bind(workspace).bind(actor).bind(key).fetch_optional(&mut **tx).await?;
    if let Some(row) = row {
        if row.get::<String, _>("request_hash") != hash {
            return Err(Error::conflict("taskTemplate.idempotencyConflict"));
        }
        return Ok(Some(row.get("response")));
    }
    Ok(None)
}
pub(crate) async fn instantiate(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(mut request): ApiJson<InstanceRequest>,
) -> Result<Response> {
    let actor = user(&s, &h, "operator", true).await?;
    let workspace = actor.workspace()?;
    let key = api::http::request_key(&h)?;
    let hash = fingerprint(&id, &request)?;
    // This path deliberately precedes reading the current template: successful
    // lost responses replay even after the template is edited or archived.
    let mut tx = s.pool.begin().await?;
    authorize(&mut tx, &actor, workspace).await?;
    if let Some(response) = prior(&mut tx, workspace, actor.id, &key, &hash).await? {
        return Ok(Json(response).into_response());
    }
    let record: Value = sqlx::query_scalar(
        "SELECT to_jsonb(t.*) FROM task_templates t WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::missing)?;
    check_revision(&record, &request)?;
    tx.rollback().await?;
    request.task_id = Some(
        request
            .task_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
    );
    let mut preview = compile(
        &s,
        workspace,
        serde_json::from_value(record["definition"].clone())?,
        request.clone(),
    )
    .await?;
    if preview.config.is_none() {
        return Ok(invalid(preview.issues));
    }
    let mut tx = s.pool.begin().await?;
    authorize(&mut tx, &actor, workspace).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{workspace}:template-operation:{}:{key}", actor.id))
        .execute(&mut *tx)
        .await?;
    if let Some(response) = prior(&mut tx, workspace, actor.id, &key, &hash).await? {
        return Ok(Json(response).into_response());
    }
    let current: Value = sqlx::query_scalar(
        "SELECT to_jsonb(t.*) FROM task_templates t WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(workspace)
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::missing)?;
    check_revision(&current, &request)?;
    let mut config = preview.config.take().expect("compiled configuration");
    let issues = validate_bindings(&mut tx, workspace, &mut config, &preview).await?;
    if !issues.is_empty() {
        return Ok(invalid(issues));
    }
    let created = store::tasks::apply_task_tx(
        &mut tx,
        workspace,
        &actor.credential,
        &config,
        None,
        actor.id,
        None,
    )
    .await
    .map_err(|e| Error::bad(e.to_string()))?
    .ok_or_else(|| Error::conflict("taskTemplate.taskIdConflict"))?;
    let mut resolved_bindings = std::collections::BTreeMap::new();
    if let Some(dag) = &config.dag {
        for node in &dag.nodes {
            if let Some(source) = preview.source_map.get(&node.id) {
                resolved_bindings.insert(
                    source.binding.clone(),
                    json!({"runner":node.runner,"agent":node.agent,"parameters":node.parameters}),
                );
            }
        }
    } else if let Some(slot) = preview.used_bindings.first() {
        resolved_bindings.insert(
            slot.clone(),
            json!({"runner":config.runner,"agent":config.agent,"parameters":config.parameters}),
        );
    }
    let origin = json!({"template_id":id,"template_name":record["definition"]["name"],"template_revision":request.expected_revision,"values":request.values,"unset":request.unset,"resolved_inputs":preview.resolved_inputs,"bindings":resolved_bindings,"binding_overrides":request.bindings,"source_map":preview.source_map});
    sqlx::query("UPDATE tasks SET template_origin=$3 WHERE workspace_id=$1 AND id=$2")
        .bind(workspace)
        .bind(&config.id)
        .bind(origin)
        .execute(&mut *tx)
        .await?;
    let run = if request.run_now {
        let run = store::runs::trigger_tx(
            &mut tx,
            workspace,
            &actor.credential,
            &config.id,
            actor.id,
            None,
            &key,
        )
        .await
        .map_err(|e| Error::bad(e.to_string()))?;
        // A past one-time occurrence is consumed by Create & run; future ones
        // remain scheduled, and repeat schedules already use next(after now).
        if config.schedule.as_ref().is_some_and(|s| s.at.is_some()) {
            sqlx::query(
                "UPDATE tasks SET next_at=NULL WHERE workspace_id=$1 AND id=$2 AND next_at<=now()",
            )
            .bind(workspace)
            .bind(&config.id)
            .execute(&mut *tx)
            .await?;
        }
        Some(run)
    } else {
        None
    };
    let response = json!({"task_id":config.id,"task_revision":created["revision"],"run_id":run,"template_revision":request.expected_revision});
    sqlx::query("INSERT INTO template_operations(workspace_id,actor_id,operation,request_key,request_hash,response) VALUES($1,$2,'instantiate',$3,$4,$5)").bind(workspace).bind(actor.id).bind(&key).bind(hash).bind(&response).execute(&mut *tx).await?;
    store::audit(
        &mut tx,
        actor.id,
        Some(workspace),
        "template.instantiate",
        &id,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(response).into_response())
}

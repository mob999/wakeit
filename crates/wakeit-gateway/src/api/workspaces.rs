use crate::api::access::workspace_header;
use crate::auth::workspace::reauthorize;
use crate::{
    api::{
        access::user,
        http::{ApiJson, Error, Result},
    },
    auth,
    state::App,
    store::workspaces,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/workspaces", get(list).post(create))
        .route("/api/v1/workspaces/{workspace}", get(detail).put(rename))
        .route("/api/v1/workspaces/{workspace}/leave", post(leave))
        .route("/api/v1/workspaces/{workspace}/members", get(members))
        .route(
            "/api/v1/workspaces/{workspace}/members/{id}",
            put(update).delete(remove),
        )
        .route("/api/v1/invites/inspect", post(inspect_invite))
        .route("/api/v1/invites/accept", post(accept_invite))
        .route("/api/v1/register", post(register))
}
fn scoped(mut h: HeaderMap, workspace: Uuid) -> HeaderMap {
    h.insert("x-wakeit-workspace", workspace.to_string().parse().unwrap());
    h
}
async fn list(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", false).await?;
    if u.kind != "session" {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "a browser session is required".into(),
        ));
    }
    let items:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',w.id,'name',w.name,'role',m.role,'is_demo',w.is_demo,'is_default',w.is_default) FROM workspaces w JOIN workspace_members m ON m.workspace_id=w.id WHERE m.user_id=$1 AND m.active AND NOT w.is_demo ORDER BY w.created_at,w.id").bind(u.id).fetch_all(&s.pool).await?;
    let demo:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'role','viewer','is_demo',true) FROM workspaces WHERE is_demo AND EXISTS(SELECT 1 FROM user_identities WHERE user_id=$1 AND provider='github')").bind(u.id).fetch_optional(&s.pool).await?;
    Ok(Json(json!({"items":items,"demo":demo})))
}
#[derive(Deserialize)]
struct Name {
    name: String,
}
async fn create(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Name>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", true).await?;
    let identity = crate::auth::workspace::identity(&s.pool, &u.credential)
        .await?
        .ok_or_else(|| Error(StatusCode::UNAUTHORIZED, "session expired".into()))?;
    Ok(Json(
        workspaces::create(&s.pool, &identity, &b.name)
            .await
            .map_err(|e| Error::bad(e.to_string()))?,
    ))
}
async fn detail(
    State(s): State<App>,
    h: HeaderMap,
    Path(workspace): Path<Uuid>,
) -> Result<Json<Value>> {
    let u = user(&s, &scoped(h, workspace), "viewer", false).await?;
    let mut result:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'is_demo',is_demo,'is_default',is_default) FROM workspaces WHERE id=$1").bind(workspace).fetch_one(&s.pool).await?;
    result["role"] = json!(u.role);
    Ok(Json(result))
}
async fn rename(
    State(s): State<App>,
    h: HeaderMap,
    Path(workspace): Path<Uuid>,
    ApiJson(b): ApiJson<Name>,
) -> Result<Json<Value>> {
    let u = user(&s, &scoped(h, workspace), "admin", true).await?;
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(Error::bad("invalid workspace name"));
    }
    let mut tx = s.pool.begin().await?;
    workspaces::lock(&mut tx, workspace).await?;
    crate::auth::workspace::reauthorize(&mut tx, &u, "admin").await?;
    sqlx::query("UPDATE workspaces SET name=$2 WHERE id=$1")
        .bind(workspace)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit(workspace_id,user_id,action,target) VALUES($1,$2,'workspace.rename',$1::text)").bind(workspace).bind(u.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn leave(
    State(s): State<App>,
    h: HeaderMap,
    Path(workspace): Path<Uuid>,
) -> Result<Json<Value>> {
    let u = user(&s, &scoped(h, workspace), "viewer", true).await?;
    workspaces::update_member(&s.pool, &u.credential, workspace, u.id, None, None, true)
        .await
        .map_err(|e| Error::conflict(e.to_string()))?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Default, Deserialize)]
struct Page {
    page: Option<i64>,
    page_size: Option<i64>,
}
async fn members(
    State(s): State<App>,
    h: HeaderMap,
    Path(workspace): Path<Uuid>,
    Query(q): Query<Page>,
) -> Result<Json<Value>> {
    user(&s, &scoped(h, workspace), "admin", false).await?;
    let (page, size) = (q.page.unwrap_or(1), q.page_size.unwrap_or(20));
    if !(1..=1_000_000).contains(&page) || !(1..=100).contains(&size) {
        return Err(Error::bad("invalid page or page_size"));
    }
    let result=sqlx::query_scalar("WITH members AS (SELECT u.id,u.username,m.role,m.active,u.active AS account_active,m.created_at FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=$1),paged AS (SELECT * FROM members ORDER BY created_at,id LIMIT $2 OFFSET $3) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(to_jsonb(p) ORDER BY created_at,id) FROM paged p),'[]'::jsonb),'total',(SELECT count(*) FROM members),'active_admins',(SELECT count(*) FROM members WHERE role='admin' AND active AND account_active),'page',$4::bigint,'page_size',$5::bigint)").bind(workspace).bind(size).bind((page-1)*size).bind(page).bind(size).fetch_one(&s.pool).await?;
    Ok(Json(result))
}
#[derive(Deserialize)]
struct Member {
    role: Option<String>,
    active: Option<bool>,
}
async fn update(
    State(s): State<App>,
    h: HeaderMap,
    Path((workspace, id)): Path<(Uuid, Uuid)>,
    ApiJson(b): ApiJson<Member>,
) -> Result<Json<Value>> {
    let u = user(&s, &scoped(h, workspace), "admin", true).await?;
    if b.role
        .as_deref()
        .is_some_and(|role| auth::credentials::role(role) == 0)
    {
        return Err(Error::bad("invalid role"));
    }
    workspaces::update_member(
        &s.pool,
        &u.credential,
        workspace,
        id,
        b.role.as_deref(),
        b.active,
        false,
    )
    .await
    .map_err(|e| Error::conflict(e.to_string()))?;
    Ok(Json(json!({"ok":true})))
}
async fn remove(
    State(s): State<App>,
    h: HeaderMap,
    Path((workspace, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    let u = user(&s, &scoped(h, workspace), "admin", true).await?;
    workspaces::update_member(&s.pool, &u.credential, workspace, id, None, None, true)
        .await
        .map_err(|e| Error::conflict(e.to_string()))?;
    Ok(Json(json!({"ok":true})))
}
async fn inspect_invite(State(s): State<App>, ApiJson(b): ApiJson<Accept>) -> Result<Json<Value>> {
    let result:Value=sqlx::query_scalar("SELECT jsonb_build_object('workspace_id',w.id,'workspace',w.name,'role',i.role) FROM invites i JOIN workspaces w ON w.id=i.workspace_id JOIN workspace_members m ON m.workspace_id=i.workspace_id AND m.user_id=i.created_by JOIN users u ON u.id=m.user_id WHERE i.hash=$1 AND i.used_at IS NULL AND i.expires_at>now() AND m.active AND u.active AND m.role='admin' AND NOT w.is_demo").bind(auth::credentials::hash(&b.token)).fetch_optional(&s.pool).await?.ok_or_else(||Error::bad("invitation invalid or expired"))?;
    Ok(Json(result))
}
async fn accept_invite(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Accept>,
) -> Result<Json<Value>> {
    let existing =
        if crate::api::access::cookie(&s.config, &h).is_some() || h.contains_key("authorization") {
            Some(user(&s, &h, "viewer", true).await?)
        } else {
            None
        };
    if existing.as_ref().is_some_and(|u| u.kind != "session") {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "a browser session is required".into(),
        ));
    }
    if existing.is_none() && s.config.auth.mode != "local" {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "sign in to accept the invitation".into(),
        ));
    }
    if h.get("x-wakeit-request").and_then(|v| v.to_str().ok()) != Some("1") {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "missing request header".into(),
        ));
    }
    let new_account = if existing.is_none() {
        let name = b.username.as_deref().unwrap_or("").trim();
        if name.is_empty() || name.len() > 100 {
            return Err(Error::bad("invalid username"));
        }
        let password = b.password.ok_or_else(|| Error::bad("password required"))?;
        let hash = tokio::task::spawn_blocking(move || auth::credentials::password(&password))
            .await?
            .map_err(|e| Error::bad(e.to_string()))?;
        Some((name.to_string(), hash))
    } else {
        None
    };
    let mut tx = s.pool.begin().await?;
    let invitation=sqlx::query("SELECT workspace_id,role,created_by FROM invites WHERE hash=$1 AND used_at IS NULL AND expires_at>now() FOR UPDATE").bind(auth::credentials::hash(&b.token)).fetch_optional(&mut *tx).await?.ok_or_else(||Error::bad("invitation invalid or expired"))?;
    let workspace: Uuid = invitation.get("workspace_id");
    workspaces::lock(&mut tx, workspace).await?;
    let inviter: Option<Uuid> = invitation.get("created_by");
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members m JOIN users u ON u.id=m.user_id JOIN workspaces w ON w.id=m.workspace_id WHERE m.workspace_id=$1 AND m.user_id=$2 AND m.role='admin' AND m.active AND u.active AND NOT w.is_demo)").bind(workspace).bind(inviter).fetch_one(&mut *tx).await?;
    if !valid {
        return Err(Error::bad("invitation invalid or expired"));
    }
    let id = if let Some(u) = existing {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM credentials c JOIN users u ON u.id=c.user_id WHERE c.hash=$1 AND c.kind='session' AND u.active AND (c.expires_at IS NULL OR c.expires_at>now()))").bind(u.credential).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(Error(StatusCode::UNAUTHORIZED, "session expired".into()));
        }
        u.id
    } else {
        let (name, hash) = new_account.unwrap();
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,$3,'viewer') ON CONFLICT(username) DO NOTHING RETURNING id").bind(id).bind(name).bind(hash).fetch_optional(&mut *tx).await?.ok_or_else(||Error::conflict("username unavailable"))?;
        id
    };
    let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members WHERE workspace_id=$1 AND user_id=$2 AND NOT active)").bind(workspace).bind(id).fetch_one(&mut *tx).await?;
    if blocked {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "membership is disabled".into(),
        ));
    }
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(workspace).bind(id).bind(invitation.get::<String,_>("role")).execute(&mut *tx).await?;
    sqlx::query("UPDATE invites SET used_at=now() WHERE hash=$1")
        .bind(auth::credentials::hash(&b.token))
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit(workspace_id,user_id,action,target) VALUES($1,$2,'member.join',$2::text)").bind(workspace).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"workspace_id":workspace})))
}
#[derive(Deserialize)]
struct Registration {
    username: String,
    password: String,
}
async fn register(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Registration>,
) -> Result<Json<Value>> {
    if s.config.auth.mode != "local" || !s.config.auth.allow_registration {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "registration is disabled".into(),
        ));
    }
    if h.get("x-wakeit-request").and_then(|v| v.to_str().ok()) != Some("1") {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "missing request header".into(),
        ));
    }
    let name = b.username.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(Error::bad("invalid username"));
    }
    let hash = tokio::task::spawn_blocking(move || auth::credentials::password(&b.password))
        .await?
        .map_err(|e| Error::bad(e.to_string()))?;
    sqlx::query("INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,$3,'viewer') ON CONFLICT(username) DO NOTHING RETURNING id").bind(Uuid::new_v4()).bind(name).bind(hash).fetch_optional(&s.pool).await?.ok_or_else(||Error::conflict("username unavailable"))?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RoleBody {
    role: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct Accept {
    pub token: String,
    pub username: Option<String>,
    pub password: Option<String>,
}
pub(crate) async fn invite(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<RoleBody>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let actor = user(&s, &h, "admin", true).await?;
    if auth::credentials::role(&b.role) == 0 {
        return Err(Error::bad("invalid role"));
    }
    let mut tx = s.pool.begin().await?;
    crate::store::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &actor, "admin").await?;
    let token = auth::credentials::secret();
    sqlx::query("INSERT INTO invites(hash,role,expires_at,workspace_id,created_by) VALUES($1,$2,now()+interval '2 days',$3,$4)")
        .bind(auth::credentials::hash(&token))
        .bind(b.role)
        .bind(workspace).bind(actor.id).execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"token":token,"url":format!("{}/invite#{}",s.config.public_url.trim_end_matches('/'),token)}),
    ))
}

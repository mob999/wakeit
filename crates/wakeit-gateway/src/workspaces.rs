//! Identity and workspace authorization are distinct. A workspace key never chooses its scope.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub id: Uuid,
    pub username: String,
    pub instance_admin: bool,
    pub kind: String,
    pub workspace_id: Option<Uuid>,
    #[serde(skip)]
    pub credential: String,
}
#[derive(Clone, Debug)]
pub struct WorkspaceContext {
    pub workspace_id: Uuid,
    pub role: String,
    pub demo: bool,
}
pub async fn identity(pool: &PgPool, credential: &str) -> Result<Option<Identity>> {
    let row = sqlx::query("SELECT u.id,u.username,u.instance_admin,c.kind,c.workspace_id FROM credentials c JOIN users u ON u.id=c.user_id WHERE c.hash=$1 AND u.active AND (c.expires_at IS NULL OR c.expires_at>now())")
        .bind(credential).fetch_optional(pool).await?;
    Ok(row.map(|r| Identity {
        id: r.get("id"),
        username: r.get("username"),
        instance_admin: r.get("instance_admin"),
        kind: r.get("kind"),
        workspace_id: r.get("workspace_id"),
        credential: credential.into(),
    }))
}
pub async fn authorize(
    pool: &PgPool,
    identity: &Identity,
    workspace: Uuid,
) -> Result<Option<WorkspaceContext>> {
    if identity.kind == "api" && identity.workspace_id != Some(workspace) {
        return Ok(None);
    }
    let row=sqlx::query("SELECT w.is_demo,m.role,m.active,u.active AS user_active,EXISTS(SELECT 1 FROM user_identities i WHERE i.user_id=u.id AND i.provider='github') AS github FROM workspaces w JOIN users u ON u.id=$2 LEFT JOIN workspace_members m ON m.workspace_id=w.id AND m.user_id=u.id WHERE w.id=$1")
        .bind(workspace).bind(identity.id).fetch_optional(pool).await?;
    Ok(row.and_then(|r| {
        if !r.get::<bool, _>("user_active") {
            return None;
        }
        let demo = r.get::<bool, _>("is_demo");
        if demo && identity.kind == "session" && r.get::<bool, _>("github") {
            return Some(WorkspaceContext {
                workspace_id: workspace,
                role: "viewer".into(),
                demo: true,
            });
        }
        if demo {
            return None;
        }
        if r.get::<Option<bool>, _>("active") != Some(true) {
            return None;
        }
        Some(WorkspaceContext {
            workspace_id: workspace,
            role: r.get("role"),
            demo,
        })
    }))
}
pub async fn lock(tx: &mut Transaction<'_, Postgres>, workspace: Uuid) -> Result<()> {
    // Serialize workspace mutations without blocking FK key-share checks from
    // execution writers that already hold a task or Run row. IDs never change.
    sqlx::query("SELECT id FROM workspaces WHERE id=$1 FOR NO KEY UPDATE")
        .bind(workspace)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workspace not found"))?;
    Ok(())
}
pub async fn check_member(
    tx: &mut Transaction<'_, Postgres>,
    credential: &str,
    workspace: Uuid,
    needed: &str,
) -> Result<Uuid> {
    let row=sqlx::query("SELECT u.id,m.role,w.is_demo FROM credentials c JOIN users u ON u.id=c.user_id JOIN workspace_members m ON m.user_id=u.id AND m.workspace_id=$2 JOIN workspaces w ON w.id=m.workspace_id WHERE c.hash=$1 AND u.active AND m.active AND (c.expires_at IS NULL OR c.expires_at>now()) AND (c.kind='session' OR c.workspace_id=$2)")
        .bind(credential).bind(workspace).fetch_optional(&mut **tx).await?.ok_or_else(||anyhow::anyhow!("workspace access denied"))?;
    ensure!(
        !row.get::<bool, _>("is_demo"),
        "demo workspace is read only"
    );
    ensure!(
        crate::auth::role(&row.get::<String, _>("role")) >= crate::auth::role(needed),
        "insufficient permissions"
    );
    Ok(row.get("id"))
}
pub async fn create(pool: &PgPool, identity: &Identity, name: &str) -> Result<Value> {
    ensure!(identity.kind == "session", "a browser session is required");
    let name = name.trim();
    ensure!(
        !name.is_empty() && name.chars().count() <= 100,
        "invalid workspace name"
    );
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM credentials c JOIN users u ON u.id=c.user_id WHERE c.hash=$1 AND c.kind='session' AND u.active AND (c.expires_at IS NULL OR c.expires_at>now()))").bind(&identity.credential).fetch_one(&mut *tx).await?;
    ensure!(active, "session expired");
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,name) VALUES($1,$2)")
        .bind(id)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'admin')")
        .bind(id)
        .bind(identity.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO settings(workspace_id,data) VALUES($1,'{\"timezone\":\"Asia/Shanghai\",\"log_retention_days\":7,\"run_retention_days\":90}')").bind(id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO audit(workspace_id,user_id,action,target) VALUES($1,$2,'workspace.create',$1::text)").bind(id).bind(identity.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(json!({"id":id,"name":name,"role":"admin","is_demo":false}))
}
pub async fn update_member(
    pool: &PgPool,
    credential: &str,
    workspace: Uuid,
    target: Uuid,
    role: Option<&str>,
    active: Option<bool>,
    remove: bool,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    lock(&mut tx, workspace).await?;
    let actor = check_member(
        &mut tx,
        credential,
        workspace,
        if remove { "viewer" } else { "admin" },
    )
    .await?;
    if remove && actor != target {
        check_member(&mut tx, credential, workspace, "admin").await?;
    }
    let row = sqlx::query(
        "SELECT role,active FROM workspace_members WHERE workspace_id=$1 AND user_id=$2 FOR UPDATE",
    )
    .bind(workspace)
    .bind(target)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| anyhow::anyhow!("member not found"))?;
    let original: String = row.get("role");
    let role = role.unwrap_or(&original);
    let active = active.unwrap_or(row.get("active"));
    ensure!(crate::auth::role(role) > 0, "invalid role");
    if original == "admin" && row.get::<bool, _>("active") && (remove || !active || role != "admin")
    {
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=$1 AND m.role='admin' AND m.active AND u.active AND m.user_id<>$2").bind(workspace).bind(target).fetch_one(&mut *tx).await?;
        if count == 0 {
            bail!("cannot remove the last administrator");
        }
    }
    if remove {
        sqlx::query("DELETE FROM workspace_members WHERE workspace_id=$1 AND user_id=$2")
            .bind(workspace)
            .bind(target)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query(
            "UPDATE workspace_members SET role=$3,active=$4 WHERE workspace_id=$1 AND user_id=$2",
        )
        .bind(workspace)
        .bind(target)
        .bind(role)
        .bind(active)
        .execute(&mut *tx)
        .await?;
    }
    if remove || !active {
        sqlx::query("UPDATE enrollments SET expires_at=LEAST(expires_at,now()) WHERE workspace_id=$1 AND created_by=$2").bind(workspace).bind(target).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM credentials WHERE workspace_id=$1 AND user_id=$2")
            .bind(workspace)
            .bind(target)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE device_grants SET state='denied' WHERE workspace_id=$1 AND user_id=$2 AND state IN ('pending','approved')").bind(workspace).bind(target).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO audit(workspace_id,user_id,action,target) VALUES($1,$2,$3,$4)")
        .bind(workspace)
        .bind(actor)
        .bind(if remove {
            "member.remove"
        } else {
            "member.update"
        })
        .bind(target.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

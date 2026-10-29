//! Identity and workspace authorization are distinct. A workspace key never chooses its scope.
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::auth::workspace::{Identity, check_member};
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
    ensure!(crate::auth::credentials::role(role) > 0, "invalid role");
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

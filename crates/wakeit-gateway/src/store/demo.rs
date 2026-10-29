//! Optional synthetic, non-executable workspace for the official preview.
use anyhow::Result;
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
pub async fn seed(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743280)")
        .execute(&mut *tx)
        .await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspaces WHERE is_demo)")
        .fetch_one(&mut *tx)
        .await?;
    if exists {
        return Ok(());
    }
    let workspace = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let runner = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,password_hash,role,active) VALUES($1,$2,NULL,'viewer',false)").bind(owner).bind(format!("demo-{}",owner.simple())).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workspaces(id,name,is_demo) VALUES($1,'Wakeit Demo',true)")
        .bind(workspace)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO settings(workspace_id,data) VALUES($1,'{\"timezone\":\"UTC\",\"log_retention_days\":7,\"run_retention_days\":90}')").bind(workspace).execute(&mut *tx).await?;
    let agents = json!([{"id":"codex","name":"Codex","logo":"/agent-logos/codex.svg","mode":"sync","digest":"synthetic","available":true,"version":"Demo","cancellable":true,"parameters":{},"models_supported":false,"trace_supported":true,"session_supported":false}]);
    sqlx::query("INSERT INTO runners(id,workspace_id,name,token_hash,public_key,revoked,agents,info) VALUES($1,$2,'demo-runner',$3,$4,true,$5,$6)").bind(runner).bind(workspace).bind(crate::auth::credentials::hash(&crate::auth::credentials::secret())).bind(nkeys::KeyPair::new_user().public_key()).bind(agents).bind(json!({"os":"linux","version":"Demo","concurrency":1})).execute(&mut *tx).await?;
    let examples = [
        (
            "daily-digest",
            "Daily digest",
            "Summarize the supplied sample project updates.",
            "# Daily digest\n\n- The sample release is ready for review.\n- Documentation and examples are up to date.\n\n| Area | Status |\n| --- | --- |\n| Build | Passed |\n| Documentation | Ready |",
        ),
        (
            "repository-review",
            "Repository review",
            "Review the supplied sample changes.",
            "# Repository review\n\nThe sample changes are ready.\n\n```diff\n- retry_count = 1\n+ retry_count = 3\n```\n\nNo blocking issues were found.",
        ),
        (
            "weekly-planning",
            "Weekly planning",
            "Prepare a plan using the supplied sample goals.",
            "# Weekly plan\n\n1. Review the release checklist.\n2. Update the example integration.\n3. Share the results with the team.",
        ),
    ];
    for (i, (id, name, prompt, result)) in examples.into_iter().enumerate() {
        let config: crate::config::TaskConfig = serde_json::from_value(
            json!({"version":1,"id":id,"name":name,"runner":"demo-runner","agent":"codex","cwd":"/demo/project","prompt":prompt,"enabled":false,"schedule":null}),
        )?;
        let config = serde_json::to_value(config)?;
        sqlx::query("INSERT INTO tasks(workspace_id,id,runner_id,config,enabled,created_by,updated_by) VALUES($1,$2,$3,$4,false,$5,$5)").bind(workspace).bind(id).bind(runner).bind(&config).bind(owner).execute(&mut *tx).await?;
        let run = Uuid::new_v4();
        let end = Utc::now() - Duration::hours((i * 18 + 1) as i64);
        let start = end - Duration::seconds(12);
        sqlx::query("INSERT INTO runs(id,workspace_id,task_id,task_revision,runner_id,config,state,source,scheduled_at,deadline,created_at,started_at,finished_at,result,execution_snapshot) VALUES($1,$2,$3,1,$4,$5,'succeeded','schedule',$6,$7,$6,$6,$7,$8,$9)").bind(run).bind(workspace).bind(id).bind(runner).bind(&config).bind(start).bind(end).bind(json!(result)).bind(json!({"trace_supported":true,"cwd":"/demo/project","parameters":{},"agent_digest":"synthetic","model":"Demo","session_supported":false})).execute(&mut *tx).await?;
        let trace = json!({"id":"report","kind":"message","status":"succeeded","title":"Summary","input":null,"output":result,"occurred_at":end,"started_at":start,"finished_at":end});
        sqlx::query("INSERT INTO events(workspace_id,run_id,seq,kind,data,created_at) VALUES($1,$2,1,'trace',$3,$4),($1,$2,2,'log',$5,$4)").bind(workspace).bind(run).bind(trace).bind(end).bind(json!({"text":"Sample report completed.\n","stream":"stdout"})).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

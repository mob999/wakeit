use anyhow::Result;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// All panels share one database snapshot; historical counts include parent Runs only.
pub async fn overview(pool: &PgPool, workspace: Uuid, timezone: &str, days: i64) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"
WITH calendar AS (
    SELECT ((now() AT TIME ZONE $2)::date - days_ago)::date AS day
    FROM generate_series(0, $3::integer - 1) AS series(days_ago)
), parents AS (
    SELECT id, task_id, config, state, reason, created_at, finished_at
    FROM runs WHERE workspace_id=$1 AND parent_run_id IS NULL
), completed AS (
    SELECT (finished_at AT TIME ZONE $2)::date AS day,
        count(*) FILTER (WHERE state='succeeded') AS succeeded,
        count(*) FILTER (WHERE state IN ('failed','timed_out','interrupted')) AS failed,
        count(*) FILTER (WHERE state='warning') AS warning
    FROM parents
    WHERE finished_at >= ((SELECT min(day) FROM calendar)::timestamp AT TIME ZONE $2)
      AND finished_at <= now()
    GROUP BY 1
), latest AS (
    SELECT DISTINCT ON (task_id) * FROM parents ORDER BY task_id,created_at DESC,id DESC
), offline AS (
    SELECT r.id,r.name,
        (SELECT count(*) FROM tasks t WHERE t.workspace_id=$1 AND t.enabled AND t.archived_at IS NULL
         AND (t.runner_id=r.id OR EXISTS (
             SELECT 1 FROM jsonb_array_elements(COALESCE(t.config->'dag'->'nodes','[]'::jsonb)) n
             WHERE n->>'runner' IN (r.id::text,r.name)
         ))) AS task_count,
        (SELECT count(DISTINCT COALESCE(x.parent_run_id,x.id)) FROM runs x
         WHERE x.workspace_id=$1 AND x.runner_id=r.id AND x.state IN ('queued','starting','running','unknown')) AS run_count
    FROM runners r WHERE r.workspace_id=$1
      AND (r.revoked OR r.last_seen IS NULL OR r.last_seen <= now()-interval '60 seconds')
), attention AS (
    SELECT 0 AS priority, created_at AS at, id::text AS id,
        jsonb_build_object('kind','unknown','id',id,'name',COALESCE(config->>'name',task_id),'state',state,'reason',reason) AS item
    FROM parents WHERE state='unknown'
    UNION ALL
    SELECT 1,l.created_at,l.id::text,
        jsonb_build_object('kind','run','id',l.id,'name',COALESCE(l.config->>'name',l.task_id),'state',l.state,'reason',l.reason)
    FROM latest l JOIN tasks t ON t.workspace_id=$1 AND t.id=l.task_id
    WHERE t.archived_at IS NULL AND l.state IN ('failed','timed_out','interrupted','warning')
    UNION ALL
    SELECT 2,NULL,id::text,
        jsonb_build_object('kind','runner','id',id,'name',name,'task_count',task_count,'run_count',run_count)
    FROM offline WHERE task_count > 0 OR run_count > 0
), attention_page AS (
    SELECT * FROM attention ORDER BY priority,at DESC NULLS LAST,id LIMIT 5
), upcoming AS (
    SELECT id,config->>'name' AS name,next_at FROM tasks
    WHERE workspace_id=$1 AND enabled AND archived_at IS NULL AND next_at IS NOT NULL
    ORDER BY next_at,id LIMIT 5
)
SELECT jsonb_build_object(
    'timezone',$2::text,'as_of',now(),
    'daily',(SELECT jsonb_agg(jsonb_build_object('date',c.day,
        'succeeded',COALESCE(d.succeeded,0),'failed',COALESCE(d.failed,0),'warning',COALESCE(d.warning,0)) ORDER BY c.day)
        FROM calendar c LEFT JOIN completed d USING(day)),
    'current',(SELECT jsonb_build_object('running',count(*) FILTER (WHERE state IN ('starting','running')),
        'queued',count(*) FILTER (WHERE state='queued')) FROM parents),
    'attention',COALESCE((SELECT jsonb_agg(item ORDER BY priority,at DESC NULLS LAST,id) FROM attention_page),'[]'::jsonb),
    'attention_total',(SELECT count(*) FROM attention),
    'upcoming',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',id,'name',name,'next_at',next_at) ORDER BY next_at,id) FROM upcoming),'[]'::jsonb)
)
        "#,
    )
    .bind(workspace)
    .bind(timezone)
    .bind(days as i32)
    .fetch_one(pool)
    .await?)
}

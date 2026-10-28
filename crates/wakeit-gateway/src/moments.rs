//! Read-only completion timeline. Day boundaries use the workspace's IANA timezone.
use crate::api::{App, Error, user};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

type Result<T> = std::result::Result<T, Error>;
const ELIGIBLE: &str = "parent_run_id IS NULL AND finished_at IS NOT NULL AND state IN ('succeeded','warning','failed','cancelled','timed_out','skipped','interrupted')";
#[derive(Deserialize)]
pub struct DaysQuery {
    before: Option<String>,
    limit: Option<i64>,
}
#[derive(Deserialize)]
pub struct EntriesQuery {
    date: String,
    cursor: Option<String>,
    limit: Option<i64>,
    timezone: Option<String>,
}
fn date(value: &str) -> Result<NaiveDate> {
    let parsed =
        NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| Error::bad("invalid date"))?;
    if parsed.to_string() != value {
        return Err(Error::bad("date must be YYYY-MM-DD"));
    }
    Ok(parsed)
}
fn limit(value: Option<i64>, default: i64, maximum: i64) -> Result<i64> {
    let value = value.unwrap_or(default);
    if !(1..=maximum).contains(&value) {
        return Err(Error::bad(format!("limit must be 1–{maximum}")));
    }
    Ok(value)
}
fn cursor(value: &str) -> Result<(DateTime<Utc>, Uuid)> {
    let (at, id) = value
        .split_once('|')
        .ok_or_else(|| Error::bad("invalid cursor"))?;
    Ok((
        at.parse().map_err(|_| Error::bad("invalid cursor time"))?,
        id.parse().map_err(|_| Error::bad("invalid cursor ID"))?,
    ))
}
async fn timezone(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace: Uuid,
) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT data->>'timezone' FROM settings WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub async fn days(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<DaysQuery>,
) -> Result<Json<Value>> {
    let user = user(&s, &h, "viewer", false).await?;
    let workspace = user.workspace()?;
    let before = q.before.as_deref().map(date).transpose()?;
    let limit = limit(q.limit, 14, 100)?;
    let mut tx = s.pool.begin().await?;
    let zone = timezone(&mut tx, workspace).await?;
    // Count and directory are one database snapshot, including dates outside this page.
    let mut response: Value = sqlx::query_scalar(&format!(
        "WITH eligible AS (SELECT (finished_at AT TIME ZONE $1)::date AS day FROM runs WHERE workspace_id=$4 AND {ELIGIBLE}), days AS (SELECT day,count(*) AS count FROM eligible WHERE ($2::date IS NULL OR day<$2) GROUP BY day ORDER BY day DESC LIMIT $3) SELECT jsonb_build_object('total_runs',(SELECT count(*) FROM eligible),'days',COALESCE((SELECT jsonb_agg(jsonb_build_object('date',day,'count',count) ORDER BY day DESC) FROM days),'[]'::jsonb))"
    )).bind(&zone).bind(before).bind(limit+1).bind(workspace).fetch_one(&mut *tx).await?;
    let days = response["days"].as_array_mut().expect("SQL array");
    let next = if days.len() > limit as usize {
        days.truncate(limit as usize);
        days.last().map(|d| d["date"].clone())
    } else {
        None
    };
    response["next_before"] = next.unwrap_or(Value::Null);
    response["timezone"] = json!(zone);
    response["today"] = json!(
        Utc::now()
            .with_timezone(&zone.parse::<chrono_tz::Tz>().map_err(anyhow::Error::from)?)
            .date_naive()
            .to_string()
    );
    tx.commit().await?;
    Ok(Json(response))
}
pub async fn entries(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<EntriesQuery>,
) -> Result<Json<Value>> {
    let user = user(&s, &h, "viewer", false).await?;
    let workspace = user.workspace()?;
    let day = date(&q.date)?;
    let limit = limit(q.limit, 20, 100)?;
    let after = q.cursor.as_deref().map(cursor).transpose()?;
    let mut tx = s.pool.begin().await?;
    let zone = timezone(&mut tx, workspace).await?;
    if q.timezone.as_ref().is_some_and(|z| z != &zone) {
        return Err(Error::conflict("工作区时区已变更，请刷新动态"));
    }
    let rows = sqlx::query(&format!(
        "SELECT r.id,r.finished_at,r.result AS dag_result,r.config->'dag' AS dag_definition,jsonb_build_object('id',r.id,'task_id',r.task_id,'name',r.config->>'name','continuation_available',((COALESCE(r.session->>'ready'='true',false) AND COALESCE(length(r.session->>'command')>0,false)) OR EXISTS(SELECT 1 FROM runs child WHERE child.parent_run_id=r.id AND COALESCE(child.session->>'ready'='true',false) AND COALESCE(length(child.session->>'command')>0,false))),'state',r.state,'reason',r.reason,'finished_at',r.finished_at,'runner',d.name,'agent',COALESCE(a.data->>'name',r.config->>'agent'),'logo',a.data->'logo','preview',left(p.body,2048),'format',CASE WHEN jsonb_typeof(r.result)='string' OR (jsonb_typeof(r.result)='object' AND r.result->'truncated'='true'::jsonb AND jsonb_typeof(r.result->'preview')='string') THEN 'markdown' ELSE 'json' END,'has_more',char_length(p.body)>2048,'truncated',COALESCE((jsonb_typeof(r.result)='object' AND r.result->'truncated'='true'::jsonb AND jsonb_typeof(r.result->'preview')='string') AND jsonb_typeof(r.result)='object',false)) AS item FROM runs r LEFT JOIN runners d ON d.id=r.runner_id LEFT JOIN LATERAL (SELECT data FROM jsonb_array_elements(d.agents) data WHERE data->>'id'=r.config->>'agent' LIMIT 1) a ON true CROSS JOIN LATERAL (SELECT CASE WHEN r.result IS NULL OR r.result='null'::jsonb THEN NULL WHEN jsonb_typeof(r.result)='string' THEN r.result#>>'{{}}' WHEN (jsonb_typeof(r.result)='object' AND r.result->'truncated'='true'::jsonb AND jsonb_typeof(r.result->'preview')='string') AND jsonb_typeof(r.result->'preview')='string' THEN r.result->>'preview' ELSE jsonb_pretty(r.result) END AS body) p WHERE r.workspace_id=$6 AND {ELIGIBLE} AND (r.finished_at AT TIME ZONE $1)::date=$2 AND ($3::timestamptz IS NULL OR (r.finished_at,r.id)<($3,$4::uuid)) ORDER BY r.finished_at DESC,r.id DESC LIMIT $5"
    )).bind(&zone).bind(day).bind(after.map(|v|v.0)).bind(after.map(|v|v.1)).bind(limit+1).bind(workspace).fetch_all(&mut *tx).await?;
    let has_more = rows.len() > limit as usize;
    let rows = rows.into_iter().take(limit as usize).collect::<Vec<_>>();
    let next = if has_more {
        rows.last().map(|r| {
            format!(
                "{}|{}",
                r.get::<DateTime<Utc>, _>("finished_at").to_rfc3339(),
                r.get::<Uuid, _>("id")
            )
        })
    } else {
        None
    };
    let items = rows
        .iter()
        .map(|r| {
            let mut item = r.get::<Value, _>("item");
            if let Some(definition) = r.get::<Option<Value>, _>("dag_definition") {
                let result = r.get::<Option<Value>, _>("dag_result");
                item["node_count"] = json!(definition["nodes"].as_array().map_or(0, Vec::len));
                item["format"] = json!("dag");
                item["preview"] = Value::Null;
                let outputs = report_previews(result.as_ref());
                item["has_more"] = json!(outputs.iter().any(|o| o["has_more"] == true));
                item["truncated"] = json!(outputs.iter().any(|o| o["truncated"] == true));
                item["outputs"] = json!(outputs);
            }
            item
        })
        .collect::<Vec<_>>();
    tx.commit().await?;
    Ok(Json(
        json!({"timezone":zone,"date":q.date,"items":items,"next_cursor":next}),
    ))
}

// One shared Unicode budget, independent of the number of selected reports.
fn report_previews(result: Option<&Value>) -> Vec<Value> {
    let Some(outputs) = result
        .and_then(|r| r.get("outputs"))
        .and_then(Value::as_array)
    else {
        return vec![];
    };
    let mut remaining = 2048;
    outputs.iter().map(|o| {
        let value = &o["result"];
        let truncated = value["truncated"] == true && value["preview"].is_string();
        let body = if value.is_null() { None } else if truncated { value["preview"].as_str().map(str::to_owned) } else if let Some(text) = value.as_str() { Some(text.to_owned()) } else { Some(serde_json::to_string_pretty(value).expect("JSON value")) };
        let count = body.as_ref().map_or(0, |b| b.chars().count());
        let preview = body.map(|b| b.chars().take(remaining).collect::<String>());
        let more = count > remaining;
        remaining = remaining.saturating_sub(count);
        json!({"node_id":o["node_id"],"name":o["name"],"state":o["state"],"reason":o["reason"],"preview":preview,"format":if value.is_string() || truncated {"markdown"} else {"json"},"has_more":more,"truncated":truncated})
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dag_previews_share_a_unicode_budget_and_preserve_report_boundaries() {
        let result = json!({"outputs":[{"node_id":"a","name":"A","state":"succeeded","result":"中".repeat(2040)},{"node_id":"b","name":"B","state":"succeeded","result":"文".repeat(40)},{"node_id":"c","name":"C","state":"failed","reason":"failed","result":null}]});
        let reports = report_previews(Some(&result));
        assert_eq!(
            reports[0]["preview"].as_str().unwrap().chars().count(),
            2040
        );
        assert_eq!(reports[1]["preview"].as_str().unwrap().chars().count(), 8);
        assert_eq!(reports[1]["has_more"], true);
        assert_eq!(reports[2]["reason"], "failed");
        assert!(reports[2]["preview"].is_null());
        assert!(report_previews(None).is_empty());
    }
    #[test]
    fn validates_completion_cursors_and_limits() {
        assert!(date("2026-02-30").is_err());
        assert!(date("2026-1-01").is_err());
        assert!(date("2026-10-15").is_ok());
        assert!(cursor("bad").is_err());
        assert!(cursor("2026-10-15T01:00:00.123456Z|00000000-0000-0000-0000-000000000001").is_ok());
        assert!(limit(Some(0), 14, 100).is_err());
        assert!(limit(Some(101), 14, 100).is_err());
        assert!(matches!(limit(None, 14, 100), Ok(14)));
    }
}

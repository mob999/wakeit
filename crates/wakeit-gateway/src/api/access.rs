use crate::{auth, config::GatewayConfig};
use axum::http::{HeaderMap, StatusCode, header};
use uuid::Uuid;

use crate::api::http::{Error, Result};
use crate::auth::workspace::User;
use crate::state::App;
pub(crate) fn cookie(config: &GatewayConfig, h: &HeaderMap) -> Option<String> {
    h.get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|s| {
            s.trim()
                .strip_prefix(&format!("{}=", config.auth.session_cookie))
                .map(str::to_owned)
        })
}

pub(crate) async fn user(s: &App, h: &HeaderMap, needed: &str, write: bool) -> Result<User> {
    let bearer = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    if bearer.is_none() && write {
        if h.get("x-wakeit-request").and_then(|v| v.to_str().ok()) != Some("1") {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "missing request header".into(),
            ));
        }
        if let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
            let allowed = reqwest::Url::parse(&s.config.public_url)?
                .origin()
                .ascii_serialization();
            if origin != allowed {
                return Err(Error(StatusCode::FORBIDDEN, "origin rejected".into()));
            }
        }
    }
    let token = bearer
        .or_else(|| cookie(&s.config, h))
        .ok_or_else(|| Error(StatusCode::UNAUTHORIZED, "authentication required".into()))?;
    let hash = auth::credentials::hash(&token);
    let identity = crate::auth::workspace::identity(&s.pool, &hash)
        .await?
        .ok_or_else(|| {
            Error(
                StatusCode::UNAUTHORIZED,
                "session expired or token invalid".into(),
            )
        })?;
    let workspace = h
        .get("x-wakeit-workspace")
        .and_then(|v| v.to_str().ok())
        .map(str::parse::<Uuid>)
        .transpose()
        .map_err(|_| Error::bad("invalid workspace"))?;
    let mut role = "viewer".to_string();
    if let Some(workspace) = workspace {
        let context = crate::auth::workspace::authorize(&s.pool, &identity, workspace)
            .await?
            .ok_or_else(Error::missing)?;
        role = context.role;
        if context.demo && (write || h.get("x-wakeit-device-query").is_some()) {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "demo workspace is read only".into(),
            ));
        }
        if auth::credentials::role(&role) < auth::credentials::role(needed) {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "insufficient permissions".into(),
            ));
        }
    } else if needed != "viewer" {
        if !identity.instance_admin || identity.kind != "session" {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "instance administrator required".into(),
            ));
        }
        role = "admin".into();
    }
    Ok(User {
        id: identity.id,
        role,
        username: identity.username,
        credential: hash,
        workspace_id: workspace.or(identity.workspace_id),
        instance_admin: identity.instance_admin,
        kind: identity.kind,
    })
}

pub(crate) async fn device(s: &App, h: &HeaderMap) -> Result<Uuid> {
    let token = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| Error(StatusCode::UNAUTHORIZED, "device token required".into()))?;
    sqlx::query_scalar("SELECT id FROM runners WHERE token_hash=$1 AND NOT revoked")
        .bind(auth::credentials::hash(token))
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(|| {
            Error(
                StatusCode::UNAUTHORIZED,
                "device revoked or token invalid".into(),
            )
        })
}

pub(crate) fn workspace_header(h: &HeaderMap) -> Result<Uuid> {
    h.get("x-wakeit-workspace")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| Error::bad("workspace is required"))
}

#[cfg(test)]
mod workspace_cookie_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn permission_denial_is_distinct_from_database_failure() {
        let denied = Error::from(crate::auth::workspace::PermissionDenied(
            "workspace access denied",
        ));
        assert_eq!(denied.0, StatusCode::FORBIDDEN);
        assert_eq!(denied.1, "workspace access denied");

        let unavailable = Error::from(sqlx::Error::PoolClosed);
        assert_eq!(unavailable.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(unavailable.1, "internal server error");
    }

    #[test]
    fn configured_session_cookie_isolated_from_other_services() {
        let mut config: GatewayConfig = serde_json::from_value(json!({
            "version": 1,
            "public_url": "http://127.0.0.1:58620",
            "database_url_env": "UNUSED_TEST_DATABASE",
            "nats_url": "nats://127.0.0.1:1",
            "nats_public_url": "ws://127.0.0.1:2",
            "nats_auth_dir": "unused"
        }))
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "wakeit_session=original; wakeit_workspace_preview=preview"
                .parse()
                .unwrap(),
        );
        assert_eq!(cookie(&config, &headers).as_deref(), Some("original"));
        config.auth.session_cookie = "wakeit_workspace_preview".into();
        assert_eq!(cookie(&config, &headers).as_deref(), Some("preview"));
        headers.insert(
            header::COOKIE,
            "wakeit_session=original; another_wakeit_workspace_preview=wrong"
                .parse()
                .unwrap(),
        );
        assert!(cookie(&config, &headers).is_none());
    }
}

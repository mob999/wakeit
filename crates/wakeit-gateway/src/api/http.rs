use axum::{
    Json,
    extract::Path as AxumPath,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

pub struct Error(pub StatusCode, pub String);

impl Error {
    pub(crate) fn bad(s: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, s.into())
    }
    pub(crate) fn conflict(s: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, s.into())
    }
    pub(crate) fn missing() -> Self {
        Self(StatusCode::NOT_FOUND, "not found".into())
    }
}

impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        let e = e.into();
        if let Some(denied) = e.downcast_ref::<crate::auth::workspace::PermissionDenied>() {
            return Self(StatusCode::FORBIDDEN, denied.to_string());
        }
        if e.is::<crate::auth::workspace::MissingWorkspace>() {
            return Self::bad(e.to_string());
        }
        if let Some(error) = e.downcast_ref::<crate::store::Error>() {
            return match error {
                crate::store::Error::Missing => Self::missing(),
                crate::store::Error::Conflict(message) => Self::conflict(message.clone()),
            };
        }
        if let Some(error) = e.downcast_ref::<crate::transport::query::QueryError>() {
            let status = match error {
                crate::transport::query::QueryError::Unavailable(_) => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
                crate::transport::query::QueryError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
                crate::transport::query::QueryError::Conflict(_) => StatusCode::CONFLICT,
            };
            return Self(status, error.to_string());
        }
        tracing::error!(error=%e,"request failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal server error".into(),
        )
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

pub(crate) struct ApiJson<T>(pub T);

impl<S, T> axum::extract::FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = Error;
    async fn from_request(
        request: axum::extract::Request,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(request, state)
            .await
            .map_err(|e| Error(e.status(), e.body_text()))?;
        Ok(Self(value))
    }
}

// Nested routes contain workspace plus the resource parameters. Keep resource extraction
// independent of the workspace, whose canonical value is set by server middleware.
pub(crate) struct Path<T>(pub(crate) T);

impl<S: Send + Sync, T: serde::de::DeserializeOwned> axum::extract::FromRequestParts<S>
    for Path<T>
{
    type Rejection = Error;
    async fn from_request_parts(parts: &mut axum::http::request::Parts, state: &S) -> Result<Self> {
        let AxumPath(mut params) =
            AxumPath::<std::collections::HashMap<String, String>>::from_request_parts(parts, state)
                .await
                .map_err(|e| Error::bad(e.to_string()))?;
        params.remove("workspace");
        let value = if params.len() == 1 {
            json!(params.into_values().next().unwrap())
        } else {
            json!([
                params.remove("id"),
                params.remove("agent").or_else(|| params.remove("node"))
            ])
        };
        Ok(Self(
            serde_json::from_value(value).map_err(|e| Error::bad(e.to_string()))?,
        ))
    }
}

#[derive(Default, Deserialize)]
pub(crate) struct PageQuery {
    pub(crate) page: Option<i64>,
    pub(crate) page_size: Option<i64>,
}

pub(crate) fn pagination(page: Option<i64>, size: Option<i64>) -> Result<(i64, i64)> {
    let page = page.unwrap_or(1);
    let size = size.unwrap_or(20);
    if !(1..=1_000_000).contains(&page) || !(1..=100).contains(&size) {
        return Err(Error::bad("invalid page or page_size"));
    }
    Ok((page, size))
}

pub(crate) fn search_pattern(q: Option<String>) -> Result<Option<String>> {
    let Some(q) = q else { return Ok(None) };
    if q.len() > 640 {
        return Err(Error::bad("search query too long"));
    }
    let escaped = q
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    Ok(Some(format!(
        "%{}%",
        escaped.split_whitespace().collect::<Vec<_>>().join("%")
    )))
}

pub(crate) fn request_key(h: &HeaderMap) -> Result<String> {
    let k = h
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::bad("Idempotency-Key required"))?;
    if k.len() > 160 || k.is_empty() {
        return Err(Error::bad("invalid Idempotency-Key"));
    }
    Ok(k.into())
}

pub(crate) fn true_value() -> bool {
    true
}

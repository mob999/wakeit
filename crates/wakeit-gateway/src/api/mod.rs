pub(crate) mod access;
pub(crate) mod accounts;
pub(crate) mod assets;
pub(crate) mod changelog;
pub(crate) mod device;
pub(crate) mod device_grants;
pub(crate) mod events;
pub(crate) mod files;
pub(crate) mod github;
pub(crate) mod http;
pub(crate) mod moments;
pub(crate) mod nodes;
pub mod openapi;
pub(crate) mod runners;
pub(crate) mod runs;
pub(crate) mod sessions;
pub(crate) mod settings;
pub(crate) mod system;
pub(crate) mod tasks;
pub mod templates;
pub(crate) mod terminals;
pub(crate) mod tokens;
pub(crate) mod workspaces;
use axum::{
    Json, Router,
    extract::Path as AxumPath,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde_json::json;
use uuid::Uuid;

use self::http::Error;
use crate::state::App;
async fn workspace_scope(
    AxumPath(params): AxumPath<std::collections::HashMap<String, String>>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(workspace) = params.get("workspace").and_then(|v| v.parse::<Uuid>().ok()) else {
        return Error::bad("invalid workspace").into_response();
    };
    request
        .headers_mut()
        .insert("x-wakeit-workspace", workspace.to_string().parse().unwrap());
    next.run(request).await
}
async fn clear_scope(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    request.headers_mut().remove("x-wakeit-workspace");
    request.headers_mut().remove("x-wakeit-device-query");
    next.run(request).await
}
async fn legacy_business() -> Response {
    (StatusCode::UPGRADE_REQUIRED,Json(json!({"error":"workspace API required; update Wakeit CLI","code":"workspace_required"}))).into_response()
}
pub fn router(app: App) -> Router {
    let business = Router::new()
        .route("/runs/{id}/terminals", post(crate::api::terminals::create))
        .route("/terminals/{id}", get(crate::api::terminals::get))
        .route(
            "/terminals/{id}/attach",
            post(crate::api::terminals::attach),
        )
        .route("/terminals/{id}/stream", get(crate::api::terminals::stream))
        .route("/terminals/{id}/close", post(crate::api::terminals::close))
        .route("/runners/{id}/directories", get(runners::directories))
        .route(
            "/runners/{id}/agents/{agent}/models/query",
            post(runners::query_models),
        )
        .route(
            "/runners/{id}/agents/{agent}/sessions/inspect",
            post(runners::inspect_session),
        )
        .route(
            "/runners/{id}/agents/{agent}/sessions/query",
            post(runners::query_sessions),
        )
        .route("/invites", post(workspaces::invite))
        .route("/tokens", get(tokens::tokens).post(tokens::create_token))
        .route("/tokens/{id}", delete(tokens::delete_token))
        .route("/tasks", get(tasks::tasks))
        .route("/tasks/validate", post(tasks::validate_task))
        .route(
            "/tasks/{id}/template",
            get(crate::api::templates::from_task),
        )
        .route("/task-templates", get(crate::api::templates::list))
        .route(
            "/task-template-preview",
            post(crate::api::templates::draft_preview),
        )
        .route(
            "/task-templates/{id}",
            get(crate::api::templates::get).put(crate::api::templates::save),
        )
        .route(
            "/task-templates/{id}/archive",
            post(crate::api::templates::archive),
        )
        .route(
            "/task-templates/{id}/restore",
            post(crate::api::templates::restore),
        )
        .route(
            "/task-templates/{id}/preview",
            post(crate::api::templates::preview),
        )
        .route(
            "/task-templates/{id}/instantiate",
            post(crate::api::templates::instantiate),
        )
        .route("/tasks/{id}", get(tasks::task).put(tasks::apply))
        .route("/tasks/{id}/runs", post(tasks::trigger))
        .route("/tasks/{id}/archive", post(tasks::archive_task))
        .route("/tasks/{id}/restore", post(tasks::restore_task))
        .route("/schedule/preview", post(settings::preview))
        .route("/moments/days", get(crate::api::moments::days))
        .route("/moments", get(crate::api::moments::entries))
        .route("/runs", get(runs::runs))
        .route("/runs/{id}", get(runs::run))
        .route("/runs/{id}/files/preview", post(crate::api::files::preview))
        .route("/runs/{id}/nodes/{node}", get(nodes::run_node))
        .route("/runs/{id}/nodes/{node}/events", get(nodes::node_events))
        .route("/runs/{id}/nodes/{node}/resolve", post(nodes::resolve_node))
        .route("/runs/{id}/nodes/{node}/retry", post(nodes::retry_node))
        .route("/runs/{id}/cancel", post(runs::cancel))
        .route("/runs/{id}/retry", post(runs::retry))
        .route("/runs/{id}/resolve", post(runs::resolve))
        .route("/runs/{id}/events", get(events::events))
        .route("/runs/{id}/stream", get(events::stream))
        .route("/runners", get(runners::runners))
        .route("/runners/enrollment", post(runners::enrollment))
        .route("/runners/{id}/revoke", post(runners::revoke))
        .route("/agents", get(runners::agents))
        .route("/agents/templates/{id}", get(runners::template))
        .route(
            "/settings",
            get(settings::settings).put(settings::update_settings),
        )
        .route("/timezones", get(settings::timezones))
        .route("/audit", get(settings::audits))
        .route_layer(axum::middleware::from_fn(workspace_scope));
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/health/ready", get(system::ready))
        .route("/metrics", get(system::metrics))
        .merge(crate::api::device_grants::routes())
        .merge(crate::api::workspaces::routes())
        .merge(crate::api::changelog::routes())
        .merge(crate::api::github::routes())
        .route("/api/v1/meta", get(system::meta))
        .route("/api/v1/session", post(sessions::login).get(sessions::me).delete(sessions::logout))
        .route("/api/v1/instance/users", get(accounts::users).post(accounts::create_user))
        .route("/api/v1/instance/users/{id}", put(accounts::update_user))
        .route("/api/v1/instance/users/{id}/password", post(accounts::reset_password))
        .route("/api/v1/device/enroll", post(device::enroll))
        .route("/api/v1/device/hello", post(device::hello))
        .route("/api/v1/device/heartbeat", post(device::heartbeat))
        .route("/api/v1/device/claim", post(device::claim))
        .route("/api/v1/schema/{kind}", get(system::schema))
        .route("/api/v1/openapi.json", get(system::openapi))
        .nest("/api/v1/workspaces/{workspace}", business)
        .route("/api/v1/{*legacy}", axum::routing::any(legacy_business))
        .fallback(assets::static_file)
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<axum::body::Body>| tracing::info_span!("http.request", method=%request.method(), path=request.uri().path())))
        .layer(axum::middleware::from_fn(clear_scope))
        .with_state(app)
}

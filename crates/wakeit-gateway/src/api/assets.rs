use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::api::http::Error;

#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist/"]
pub(crate) struct Assets;

pub(crate) async fn static_file(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path.starts_with("health/") {
        return Error::missing().into_response();
    }
    let file = Assets::get(if path.is_empty() { "index.html" } else { path })
        .or_else(|| Assets::get("index.html"));
    match file {
        Some(file) => {
            let mime = if path.contains('.') {
                mime_guess::from_path(path)
                    .first_or_octet_stream()
                    .to_string()
            } else {
                "text/html; charset=utf-8".into()
            };
            (
                [
                    (header::CONTENT_TYPE, mime),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
                    (header::CACHE_CONTROL, "no-cache".into()),
                ],
                file.data.to_vec(),
            )
                .into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Build web assets with npm run build in web/, then rebuild wakeit.",
        )
            .into_response(),
    }
}

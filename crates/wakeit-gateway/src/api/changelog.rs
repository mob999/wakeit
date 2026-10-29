//! Curated release notes shipped with this Gateway, independent of GitHub availability.
use axum::{Json, Router, extract::Query, routing::get};
use serde::{Deserialize, Serialize};

const ENGLISH: &str = include_str!("../../../../CHANGELOG.md");
const CHINESE: &str = include_str!("../../../../CHANGELOG.zh-CN.md");

#[derive(Deserialize)]
struct LanguageQuery {
    lang: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct Changelog {
    pub version: &'static str,
    pub content: &'static str,
}

async fn read(
    Query(query): Query<LanguageQuery>,
) -> Result<Json<Changelog>, crate::api::http::Error> {
    let content = match query.lang.as_deref().unwrap_or("en") {
        "en" => ENGLISH,
        "zh" => CHINESE,
        _ => {
            return Err(crate::api::http::Error::bad(
                "unsupported changelog language",
            ));
        }
    };
    Ok(Json(Changelog {
        version: env!("CARGO_PKG_VERSION"),
        content,
    }))
}

pub fn routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route("/api/v1/changelog", get(read))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn public_route_serves_embedded_locales_and_rejects_unsupported_languages() {
        let spec = crate::api::openapi::document();
        let operation = &spec["paths"]["/api/v1/changelog"]["get"];
        assert_eq!(operation["security"], serde_json::json!([]));
        assert_eq!(
            operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/Changelog"
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, routes::<()>()).await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for (query, content) in [("", ENGLISH), ("?lang=en", ENGLISH), ("?lang=zh", CHINESE)] {
            let response = client
                .get(format!("{base}/api/v1/changelog{query}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
            assert_eq!(body["content"], content);
        }
        for language in ["de", "", "zh-CN"] {
            let response = client
                .get(format!("{base}/api/v1/changelog?lang={language}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 400);
            assert_eq!(
                response.json::<Value>().await.unwrap()["error"],
                "unsupported changelog language"
            );
        }
        server.abort();
    }
}

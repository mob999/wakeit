use axum::{Router, body::Body, http::Request, response::IntoResponse};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

fn directory(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support");
    #[cfg(not(target_os = "macos"))]
    let base = home.to_owned();
    base.join("wakeit")
}
fn command(home: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_wakeit"));
    for key in [
        "WAKEIT_GATEWAY",
        "WAKEIT_WORKSPACE",
        "WAKEIT_TOKEN",
        "WAKEIT_TOKEN_FILE",
    ] {
        c.env_remove(key);
    }
    c.env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("APPDATA", home);
    c
}
fn profiles(
    home: &Path,
    gateway: &str,
    workspace: Uuid,
    users: &[Uuid],
) -> (PathBuf, Vec<PathBuf>) {
    let dir = directory(home);
    std::fs::create_dir_all(&dir).unwrap();
    let mut entries = serde_json::Map::new();
    let mut keys = vec![];
    let mut paths = vec![];
    for user in users {
        let key = wakeit::connection::scoped_name(gateway, workspace, *user);
        let path = dir.join(format!("{user}.token"));
        std::fs::write(&path, format!("key-{user}")).unwrap();
        entries.insert(key.clone(),json!({"gateway":gateway,"workspace_id":workspace,"user_id":user,"workspace_name":"Fixture","username":"fixture","token_file":path}));
        keys.push(key);
        paths.push(path);
    }
    let path = dir.join("connections.json");
    std::fs::write(
        &path,
        json!({"current":keys.last(),"connections":entries}).to_string(),
    )
    .unwrap();
    (path, paths)
}
async fn server(workspace: Uuid, user: Uuid) -> (String, Arc<Mutex<Vec<(String, String)>>>) {
    let calls = Arc::new(Mutex::new(vec![]));
    let recorded = calls.clone();
    let router = Router::new().fallback(move |request: Request<Body>| {
        let calls = recorded.clone();
        async move {
            let path = request.uri().path().to_string();
            calls.lock().unwrap().push((
                path.clone(),
                request
                    .headers()
                    .get("authorization")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            ));
            let value = if path == "/api/v1/session" {
                json!({"id":user,"workspace_id":workspace})
            } else if path == format!("/api/v1/workspaces/{workspace}") {
                json!({"id":workspace,"name":"Fixture","role":"admin"})
            } else if path == format!("/api/v1/workspaces/{workspace}/tasks") {
                json!([])
            } else {
                panic!("unexpected request: {path}")
            };
            axum::Json(value).into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, calls)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn current_account_is_used_and_explicit_credentials_override_ambiguous_profiles() {
    let home = tempfile::tempdir().unwrap();
    let workspace = Uuid::new_v4();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let (url, calls) = server(workspace, b).await;
    let (path, _) = profiles(home.path(), &url, workspace, &[a, b]);
    let before = std::fs::read(&path).unwrap();
    let output = command(home.path())
        .args(["task", "list"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let selected = command(home.path())
        .args([
            "--gateway",
            &url,
            "--workspace",
            &workspace.to_string(),
            "task",
            "list",
        ])
        .output()
        .unwrap();
    assert!(
        selected.status.success(),
        "{}",
        String::from_utf8_lossy(&selected.stderr)
    );
    let output = command(home.path())
        .args([
            "--gateway",
            &url,
            "--workspace",
            &workspace.to_string(),
            "--token",
            "explicit-key",
            "task",
            "list",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .map(|(_, auth)| auth.clone())
            .collect::<Vec<_>>(),
        vec![
            format!("Bearer key-{b}"),
            format!("Bearer key-{b}"),
            "Bearer explicit-key".into()
        ]
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_gateway_never_receives_a_saved_key() {
    let home = tempfile::tempdir().unwrap();
    let workspace = Uuid::new_v4();
    let user = Uuid::new_v4();
    profiles(home.path(), "https://previous.example", workspace, &[user]);
    let (url, calls) = server(workspace, user).await;
    let output = command(home.path())
        .args(["--gateway", &url, "task", "list"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(calls.lock().unwrap().is_empty());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_upgrade_authenticates_and_preserves_token_and_runner_paths() {
    let home = tempfile::tempdir().unwrap();
    let workspace = Uuid::new_v4();
    let user = Uuid::new_v4();
    let (url, calls) = server(workspace, user).await;
    let dir = directory(home.path());
    std::fs::create_dir_all(&dir).unwrap();
    let token = dir.join("old.token");
    std::fs::write(&token, "legacy-key").unwrap();
    let runner = dir.join("original-runner.yaml");
    std::fs::write(&runner, "fixture: untouched").unwrap();
    let path = dir.join("connections.json");
    std::fs::write(&path,json!({"current":"old","connections":{"old":{"gateway":url,"username":"fixture","token_file":token,"runner_config":runner}}}).to_string()).unwrap();
    let output = command(home.path())
        .args(["task", "list"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let key = wakeit::connection::scoped_name(&url, workspace, user);
    assert_eq!(saved["current"], key);
    assert_eq!(saved["connections"][&key]["token_file"], json!(token));
    assert_eq!(saved["connections"][&key]["runner_config"], json!(runner));
    assert_eq!(std::fs::read_to_string(token).unwrap(), "legacy-key");
    assert_eq!(
        std::fs::read_to_string(runner).unwrap(),
        "fixture: untouched"
    );
    assert_eq!(calls.lock().unwrap().len(), 3);
}

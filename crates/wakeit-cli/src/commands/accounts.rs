use super::{args::*, client::Client, display};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::time::Duration;

pub(super) async fn connection(command: &Connection) -> Result<()> {
    match command {
        Connection::List => {
            let p = crate::connection::load()?;
            display(
                json!({"current":p.current,"connections":p.connections.iter().map(|(name,p)|json!({"name":name,"gateway":p.gateway,"username":p.username,"user_id":p.user_id,"workspace_id":p.workspace_id,"workspace":p.workspace_name})).collect::<Vec<_>>()}),
                true,
            );
        }
        Connection::Use { name } => {
            crate::connection::use_profile(name)?;
            println!("Current connection: {name}");
        }
    };
    Ok(())
}
pub(super) async fn members(cli: &Cli, command: &Members) -> Result<()> {
    let c = Client::new(cli)?;
    match command {
        Members::List => display(c.get("/members?page=1&page_size=100").await?, cli.json),
        Members::Invite { role } => display(c.post("/invites", json!({"role":role})).await?, true),
    }
    Ok(())
}
pub(super) async fn login(
    cli: &Cli,
    username: &Option<String>,
    password_env: &str,
    explicit_token_file: bool,
) -> Result<()> {
    let Some(username) = username else {
        let intent = crate::device_auth::Intent {
            workspace_id: Some(
                cli.workspace
                    .context("login requires --workspace or a selected workspace connection")?,
            ),
            name: std::env::var("HOSTNAME").unwrap_or_else(|_| "wakeit-cli".into()),
            os: std::env::consts::OS.into(),
            register_runner: false,
            allowed_roots: vec![],
        };
        let p = crate::setup::login(&cli.gateway, &intent).await?;
        println!("Connected to {} as {}", p.gateway, p.username);
        return Ok(());
    };
    let workspace = cli
        .workspace
        .context("login requires --workspace or a selected workspace connection")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response=client.post(format!("{}/api/v1/session",cli.gateway)).json(&json!({"username":username,"password":std::env::var(password_env).with_context(||format!("set {password_env}"))?})).send().await?;
    let status = response.status();
    let body: Value = response.json().await?;
    ensure!(status.is_success(), "login {status}: {}", body["error"]);
    let session = body["token"].as_str().context("session token")?;
    let result: Result<crate::connection::Profile> = async {
        let response = client
            .get(format!("{}/api/v1/session", cli.gateway))
            .bearer_auth(session)
            .send()
            .await?;
        ensure!(
            response.status().is_success(),
            "account authentication failed"
        );
        let identity: Value = response.json().await?;
        let response = client
            .get(format!("{}/api/v1/workspaces/{workspace}", cli.gateway))
            .bearer_auth(session)
            .send()
            .await?;
        ensure!(response.status().is_success(), "workspace is unavailable");
        let metadata: Value = response.json().await?;
        let response = client
            .post(format!(
                "{}/api/v1/workspaces/{workspace}/tokens",
                cli.gateway
            ))
            .bearer_auth(session)
            .json(&json!({"name":"wakeit cli"}))
            .send()
            .await?;
        ensure!(response.status().is_success(), "API Key creation failed");
        let key: Value = response.json().await?;
        let token = key["token"].as_str().context("API Key missing")?;
        let user = identity["id"]
            .as_str()
            .context("account ID missing")?
            .parse()?;
        let profile = crate::connection::save(
            &cli.gateway,
            workspace,
            user,
            metadata["name"]
                .as_str()
                .context("workspace name missing")?,
            username,
            token,
        )?;
        if explicit_token_file {
            crate::connection::atomic(&cli.token_file, token)?;
        }
        Ok(profile)
    }
    .await;
    let _ = client
        .delete(format!("{}/api/v1/session", cli.gateway))
        .bearer_auth(session)
        .send()
        .await;
    let profile = result?;
    println!(
        "Connected to {} / {} as {}",
        profile.gateway,
        profile.workspace_name.as_deref().unwrap_or("workspace"),
        profile.username
    );
    Ok(())
}

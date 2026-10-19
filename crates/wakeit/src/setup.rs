use crate::{
    config::{self, AgentConfig, RunnerConfig},
    connection as c,
    device_auth::{Connect, Intent},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
async fn value(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let v: Value = response.json().await?;
    ensure!(status.is_success(), "{status}: {}", v["error"]);
    Ok(v)
}
pub fn browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(url).status()?;
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .status()?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = std::process::Command::new("xdg-open").arg(url).status()?;
    ensure!(status.success(), "could not open browser");
    Ok(())
}
pub async fn login(gateway: &str, intent: &Intent) -> Result<c::Profile> {
    let gateway = crate::device_auth::validate_url(gateway)?;
    let client = http()?;
    let grant = value(
        client
            .post(format!("{gateway}/api/v1/oauth/device/code"))
            .json(intent)
            .send()
            .await?,
    )
    .await?;
    let url = grant["verification_uri_complete"]
        .as_str()
        .context("verification URL")?;
    let parsed = reqwest::Url::parse(url)?;
    ensure!(
        parsed.origin() == reqwest::Url::parse(&gateway)?.origin(),
        "verification URL must belong to Gateway"
    );
    eprintln!(
        "Open {url}\nVerification code: {}\nWaiting for browser approval…",
        grant["user_code"].as_str().context("user code")?
    );
    if browser(url).is_err() {
        eprintln!("Open the URL above in your browser to continue.");
    }
    let end = tokio::time::Instant::now() + Duration::from_secs(600);
    let mut interval = 5;
    while tokio::time::Instant::now() < end {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let response=client.post(format!("{gateway}/api/v1/oauth/token")).json(&json!({"grant_type":"urn:ietf:params:oauth:grant-type:device_code","device_code":grant["device_code"]})).send().await.context("authorization response lost; run login/setup again to authorize")?;
        let status = response.status();
        let v: Value = response
            .json()
            .await
            .context("authorization response lost; authorize again")?;
        if status.is_success() {
            let workspace: Uuid = v["workspace_id"]
                .as_str()
                .context("workspace ID")?
                .parse()?;
            ensure!(
                intent.workspace_id == Some(workspace),
                "authorized workspace differs from the request"
            );
            let user: Uuid = v["user"]["id"].as_str().context("account ID")?.parse()?;
            let token = v["access_token"].as_str().context("access token")?;
            let metadata = value(
                client
                    .get(format!("{gateway}/api/v1/workspaces/{workspace}"))
                    .bearer_auth(token)
                    .send()
                    .await?,
            )
            .await?;
            return c::save(
                &gateway,
                workspace,
                user,
                metadata["name"].as_str().context("workspace name")?,
                v["user"]["username"].as_str().context("username")?,
                token,
            );
        }
        match v["error"].as_str().unwrap_or("") {
            "authorization_pending" => {}
            "slow_down" => interval = (interval + 5).min(60),
            "access_denied" => bail!("Browser authorization denied or account permissions changed"),
            "expired_token" => bail!("Authorization expired; run the command again"),
            _ => bail!("Authorization could not be exchanged; authorize again ({status})"),
        }
    }
    bail!("Authorization expired; run the command again")
}
#[derive(Serialize, Deserialize)]
struct Pending {
    request_id: Uuid,
    name: String,
    roots: Vec<String>,
}
fn hostname() -> String {
    let raw = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().into())
        })
        .unwrap_or_else(|| "runner".into());
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(80)
        .collect();
    if s.is_empty() { "runner".into() } else { s }
}
async fn authenticated(gateway: &str, workspace: Uuid, token: &str) -> bool {
    match http() {
        Ok(h) => h
            .get(format!("{gateway}/api/v1/workspaces/{workspace}"))
            .bearer_auth(token.trim())
            .send()
            .await
            .is_ok_and(|r| r.status().is_success()),
        _ => false,
    }
}
pub async fn run(
    gateway: &str,
    workspace: Uuid,
    name: Option<&str>,
    roots: &[PathBuf],
) -> Result<()> {
    let gateway = crate::device_auth::validate_url(gateway)?;
    let current = c::resolve(None, None)?;
    let mut old = if current
        .as_ref()
        .is_some_and(|p| p.gateway == gateway && p.workspace_id == Some(workspace))
    {
        current
    } else {
        c::resolve(Some(&gateway), Some(workspace))?
    };
    if let Some(profile) = old.take() {
        old = Some(c::upgrade_legacy(profile, Some(workspace)).await?);
    }
    let hinted = old
        .as_ref()
        .and_then(|p| p.runner_config.as_ref())
        .map(|path| crate::sessions::read_runner(path))
        .transpose()?;
    if let Some(r) = &hinted {
        ensure!(
            r.gateway.trim_end_matches('/') == gateway,
            "saved Runner belongs to another Gateway"
        );
    }
    let requested = if roots.is_empty() {
        hinted
            .as_ref()
            .map(|r| r.allowed_roots.clone())
            .unwrap_or(vec![std::env::current_dir()?])
    } else {
        roots.to_vec()
    };
    let requested: Vec<String> = requested
        .iter()
        .map(|p| Ok(p.canonicalize()?.to_string_lossy().into_owned()))
        .collect::<Result<_>>()?;
    let intent = Intent {
        workspace_id: Some(workspace),
        name: name
            .map(str::to_owned)
            .or_else(|| hinted.as_ref().map(|r| r.name.clone()))
            .unwrap_or_else(hostname),
        os: std::env::consts::OS.into(),
        register_runner: true,
        allowed_roots: requested,
    };
    let token = old
        .as_ref()
        .and_then(|p| std::fs::read_to_string(&p.token_file).ok());
    if token.as_ref().is_none()
        || !authenticated(&gateway, workspace, token.as_deref().unwrap_or("")).await
    {
        old = Some(login(&gateway, &intent).await?);
    }
    let profile = old.context("missing connection")?;
    let account = profile
        .user_id
        .context("connection account metadata is missing")?;
    ensure!(
        profile.workspace_id == Some(workspace),
        "connection belongs to another workspace"
    );
    let connection = c::scoped_name(&gateway, workspace, account);
    let dir = c::directory()?.join("runners").join(&connection);
    std::fs::create_dir_all(&dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("setup.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("another setup is in progress")?;
    let token = std::fs::read_to_string(&profile.token_file)?;
    let client = http()?;
    let metadata = value(
        client
            .get(format!("{gateway}/api/v1/workspaces/{workspace}"))
            .bearer_auth(token.trim())
            .send()
            .await?,
    )
    .await?;
    ensure!(
        metadata["role"] == "operator" || metadata["role"] == "admin",
        "Operator permission is required to register a Runner"
    );
    let config_path = profile.runner_config.clone();
    if let Some(path) = &config_path {
        let existing = crate::sessions::read_runner(path)?;
        let rows = value(
            client
                .get(format!("{gateway}/api/v1/workspaces/{workspace}/runners"))
                .bearer_auth(token.trim())
                .send()
                .await?,
        )
        .await?;
        ensure!(
            rows.as_array().is_some_and(|rows| rows
                .iter()
                .any(|r| r["id"] == existing.id.to_string() && r["revoked"] == false)),
            "saved Runner is unavailable in this workspace; refusing to reuse it"
        );
    }
    let pending_path = dir.join("registration.json");
    let existing = config_path
        .as_ref()
        .map(|p| crate::sessions::read_runner(p))
        .transpose()?;
    let requested_roots = if roots.is_empty() {
        existing
            .as_ref()
            .map(|r| r.allowed_roots.clone())
            .unwrap_or(vec![std::env::current_dir()?])
    } else {
        roots.to_vec()
    };
    let requested_roots: Vec<String> = requested_roots
        .iter()
        .map(|p| Ok(p.canonicalize()?.to_string_lossy().into_owned()))
        .collect::<Result<_>>()?;
    let pending = if pending_path.exists() {
        let p: Pending = serde_json::from_slice(&std::fs::read(&pending_path)?)?;
        ensure!(
            name.is_none_or(|n| n == p.name) && p.roots == requested_roots,
            "saved registration differs; setup will not change its identity or expand roots"
        );
        p
    } else {
        let p = Pending {
            request_id: Uuid::new_v4(),
            name: name
                .map(str::to_owned)
                .or_else(|| existing.as_ref().map(|r| r.name.clone()))
                .unwrap_or_else(hostname),
            roots: requested_roots,
        };
        config::validate_id(&p.name)?;
        c::atomic(&pending_path, serde_json::to_vec(&p)?)?;
        p
    };
    if let Some(r) = &existing {
        ensure!(
            r.gateway.trim_end_matches('/') == gateway
                && r.name == pending.name
                && r.allowed_roots
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    == pending.roots,
            "existing Runner configuration differs; refusing to overwrite"
        );
    }
    let seed_path = dir.join("identity.seed");
    if existing.is_none() && !seed_path.exists() {
        c::atomic(&seed_path, nkeys::KeyPair::new_user().seed()?)?;
    }
    let config_path = if let Some(path) = config_path {
        path
    } else {
        let key = nkeys::KeyPair::from_seed(std::fs::read_to_string(&seed_path)?.trim())?;
        let timestamp = chrono::Utc::now().timestamp();
        let signature = hex::encode(
            key.sign(
                crate::device_auth::proof(
                    pending.request_id,
                    &pending.name,
                    &pending.roots,
                    timestamp,
                )
                .as_bytes(),
            )?,
        );
        let b = Connect {
            request_id: pending.request_id,
            public_key: key.public_key(),
            signature,
            timestamp,
            name: pending.name.clone(),
            allowed_roots: pending.roots.clone(),
        };
        let response = value(
            client
                .post(format!(
                    "{gateway}/api/v1/workspaces/{workspace}/runners/connect"
                ))
                .bearer_auth(token.trim())
                .json(&b)
                .send()
                .await?,
        )
        .await?;
        c::atomic(
            &dir.join("device.token"),
            response["token"].as_str().context("device token")?,
        )?;
        c::atomic(
            &dir.join("runner.creds"),
            crate::nats_auth::credentials(response["jwt"].as_str().context("jwt")?, &key.seed()?),
        )?;
        std::fs::create_dir_all(dir.join("agents"))?;
        let mut agents = Vec::new();
        for &(id, _) in config::BUILTIN_AGENTS {
            let path = dir.join("agents").join(format!("{id}.yaml"));
            if !path.exists() {
                let mut a: AgentConfig = yaml_serde::from_str(config::builtin(id)?)?;
                config::normalize_commands(&mut a)?;
                c::atomic(&path, yaml_serde::to_string(&a)?)?;
            }
            agents.push(path);
        }
        let r = RunnerConfig {
            version: 1,
            id: response["id"].as_str().context("runner ID")?.parse()?,
            name: pending.name,
            gateway: gateway.clone(),
            token_file: dir.join("device.token"),
            credentials_file: dir.join("runner.creds"),
            nats_url: response["nats_url"].as_str().context("NATS URL")?.into(),
            data_dir: dir.join("data"),
            concurrency: 2,
            agents,
            allowed_roots: pending.roots.iter().map(PathBuf::from).collect(),
        };
        let path = dir.join("runner.yaml");
        c::atomic(&path, yaml_serde::to_string(&r)?)?;
        path
    };
    crate::sessions::register(&config_path)?;
    c::update(|all| {
        all.connections
            .get_mut(&connection)
            .context("connection disappeared")?
            .runner_config = Some(config_path.clone());
        all.current = Some(connection.clone());
        Ok(())
    })?;
    install_service(&config_path).context(
        "Connection and Runner saved; user autostart is unfinished. Run setup again to continue",
    )?;
    let r = crate::sessions::read_runner(&config_path)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        let rows = value(
            client
                .get(format!("{gateway}/api/v1/workspaces/{workspace}/runners"))
                .bearer_auth(token.trim())
                .send()
                .await?,
        )
        .await?;
        if let Some(row) = rows.as_array().and_then(|a| {
            a.iter().find(|v| {
                v["id"] == r.id.to_string()
                    && v["online"] == true
                    && v["info"]["version"] == env!("CARGO_PKG_VERSION")
                    && v["agents"].as_array().is_some_and(|a| a.len() >= 4)
            })
        }) {
            ensure!(
                authenticated(&gateway, workspace, &token).await,
                "CLI authentication was revoked"
            );
            let query = client
                .get(format!(
                    "{gateway}/api/v1/workspaces/{workspace}/runners/{}/directories",
                    r.id
                ))
                .bearer_auth(token.trim())
                .send()
                .await?;
            if !query.status().is_success() {
                ensure!(
                    tokio::time::Instant::now() < deadline,
                    "Runner is online but its query channel is unavailable"
                );
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"status":"ready","gateway":gateway,"connection":connection,"workspace_id":workspace,"runner":r.id,"name":r.name,"allowed_roots":r.allowed_roots,"agents":row["agents"].as_array().unwrap().iter().map(|a|json!({"id":a["id"],"available":a["available"],"version":a["version"]})).collect::<Vec<_>>(),"autostart":true})
                )?
            );
            return Ok(());
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Runner saved and autostart installed, but online/adapter verification timed out; inspect its service logs and rerun setup"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
#[cfg(target_os = "macos")]
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn checked(mut command: std::process::Command) -> Result<()> {
    let output = command.output()?;
    ensure!(
        output.status.success(),
        "service manager failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
pub fn install_service(config: &Path) -> Result<()> {
    let config = config.canonicalize()?;
    let r = crate::sessions::read_runner(&config)?;
    let exe = std::env::current_exe()?.canonicalize()?;
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))?;
    let path = std::env::var("PATH").unwrap_or_default();
    let label = format!("dev.wakeit.runner.{}", r.id);
    let log = r.data_dir.join("service.log");
    let _ = (&label, &path, &home, &exe, &log);
    std::fs::create_dir_all(&r.data_dir)?;
    #[cfg(target_os = "macos")]
    {
        let uid = unsafe { libc::getuid() };
        let domain = format!("gui/{uid}");
        let mut check = std::process::Command::new("launchctl");
        check.args(["print", &domain]);
        let out = check.output()?;
        ensure!(
            out.status.success(),
            "current user has no launchd GUI domain"
        );
        let target = format!("{domain}/{label}");
        let mut exists = std::process::Command::new("launchctl");
        exists.args(["print", &target]);
        if exists.output()?.status.success() {
            return Ok(());
        }
        let dir = PathBuf::from(&home).join("Library/LaunchAgents");
        std::fs::create_dir_all(&dir)?;
        // Preserve an already installed user service for the same local identity.
        for entry in std::fs::read_dir(&dir)? {
            let p = entry?.path();
            if p.extension().is_some_and(|e| e == "plist") {
                let doc = std::fs::read_to_string(&p).unwrap_or_default();
                if doc.contains(&format!(
                    "<string>{}</string>",
                    xml(&config.to_string_lossy())
                )) && doc.contains("<string>runner</string>")
                    && doc.contains("<key>RunAtLoad</key>")
                    && let Some(old) = doc
                        .split("<key>Label</key>")
                        .nth(1)
                        .and_then(|v| v.trim_start().strip_prefix("<string>"))
                        .and_then(|v| v.split("</string>").next())
                {
                    let mut q = std::process::Command::new("launchctl");
                    q.args(["print", &format!("{domain}/{old}")]);
                    if q.output()?.status.success() {
                        return Ok(());
                    }
                }
            }
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(r.data_dir.join("runner.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock).context("Runner is already running outside its installed user service; stop that process and rerun setup")?;
        drop(lock);
        let file = dir.join(format!("{label}.plist"));
        let doc = format!(
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>{}</string><string>runner</string><string>start</string><string>--config</string><string>{}</string></array><key>EnvironmentVariables</key><dict><key>HOME</key><string>{}</string><key>PATH</key><string>{}</string></dict><key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>ThrottleInterval</key><integer>5</integer><key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string></dict></plist>",
            xml(&exe.to_string_lossy()),
            xml(&config.to_string_lossy()),
            xml(&home),
            xml(&path),
            xml(&log.to_string_lossy()),
            xml(&log.to_string_lossy())
        );
        c::atomic(&file, doc)?;
        let mut cmd = std::process::Command::new("launchctl");
        cmd.args(["bootstrap", &domain]).arg(&file);
        checked(cmd)?;
    }
    #[cfg(target_os = "linux")]
    {
        fn arg(s: &str) -> String {
            format!(
                "\"{}\"",
                s.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('%', "%%")
                    .replace('$', "$$")
            )
        }
        let mut check = std::process::Command::new("systemctl");
        check.args(["--user", "show-environment"]);
        checked(check).context("systemd user manager is unavailable")?;
        let dir = PathBuf::from(&home).join(".config/systemd/user");
        std::fs::create_dir_all(&dir)?;
        let name = format!("wakeit-runner-{}.service", r.id);
        c::atomic(
            &dir.join(&name),
            format!(
                "[Unit]\nDescription=Wakeit Runner\nAfter=network-online.target\n[Service]\nExecStart={} runner start --config {}\nEnvironment={} {}\nRestart=on-failure\nRestartSec=5\n[Install]\nWantedBy=default.target\n",
                arg(&exe.to_string_lossy()),
                arg(&config.to_string_lossy()),
                arg(&format!("HOME={home}")).replace("$$", "$"),
                arg(&format!("PATH={path}")).replace("$$", "$")
            ),
        )?;
        let mut reload = std::process::Command::new("systemctl");
        reload.args(["--user", "daemon-reload"]);
        checked(reload)?;
        let mut start = std::process::Command::new("systemctl");
        start.args(["--user", "enable", "--now", &name]);
        checked(start)?;
    }
    #[cfg(target_os = "windows")]
    {
        fn ps(s: &str) -> String {
            format!("'{}'", s.replace('\'', "''"))
        }
        let script = format!(
            "$ErrorActionPreference='Stop'; $a=New-ScheduledTaskAction -Execute {} -Argument {}; $t=New-ScheduledTaskTrigger -AtLogOn -User ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name); $p=New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited; $s=New-ScheduledTaskSettingsSet -RestartCount 3 -RestartInterval (New-TimeSpan -Seconds 10) -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew; Register-ScheduledTask -TaskName {} -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null; Start-ScheduledTask -TaskName {}",
            ps(&exe.to_string_lossy()),
            ps(&format!("runner start --config \"{}\"", config.display())),
            ps(&label),
            ps(&label)
        );
        let file = r.data_dir.join("install-service.ps1");
        c::atomic(&file, script)?;
        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(file);
        checked(cmd)?;
    }
    Ok(())
}

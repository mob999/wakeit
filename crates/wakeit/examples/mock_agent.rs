//! A deterministic subprocess fixture, including a durable external-job simulator.
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("sync");
    if mode == "native" {
        let file = args.get(2).context("fixture path")?;
        let bytes = std::fs::read(file)?;
        for chunk in bytes.chunks(5) {
            std::io::stdout().write_all(chunk)?;
        }
        std::io::stdout().flush()?;
        if args.get(3).is_some_and(|s| s == "fail") {
            std::process::exit(7);
        }
        return Ok(());
    }
    if mode == "--version" {
        println!("wakeit mock agent 0.1.0");
        return Ok(());
    }
    if mode == "models" {
        let state = std::fs::read_to_string(".models-state").unwrap_or_default();
        match state.trim() {
            "fail" => anyhow::bail!("fixture discovery failure"),
            "invalid" => {
                println!("not JSON");
                return Ok(());
            }
            "sleep" => std::thread::sleep(std::time::Duration::from_secs(2)),
            "empty" => {
                println!("{}", json!({"version":1,"models":[]}));
                return Ok(());
            }
            _ => {}
        }
        println!(
            "{}",
            json!({"version":1,"models":[{"id":"fixture/model-a","name":"Model A","parameters":{"model":"model-a"},"parameter_options":{"reasoning":["low","high"]}},{"id":"fixture/model-b","name":"Model B","parameters":{"model":"model-b"}}]})
        );
        return Ok(());
    }
    let root = if mode == "session-interactive" {
        args.get(3)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(std::env::var("WAKEIT_MOCK_DIR").unwrap_or_else(|_| ".".into()))
    };
    std::fs::create_dir_all(&root)?;
    let run = std::env::var("WAKEIT_RUN_ID").unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
    if mode == "session-list" {
        let cwd = std::env::current_dir()?.canonicalize()?;
        let mut entries = vec![];
        let directory = root.join("sessions");
        if directory.exists() {
            for file in std::fs::read_dir(directory)? {
                let file = file?;
                if file
                    .path()
                    .extension()
                    .is_none_or(|extension| extension != "json")
                {
                    continue;
                }
                let info: Value = serde_json::from_slice(&std::fs::read(file.path())?)?;
                let Some(path) = info["cwd"].as_str() else {
                    continue;
                };
                if info["persisted"] == true && PathBuf::from(path).canonicalize()? == cwd {
                    entries.push(json!({"id":info["id"],"title":info.get("title"),"cwd":info["cwd"],"updated_at":info.get("updated_at")}));
                }
            }
        }
        println!(
            "{}",
            json!({"version":1,"sessions":entries,"next_cursor":null})
        );
        return Ok(());
    }
    if matches!(mode, "session-inspect" | "session-interactive") {
        let id = args.get(2).context("session id required")?;
        wakeit::sessions::validate_id(id)?;
        let data = std::fs::read_to_string(root.join("sessions").join(format!("{id}.json")))
            .context("fixture session does not exist")?;
        let info: Value = serde_json::from_str(&data)?;
        anyhow::ensure!(
            info["persisted"] == true,
            "fixture session is not persisted"
        );
        if mode == "session-inspect" {
            println!(
                "{}",
                json!({"version":info["version"],"id":info["id"],"cwd":info["cwd"],"persisted":info["persisted"],"title":info.get("title")})
            );
        } else {
            println!("continued existing session {id}");
        }
        return Ok(());
    }
    if matches!(
        mode,
        "sync" | "submit" | "events" | "session-events" | "session-resume"
    ) {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        let options = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({"text":text}));
        let delay = options["sleep"].as_f64().unwrap_or(0.1);
        let result = options
            .get("text")
            .cloned()
            .unwrap_or(json!("mock completed"));
        let mut trace = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("calls.log"))?;
        writeln!(trace, "{mode} {run}")?;
        trace.sync_all()?;
        if matches!(mode, "events" | "session-events" | "session-resume") {
            let session_info = if mode.starts_with("session-") {
                let id = if mode == "session-resume" {
                    let id = args.get(2).context("bound session id required")?.clone();
                    wakeit::sessions::validate_id(&id)?;
                    anyhow::ensure!(
                        root.join("sessions").join(format!("{id}.json")).exists(),
                        "bound session does not exist"
                    );
                    id
                } else {
                    options["session_id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("ses_{}", uuid::Uuid::new_v4().simple()))
                };
                wakeit::sessions::validate_id(&id)?;
                let directory = root.join("sessions");
                std::fs::create_dir_all(&directory)?;
                let cwd = std::env::current_dir()?
                    .canonicalize()?
                    .to_string_lossy()
                    .into_owned();
                let title = if mode == "session-resume" {
                    let existing: Value = serde_json::from_str(&std::fs::read_to_string(
                        directory.join(format!("{id}.json")),
                    )?)?;
                    existing.get("title").cloned().unwrap_or(Value::Null)
                } else {
                    args.iter()
                        .position(|arg| arg == "--title")
                        .and_then(|index| args.get(index + 1))
                        .map(|value| json!(value))
                        .unwrap_or(Value::Null)
                };
                let info = json!({"version":1,"id":id,"cwd":cwd,"persisted":true,"title":title});
                std::fs::write(directory.join(format!("{id}.json")), info.to_string())?;
                let mut reported = info;
                if let Some(id) = options["reported_session_id"].as_str() {
                    wakeit::sessions::validate_id(id)?;
                    reported["id"] = json!(id);
                }
                if options["omit_session"] == true {
                    None
                } else {
                    Some(reported)
                }
            } else {
                None
            };

            let start = chrono::Utc::now();
            let send = |typ: &str, data: Value| -> Result<()> {
                println!("{}", json!({"version":1,"type":typ,"data":data}));
                std::io::stdout().flush()?;
                Ok(())
            };
            if options["session_after_logs"] != true
                && let Some(info) = &session_info
            {
                send("session", info.clone())?;
            }
            send(
                "trace",
                json!({"id":"cmd-1","kind":"command","status":"running","title":"检查目录","input":{"command":"pwd"},"output":null,"occurred_at":start,"started_at":start}),
            )?;
            send(
                "trace",
                json!({"id":"tool-2","kind":"tool","status":"running","title":"并行检查","input":{},"output":null,"occurred_at":start,"started_at":start}),
            )?;
            std::thread::sleep(std::time::Duration::from_secs_f64(delay));
            if options["corrupt"] == true {
                println!("not JSON");
            }
            for n in 0..options["count"].as_u64().unwrap_or(1).min(6000) {
                send(
                    "log",
                    json!({"stream":"stdout","text":format!("检查 {n}\n")}),
                )?;
            }
            for n in 0..options["trace_count"].as_u64().unwrap_or(0).min(6000) {
                send(
                    "trace",
                    json!({"id":format!("message-{n}"),"kind":"message","status":"succeeded","title":"Fixture message","input":null,"output":"fixture","occurred_at":start,"finished_at":start}),
                )?;
            }
            let bytes = options["log_bytes"]
                .as_u64()
                .unwrap_or(0)
                .min(23 * 1024 * 1024);
            let mut remaining = bytes;
            while remaining > 0 {
                let size = remaining.min(256 * 1024) as usize;
                send("log", json!({"stream":"stdout","text":"x".repeat(size)}))?;
                remaining -= size as u64;
            }
            if options["session_after_logs"] == true
                && let Some(info) = &session_info
            {
                send("session", info.clone())?;
            }
            let finish = chrono::Utc::now();
            send(
                "trace",
                json!({"id":"cmd-1","kind":"command","status":"succeeded","title":"检查目录","input":{"command":"pwd"},"output":"中文目录","occurred_at":finish,"started_at":start,"finished_at":finish}),
            )?;
            send(
                "trace",
                json!({"id":"message-final","kind":"message","status":"succeeded","title":"消息","input":null,"output":result,"occurred_at":finish,"finished_at":finish}),
            )?;
            if options["fail"] == true {
                std::process::exit(7);
            }
            send("result", result)?;
            return Ok(());
        }
        if mode == "sync" {
            println!("started {run}");
            std::io::stdout().flush()?;
            std::thread::sleep(std::time::Duration::from_secs_f64(delay));
            println!("{}", result.as_str().unwrap_or("completed"));
            return Ok(());
        }
        let job = root.join(format!("{run}.json"));
        if !job.exists() {
            std::fs::write(
                job,
                json!({"finish":now()+delay,"state":"running","result":result}).to_string(),
            )?;
        }
        if options["uncertain"] == true {
            println!("response interrupted");
        } else {
            println!("{}", json!({"job_id":run}));
        }
        return Ok(());
    }
    let id = args.get(2).context("job id required")?;
    let path = root.join(format!("{id}.json"));
    let mut job: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    match mode {
        "status" => {
            let state = if job["state"] == "cancelled" {
                "cancelled"
            } else if now() >= job["finish"].as_f64().context("finish")? {
                "succeeded"
            } else {
                "running"
            };
            println!(
                "{}",
                json!({"state":state,"message":if state=="running"{"working"}else{"complete"},"result":if state=="succeeded"{job["result"].clone()}else{Value::Null}})
            );
        }
        "cancel" => {
            job["state"] = json!("cancelled");
            std::fs::write(path, job.to_string())?;
            println!("{}", json!({"accepted":true}));
        }
        _ => anyhow::bail!("unknown fixture operation"),
    }
    Ok(())
}

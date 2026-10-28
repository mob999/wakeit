//! Exercise registration response-loss recovery against a disposable test Gateway.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().expect("Gateway");
    let token_file = args.next().expect("private key file");
    let root = args.next().expect("root");
    let workspace: uuid::Uuid = args
        .next()
        .or_else(|| std::env::var("WAKEIT_TEST_WORKSPACE").ok())
        .expect("workspace UUID")
        .parse()?;
    let token = std::fs::read_to_string(token_file)?;
    let client = reqwest::Client::new();
    let key = nkeys::KeyPair::new_user();
    let id = uuid::Uuid::new_v4();
    let name = format!("recovery-{}", &id.to_string()[..8]);
    let roots = vec![root];
    let body = |key: &nkeys::KeyPair| -> Result<Value> {
        let ts = chrono::Utc::now().timestamp();
        Ok(
            json!({"request_id":id,"name":name,"allowed_roots":roots,"public_key":key.public_key(),"timestamp":ts,"signature":hex::encode(key.sign(wakeit::device_auth::proof(id,&name,&roots,ts).as_bytes())?)}),
        )
    };
    let post = |body: Value| {
        client
            .post(format!(
                "{base}/api/v1/workspaces/{workspace}/runners/connect"
            ))
            .bearer_auth(token.trim())
            .json(&body)
            .send()
    };
    let a = post(body(&key)?).await?;
    ensure!(a.status().is_success(), "first registration failed");
    let a: Value = a.json().await?;
    let b = post(body(&key)?).await?;
    ensure!(b.status().is_success(), "response-loss recovery failed");
    let b: Value = b.json().await?;
    ensure!(
        a["id"] == b["id"] && a["token"] != b["token"],
        "identity must be stable and token rotated"
    );
    let heartbeat = client
        .post(format!("{base}/api/v1/device/heartbeat"))
        .bearer_auth(a["token"].as_str().unwrap())
        .json(&json!({"runtime_id":uuid::Uuid::new_v4(),"claims":[],"info":{}}))
        .send()
        .await?;
    ensure!(
        heartbeat.status() == 401,
        "old device credential must be invalid"
    );
    ensure!(
        post(body(&nkeys::KeyPair::new_user())?).await?.status() == 403,
        "wrong key cannot recover registration"
    );
    let stranger = nkeys::KeyPair::new_user();
    let mut collision = body(&stranger)?;
    let request_id = uuid::Uuid::new_v4();
    let timestamp = chrono::Utc::now().timestamp();
    collision["request_id"] = json!(request_id);
    collision["timestamp"] = json!(timestamp);
    collision["signature"] = json!(hex::encode(
        stranger
            .sign(wakeit::device_auth::proof(request_id, &name, &roots, timestamp).as_bytes())?
    ));
    ensure!(
        post(collision).await?.status() == 409,
        "another registration must not take an existing Runner name"
    );
    println!(
        "PASS signed registration recovery preserves Runner identity, rotates credentials and rejects a different key"
    );
    Ok(())
}

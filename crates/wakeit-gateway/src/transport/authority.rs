use crate::protocol;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use nkeys::KeyPair;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn sign(key: &KeyPair, sub: &str, name: &str, nats: Value) -> Result<String> {
    let mut claims = json!({"iat":chrono::Utc::now().timestamp(),"iss":key.public_key(),"sub":sub,"name":name,"nats":nats,"jti":""});
    claims["jti"] = json!(hex::encode(Sha256::digest(serde_json::to_vec(&claims)?)));
    let data = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ed25519-nkey"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?)
    );
    Ok(format!(
        "{}.{}",
        data,
        URL_SAFE_NO_PAD.encode(key.sign(data.as_bytes())?)
    ))
}
pub use wakeit_core::credentials::{credentials, private_write};
#[derive(Clone)]
pub struct Authority {
    pub dir: PathBuf,
}
impl Authority {
    fn key(&self, name: &str) -> Result<KeyPair> {
        KeyPair::from_seed(std::fs::read_to_string(self.dir.join(format!("{name}.seed")))?.trim())
            .map_err(Into::into)
    }
    pub fn account_jwt(&self, revoked: Vec<String>) -> Result<String> {
        let account = self.key("account")?;
        let operator = self.key("operator")?;
        let mut revocations = serde_json::Map::new();
        for public in revoked {
            revocations.insert(public, json!(i64::MAX));
        }
        sign(
            &operator,
            &account.public_key(),
            "wakeit",
            json!({"type":"account","version":2,"limits":{"subs":-1,"data":-1,"payload":-1,"imports":-1,"exports":-1,"wildcards":true,"conn":-1,"leaf":-1,"mem_storage":-1,"disk_storage":-1,"streams":-1,"consumer":-1},"revocations":revocations}),
        )
    }
    pub fn runner_jwt(&self, id: Uuid, public: &str) -> Result<String> {
        ensure!(public.starts_with('U'), "expected a user public NKey");
        KeyPair::from_public_key(public)?;
        let mut publish = vec![
            protocol::event_subject(id),
            format!("{}.>", protocol::reply_prefix(id)),
        ];
        for (stream, kind) in [("WAKEIT_WORK", "work"), ("WAKEIT_CONTROL", "control")] {
            let c = protocol::consumer(id, kind);
            publish.extend([
                format!("$JS.API.STREAM.INFO.{stream}"),
                format!("$JS.API.CONSUMER.INFO.{stream}.{c}"),
                format!("$JS.API.CONSUMER.MSG.NEXT.{stream}.{c}"),
                format!("$JS.ACK.{stream}.{c}.>"),
            ]);
        }
        let key = self.key("account")?;
        sign(
            &key,
            public,
            &format!("runner-{id}"),
            json!({"type":"user","version":2,"pub":{"allow":publish},"sub":{"allow":[format!("_INBOX.r_{}.>",id.simple()),protocol::query_subject(id)]},"subs":64,"data":-1,"payload":2097152,"bearer_token":false}),
        )
    }
    pub async fn sync_revocations(&self, url: &str, pool: &sqlx::PgPool) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(7347232)")
            .execute(&mut *tx)
            .await?;
        let revoked: Vec<String> =
            sqlx::query_scalar("SELECT public_key FROM runners WHERE revoked")
                .fetch_all(&mut *tx)
                .await?;
        let jwt = self.account_jwt(revoked)?;
        let client =
            crate::transport::streams::connect(url, &self.dir.join("system.creds"), None).await?;
        let message = client.request("$SYS.REQ.CLAIMS.UPDATE", jwt.into()).await?;
        let response: Value = serde_json::from_slice(&message.payload)?;
        ensure!(
            response.get("error").is_none(),
            "NATS claim update rejected: {response}"
        );
        tx.commit().await?;
        Ok(())
    }
}
pub fn init(dir: &Path, host: &str, port: u16, ws_port: u16) -> Result<()> {
    ensure!(
        !dir.join("operator.seed").exists(),
        "NATS identity already initialized; reuse this directory"
    );
    std::fs::create_dir_all(dir)?;
    let dir = dir.canonicalize()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let op = KeyPair::new_operator();
    let account = KeyPair::new_account();
    let sys = KeyPair::new_account();
    for (name, key) in [("operator", &op), ("account", &account), ("system", &sys)] {
        private_write(&dir.join(format!("{name}.seed")), key.seed()?)?;
    }
    let operator = sign(
        &op,
        &op.public_key(),
        "wakeit",
        json!({"type":"operator","version":2,"system_account":sys.public_key()}),
    )?;
    private_write(&dir.join("operator.jwt"), &operator)?;
    let authority = Authority { dir: dir.clone() };
    let account_jwt = authority.account_jwt(vec![])?;
    let system_jwt = sign(
        &op,
        &sys.public_key(),
        "SYS",
        json!({"type":"account","version":2,"limits":{"subs":-1,"data":-1,"payload":-1,"imports":-1,"exports":-1,"conn":-1,"leaf":-1}}),
    )?;
    for (name, issuer) in [("gateway", &account), ("system", &sys)] {
        let user = KeyPair::new_user();
        let jwt = sign(
            issuer,
            &user.public_key(),
            name,
            json!({"type":"user","version":2,"subs":-1,"data":-1,"payload":-1,"pub":{"allow":[">"]},"sub":{"allow":[">"]}}),
        )?;
        private_write(
            &dir.join(format!("{name}.creds")),
            credentials(&jwt, &user.seed()?),
        )?;
    }
    // Paths are absolute so nats-server can be launched from any working directory.
    let config = format!(
        "server_name: wakeit-local\nhost: {host}\nport: {port}\nmax_payload: 2097152\noperator: {}\nsystem_account: {}\nresolver {{ type: full, dir: {}, allow_delete: true, interval: \"2s\" }}\nresolver_preload {{ {}: {}, {}: {} }}\njetstream {{ store_dir: {} }}\nwebsocket {{ host: {host}, port: {ws_port}, no_tls: true, same_origin: false }}\n",
        serde_json::to_string(&dir.join("operator.jwt"))?,
        serde_json::to_string(&sys.public_key())?,
        serde_json::to_string(&dir.join("resolver"))?,
        sys.public_key(),
        serde_json::to_string(&system_jwt)?,
        account.public_key(),
        serde_json::to_string(&account_jwt)?,
        serde_json::to_string(&dir.join("jetstream"))?
    );
    private_write(&dir.join("nats.conf"), config).context("write NATS configuration")?;
    Ok(())
}

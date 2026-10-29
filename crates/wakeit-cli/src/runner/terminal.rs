//! Runner-owned, single-controller PTYs. No terminal bytes are written to the Journal.
use crate::{
    config::RunnerConfig,
    process::ProcessGroup,
    runner::journal::Journal,
    runner::sessions,
    terminal_protocol::{self as wire, Control, Frame},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::StreamExt;
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use serde_json::{Value, json};
use sqlx::Row;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex as SyncMutex,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct Display {
    parser: vt100::Parser,
    bytes: usize,
}
struct Entry {
    id: Uuid,
    run: Uuid,
    workspace: Uuid,
    generation: AtomicI64,
    seq: AtomicU64,
    input_seq: AtomicU64,
    lease: SyncMutex<Instant>,
    state: SyncMutex<Value>,
    display: SyncMutex<Display>,
    master: SyncMutex<Option<Box<dyn MasterPty + Send>>>,
    input: SyncMutex<Option<mpsc::Sender<Vec<u8>>>>,
    stop: CancellationToken,
    control: Mutex<()>,
    slot: SyncMutex<Option<OwnedSemaphorePermit>>,
}
impl Entry {
    fn frame(&self, runtime: Uuid, kind: &str, data: Value) -> Frame {
        Frame {
            version: 1,
            workspace_id: self.workspace,
            terminal_id: self.id,
            runtime_id: runtime,
            generation: self.generation.load(Ordering::SeqCst),
            seq: self.seq.load(Ordering::SeqCst),
            kind: kind.into(),
            data,
        }
    }
    fn snapshot(&self) -> Value {
        let display = self.display.lock().unwrap();
        json!({"seq":self.seq.load(Ordering::SeqCst),"screen":STANDARD.encode(display.parser.screen().state_formatted()),"state":self.state.lock().unwrap().clone()})
    }
}
#[derive(Clone)]
pub struct Runtime {
    config: Arc<RunnerConfig>,
    journal: Journal,
    runtime: Uuid,
    slots: Arc<Semaphore>,
    entries: Arc<Mutex<HashMap<Uuid, Arc<Entry>>>>,
    recovered: Arc<Mutex<HashMap<String, OwnedSemaphorePermit>>>,
}
impl Runtime {
    pub async fn new(
        config: Arc<RunnerConfig>,
        journal: Journal,
        runtime: Uuid,
        slots: Arc<Semaphore>,
    ) -> Result<Self> {
        // Never restart a PTY after a Runner restart. Only confirmed absence releases its marker.
        let old = sqlx::query("SELECT id,pid,marker FROM local_terminals WHERE state <> 'closed'")
            .fetch_all(&journal.pool)
            .await?;
        for row in old {
            let pid: Option<i64> = row.get("pid");
            let terminal: Uuid = row.get::<String, _>("id").parse()?;
            let absent = pid.is_some_and(|pid| process_absent(terminal, pid as u32))
                || (pid.is_none() && row.get::<Option<String>, _>("marker").is_none());
            if absent && let Some(path) = row.get::<Option<String>, _>("marker") {
                let _ = std::fs::remove_file(path);
            }
            sqlx::query(
                "UPDATE local_terminals SET state=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
            )
            .bind(if absent { "closed" } else { "unknown" })
            .bind(row.get::<String, _>("id"))
            .execute(&journal.pool)
            .await?;
        }
        let service = Self {
            config,
            journal,
            runtime,
            slots,
            entries: Arc::new(Mutex::new(HashMap::new())),
            recovered: Arc::new(Mutex::new(HashMap::new())),
        };
        service.reconcile().await?;
        Ok(service)
    }
    async fn reconcile(&self) -> Result<()> {
        let rows = sqlx::query("SELECT id,pid,marker FROM local_terminals WHERE state='unknown'")
            .fetch_all(&self.journal.pool)
            .await?;
        for row in rows {
            let id: String = row.get("id");
            let pid: Option<i64> = row.get("pid");
            if pid.is_some_and(|p| {
                process_absent(id.parse().expect("journal terminal UUID"), p as u32)
            }) {
                if let Some(path) = row.get::<Option<String>, _>("marker") {
                    let _ = std::fs::remove_file(path);
                }
                sqlx::query("UPDATE local_terminals SET state='closed',updated_at=CURRENT_TIMESTAMP WHERE id=?").bind(&id).execute(&self.journal.pool).await?;
                self.recovered.lock().await.remove(&id);
                if let Ok(uuid) = id.parse::<Uuid>()
                    && let Some(entry) = self.entries.lock().await.get(&uuid)
                {
                    entry.slot.lock().unwrap().take();
                    *entry.state.lock().unwrap() = json!({"state":"closed"});
                }
            } else if !self.entries.lock().await.contains_key(&id.parse::<Uuid>()?) {
                let mut recovered = self.recovered.lock().await;
                if !recovered.contains_key(&id)
                    && let Ok(slot) = self.slots.clone().try_acquire_owned()
                {
                    recovered.insert(id, slot);
                }
            }
        }
        Ok(())
    }
    async fn emit(&self, client: &async_nats::Client, entry: &Entry, kind: &str, data: Value) {
        if let Ok(payload) = serde_json::to_vec(&entry.frame(self.runtime, kind, data)) {
            // Output is ephemeral. A disconnected/full bus must not block the local lease timer.
            let _ = tokio::time::timeout(
                Duration::from_millis(50),
                client.publish(
                    wire::output_subject(self.config.id, entry.id),
                    payload.into(),
                ),
            )
            .await;
        }
    }
    pub async fn serve(self, client: async_nats::Client, stop: CancellationToken) -> Result<()> {
        let mut sub = client
            .subscribe(wire::control_subject(self.config.id))
            .await?;
        let mut maintenance = tokio::time::interval(Duration::from_secs(5));
        loop {
            let msg = tokio::select! {_=stop.cancelled()=>break,_=maintenance.tick()=>{
                let _=self.reconcile().await;
                self.entries.lock().await.retain(|_,e| !(e.state.lock().unwrap()["state"]=="closed" && Instant::now()>*e.lease.lock().unwrap()));
                continue;
            },msg=sub.next()=>match msg{Some(m)=>m,None=>break}};
            let Some(reply) = msg.reply else { continue };
            if !reply
                .as_str()
                .starts_with(&(crate::protocol::reply_prefix(self.config.id) + "."))
            {
                continue;
            }
            let Ok(request) = serde_json::from_slice::<Control>(&msg.payload) else {
                continue;
            };
            let service = self.clone();
            let bus = client.clone();
            let shutdown = stop.clone();
            tokio::spawn(async move {
                let result = service.control(&bus, &request, &shutdown).await;
                let data = match result {
                    Ok(data) => json!({"ok":true,"data":data}),
                    Err(error) => json!({"ok":false,"error":format!("{error:#}")}),
                };
                let _ = bus
                    .publish(reply, serde_json::to_vec(&data).unwrap_or_default().into())
                    .await;
            });
        }
        for entry in self.entries.lock().await.values() {
            entry.stop.cancel();
        }
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            let finished = self.entries.lock().await.values().all(|e| {
                matches!(
                    e.state.lock().unwrap()["state"].as_str(),
                    Some("closed" | "unknown")
                )
            });
            if finished || Instant::now() > deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(())
    }
    async fn control(
        &self,
        client: &async_nats::Client,
        r: &Control,
        shutdown: &CancellationToken,
    ) -> Result<Value> {
        ensure!(
            r.version == 1 && r.runtime_id == self.runtime && r.runner_id == self.config.id,
            "Runner runtime changed"
        );
        ensure!(
            r.expires_at >= chrono::Utc::now().timestamp_millis(),
            "terminal request expired"
        );
        ensure!(!shutdown.is_cancelled(), "Runner is stopping");
        let mut entries = self.entries.lock().await;
        if r.action == "attach" && !entries.contains_key(&r.terminal_id) {
            let previous: Option<String> =
                sqlx::query_scalar("SELECT state FROM local_terminals WHERE id=?")
                    .bind(r.terminal_id.to_string())
                    .fetch_optional(&self.journal.pool)
                    .await?;
            ensure!(
                previous.is_none(),
                "previous terminal cannot be restarted; check its status"
            );
            let run = r.data["run_id"]
                .as_str()
                .context("run reference required")?
                .parse()?;
            let (cols, rows) = wire::dimensions(&r.data)?;
            // Bound display allocations, including per-cell attributes and wide characters.
            let history = 2000usize
                .min((wire::BUFFER_BYTES / 128 / (cols as usize)).saturating_sub(rows as usize));
            let entry = Arc::new(Entry {
                id: r.terminal_id,
                run,
                workspace: r.workspace_id,
                generation: AtomicI64::new(r.generation),
                seq: AtomicU64::new(0),
                input_seq: AtomicU64::new(0),
                lease: SyncMutex::new(Instant::now() + Duration::from_secs(wire::LEASE_SECONDS)),
                state: SyncMutex::new(json!({"state":"starting"})),
                display: SyncMutex::new(Display {
                    parser: vt100::Parser::new(
                        rows.try_into().unwrap(),
                        cols.try_into().unwrap(),
                        history,
                    ),
                    bytes: 0,
                }),
                master: SyncMutex::new(None),
                input: SyncMutex::new(None),
                stop: CancellationToken::new(),
                control: Mutex::new(()),
                slot: SyncMutex::new(None),
            });
            sqlx::query(
                "INSERT INTO local_terminals(id,run_id,runtime_id,state) VALUES(?,?,?,'starting')",
            )
            .bind(entry.id.to_string())
            .bind(run.to_string())
            .bind(self.runtime.to_string())
            .execute(&self.journal.pool)
            .await?;
            if self.journal.unknown_count().await? >= self.slots.available_permits() {
                sqlx::query("UPDATE local_terminals SET state='closed' WHERE id=?")
                    .bind(entry.id.to_string())
                    .execute(&self.journal.pool)
                    .await?;
                anyhow::bail!("Runner execution capacity is full")
            }
            let permit = match self.slots.clone().try_acquire_owned() {
                Ok(v) => v,
                Err(_) => {
                    sqlx::query("UPDATE local_terminals SET state='closed' WHERE id=?")
                        .bind(entry.id.to_string())
                        .execute(&self.journal.pool)
                        .await?;
                    anyhow::bail!("Runner execution capacity is full")
                }
            };
            if r.expires_at < chrono::Utc::now().timestamp_millis() {
                sqlx::query("UPDATE local_terminals SET state='closed' WHERE id=?")
                    .bind(entry.id.to_string())
                    .execute(&self.journal.pool)
                    .await?;
                anyhow::bail!("terminal request expired")
            }
            *entry.slot.lock().unwrap() = Some(permit);
            entries.insert(entry.id, entry.clone());
            let service = self.clone();
            let bus = client.clone();
            let request = r.clone();
            let shutdown = shutdown.clone();
            tokio::spawn(async move {
                service.run(bus, entry, request, shutdown).await;
            });
        }
        let entry = entries.get(&r.terminal_id).cloned();
        drop(entries);
        let Some(entry) = entry else {
            let state: Option<String> =
                sqlx::query_scalar("SELECT state FROM local_terminals WHERE id=?")
                    .bind(r.terminal_id.to_string())
                    .fetch_optional(&self.journal.pool)
                    .await?;
            return Ok(json!({"state":state.unwrap_or_else(||"missing".into())}));
        };
        let _control = entry.control.lock().await;
        ensure!(
            r.expires_at >= chrono::Utc::now().timestamp_millis(),
            "terminal request expired"
        );
        ensure!(
            r.workspace_id == entry.workspace,
            "terminal workspace changed"
        );
        if ["attach", "lease", "input", "resize"].contains(&r.action.as_str()) {
            ensure!(
                !entry.stop.is_cancelled() && Instant::now() < *entry.lease.lock().unwrap(),
                "terminal lease expired or closed"
            );
            ensure!(
                matches!(
                    entry.state.lock().unwrap()["state"].as_str(),
                    Some("starting" | "running")
                ),
                "terminal has ended"
            );
        }
        if r.action == "attach" {
            ensure!(
                r.data["run_id"]
                    .as_str()
                    .is_some_and(|id| id == entry.run.to_string()),
                "terminal source changed"
            );
            ensure!(
                r.generation >= entry.generation.load(Ordering::SeqCst),
                "stale terminal attachment"
            );
            if r.generation > entry.generation.load(Ordering::SeqCst) {
                entry.generation.store(r.generation, Ordering::SeqCst);
                entry.input_seq.store(0, Ordering::SeqCst);
            }
            *entry.lease.lock().unwrap() =
                Instant::now() + Duration::from_secs(wire::LEASE_SECONDS);
            return Ok(entry.snapshot());
        }
        ensure!(
            r.generation == entry.generation.load(Ordering::SeqCst),
            "stale terminal connection"
        );
        match r.action.as_str() {
            "status" | "snapshot" => Ok(entry.snapshot()),
            "lease" => {
                *entry.lease.lock().unwrap() =
                    Instant::now() + Duration::from_secs(wire::LEASE_SECONDS);
                Ok(json!({"state":entry.state.lock().unwrap()["state"]}))
            }
            "detach" => {
                *entry.lease.lock().unwrap() =
                    Instant::now() + Duration::from_secs(wire::LEASE_SECONDS);
                Ok(json!({}))
            }
            "close" => {
                entry.stop.cancel();
                Ok(json!({"state":entry.state.lock().unwrap()["state"]}))
            }
            "resize" => {
                let (cols, rows) = wire::dimensions(&r.data)?;
                if let Some(master) = entry.master.lock().unwrap().as_ref() {
                    master.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })?;
                }
                let mut display = entry.display.lock().unwrap();
                if (rows as usize + display.parser.screen().scrollback_capacity())
                    * (cols as usize)
                    * 128
                    > wire::BUFFER_BYTES
                {
                    let previous = display.parser.screen().state_formatted();
                    let history = 2000usize.min(
                        (wire::BUFFER_BYTES / 128 / (cols as usize)).saturating_sub(rows as usize),
                    );
                    display.parser = vt100::Parser::new(
                        rows.try_into().unwrap(),
                        cols.try_into().unwrap(),
                        history,
                    );
                    display.parser.process(previous.as_bytes());
                } else {
                    display
                        .parser
                        .set_size(rows.try_into().unwrap(), cols.try_into().unwrap());
                }
                Ok(json!({}))
            }
            "input" => {
                ensure!(
                    entry.state.lock().unwrap()["state"] == "running" && !entry.stop.is_cancelled(),
                    "terminal is not ready"
                );
                let seq = r.data["seq"].as_u64().context("input sequence required")?;
                let previous = entry.input_seq.load(Ordering::SeqCst);
                ensure!(
                    seq == previous + 1,
                    "terminal input sequence changed; reconnect"
                );
                let bytes = STANDARD.decode(r.data["data"].as_str().context("input required")?)?;
                ensure!(
                    !bytes.is_empty() && bytes.len() <= wire::FRAME_BYTES,
                    "terminal input is too large"
                );
                entry
                    .input
                    .lock()
                    .unwrap()
                    .as_ref()
                    .context("terminal is not ready")?
                    .try_send(bytes)
                    .context("terminal input is busy")?;
                entry.input_seq.store(seq, Ordering::SeqCst);
                Ok(json!({"seq":seq}))
            }
            _ => anyhow::bail!("unsupported terminal operation"),
        }
    }
    async fn run(
        &self,
        client: async_nats::Client,
        entry: Arc<Entry>,
        request: Control,
        shutdown: CancellationToken,
    ) {
        let mut marker: Option<PathBuf> = None;
        let mut spawned_pid = None;
        let mut group_bound = false;
        let result:Result<(bool,Option<u32>)>=async {
            let prepared=tokio::select!{_=entry.stop.cancelled()=>anyhow::bail!("terminal closed before start"),_=shutdown.cancelled()=>anyhow::bail!("Runner stopping"),p=sessions::prepare_local(&self.config,&self.journal,entry.run)=>p?};
            ensure!(prepared.session.id==request.data["session_id"].as_str().unwrap_or_default()
                && prepared.session.agent==request.data["agent"].as_str().unwrap_or_default(),"session identity changed");
            ensure!(!entry.stop.is_cancelled() && Instant::now()<*entry.lease.lock().unwrap(),"terminal start expired");
            let spec=&prepared.command;
            let mut command=CommandBuilder::new(&spec.command);
            let (args,cwd)=crate::process::render_command(spec,&prepared.task,entry.run,None,None)?;
            command.args(args);
            ensure!(std::fs::canonicalize(&cwd)?==std::fs::canonicalize(&prepared.session.cwd)?,"interactive command changes session working directory");
            command.cwd(cwd); command.env("TERM","xterm-256color"); command.env("COLORTERM","truecolor"); command.env("PWD",&prepared.session.cwd);
            for (key,value) in crate::process::render_environment(spec,&prepared.task,entry.run,None,None)? { command.env(key,value); }
            marker=Some(sessions::mark_continuation(&self.config,entry.run,&prepared.session)?);
            sqlx::query("UPDATE local_terminals SET marker=? WHERE id=?").bind(marker.as_ref().map(|p|p.to_string_lossy().into_owned())).bind(entry.id.to_string()).execute(&self.journal.pool).await?;
            let (cols,rows)=wire::dimensions(&request.data)?;
            let pair=portable_pty::native_pty_system().openpty(PtySize{rows,cols,pixel_width:0,pixel_height:0})?;
            let mut child=pair.slave.spawn_command(command)?;
            drop(pair.slave);
            let pid=child.process_id().context("terminal process has no identity")?;
            spawned_pid=Some(pid);
            let group=match ProcessGroup::for_pty(child.as_ref(),entry.id){Ok(group)=>group,Err(error)=>{let _=child.kill();let _=child.wait();return Err(error)}};
            group_bound=true;
            // Persist process identity before accepting keyboard input.
            if let Err(error)=sqlx::query("UPDATE local_terminals SET pid=?,state='running',updated_at=CURRENT_TIMESTAMP WHERE id=?").bind(pid as i64).bind(entry.id.to_string()).execute(&self.journal.pool).await {
                group.finish_confirmed().await; return Err(error.into())
            }
            let mut reader=pair.master.try_clone_reader()?;
            let mut writer=pair.master.take_writer()?;
            *entry.master.lock().unwrap()=Some(pair.master);
            let (input,mut inputs)=mpsc::channel::<Vec<u8>>(16);
            *entry.input.lock().unwrap()=Some(input);
            tokio::task::spawn_blocking(move || {while let Some(bytes)=inputs.blocking_recv(){if writer.write_all(&bytes).is_err(){break}}});
            let (out,mut output)=mpsc::channel::<Vec<u8>>(32);
            tokio::task::spawn_blocking(move || {let mut bytes=vec![0;16*1024];loop{match reader.read(&mut bytes){Ok(0)|Err(_)=>break,Ok(n)=>if out.blocking_send(bytes[..n].to_vec()).is_err(){break}}}});
            *entry.state.lock().unwrap()=json!({"state":"running"});
            self.emit(&client,&entry,"state",json!({"state":"running"})).await;
            let mut tick=tokio::time::interval(Duration::from_millis(50));
            let mut stopping:Option<Instant>=None;
            let mut eof=false;
            let status=loop {
                tokio::select!{
                    bytes=output.recv(), if !eof=>{
                        if let Some(bytes)=bytes {
                            {let mut display=entry.display.lock().unwrap();display.bytes=display.bytes.saturating_add(bytes.len());display.parser.process(&bytes);if display.bytes>wire::BUFFER_BYTES{display.parser.screen_mut().clear_scrollback();display.bytes=0;}entry.seq.fetch_add(1,Ordering::SeqCst);}
                            self.emit(&client,&entry,"output",json!({"data":STANDARD.encode(bytes)})).await;
                        } else {
                            eof=true;
                            // EOF can precede wait() by a scheduler tick.
                            if let Some(status)=child.try_wait()?{break status}
                        }
                    },
                    _=tick.tick()=>{
                        let terminate=entry.stop.is_cancelled() || shutdown.is_cancelled() || Instant::now()>*entry.lease.lock().unwrap();
                        if terminate && stopping.is_none(){group.terminate();stopping=Some(Instant::now());*entry.state.lock().unwrap()=json!({"state":"closing"});}
                        if stopping.is_some_and(|start|start.elapsed()>=Duration::from_secs(5)){group.kill();}
                        if let Some(status)=child.try_wait()?{break status}
                    }
                }
            };
            *entry.input.lock().unwrap()=None;
            let confirmed=group.finish_confirmed().await;
            // Consume bytes already produced before the final state, with a bounded drain.
            let drain=async {while let Some(bytes)=output.recv().await{
                {let mut display=entry.display.lock().unwrap();display.parser.process(&bytes);entry.seq.fetch_add(1,Ordering::SeqCst);}
                self.emit(&client,&entry,"output",json!({"data":STANDARD.encode(bytes)})).await;
            }};
            let _=tokio::time::timeout(Duration::from_secs(2),drain).await;
            *entry.master.lock().unwrap()=None;
            if !confirmed {return Ok((false,Some(status.exit_code())))}
            // Keep prepared (and its local Session file lock) alive until group confirmation.
            drop(prepared);
            Ok((true,Some(status.exit_code())))
        }.await;
        let (state, code, reason) = match result {
            Ok((true, code)) => ("closed", code, None),
            Ok((false, code)) => (
                "unknown",
                code,
                Some("could not confirm terminal process tree stopped".to_owned()),
            ),
            Err(error) => {
                let pid = spawned_pid;
                (
                    if pid.is_some_and(|pid| !group_bound || !process_absent(entry.id, pid)) {
                        "unknown"
                    } else {
                        "closed"
                    },
                    None,
                    Some(format!("{error:#}")),
                )
            }
        };
        if state == "closed" {
            if let Some(path) = marker {
                let _ = std::fs::remove_file(path);
            }
            entry.slot.lock().unwrap().take();
        }
        let _ = sqlx::query(
            "UPDATE local_terminals SET state=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
        )
        .bind(state)
        .bind(entry.id.to_string())
        .execute(&self.journal.pool)
        .await;
        *entry.state.lock().unwrap() = json!({"state":state,"exit_code":code,"reason":reason});
        *entry.lease.lock().unwrap() = Instant::now() + Duration::from_secs(wire::LEASE_SECONDS);
        let state = entry.state.lock().unwrap().clone();
        self.emit(&client, &entry, "state", state).await;
    }
}
#[cfg(unix)]
fn process_absent(_terminal: Uuid, pid: u32) -> bool {
    unsafe {
        libc::kill(-(pid as i32), 0) == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
}
#[cfg(windows)]
fn process_absent(terminal: Uuid, pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::*,
        System::{JobObjects::*, SystemServices::JOB_OBJECT_QUERY, Threading::*},
    };
    unsafe {
        // A PID alone cannot confirm that descendants have exited on Windows.
        let name = crate::process::terminal_job_name(terminal);
        let job = OpenJobObjectW(JOB_OBJECT_QUERY, 0, name.as_ptr());
        if !job.is_null() {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            let ok = QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            ) != 0;
            CloseHandle(job);
            return ok && info.ActiveProcesses == 0;
        }
        if GetLastError() != ERROR_FILE_NOT_FOUND {
            return false;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return GetLastError() == ERROR_INVALID_PARAMETER;
        }
        let mut code = 0;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code != 259
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_restores_unicode_cursor_and_native_input_modes() {
        let mut parser = vt100::Parser::new(24.try_into().unwrap(), 80.try_into().unwrap(), 20);
        parser.process("中文\r\nline\x1b[?2004h\x1b[?1h\x1b[3;4H".as_bytes());
        let snapshot = parser.screen().state_formatted();
        let mut recovered = vt100::Parser::new(24.try_into().unwrap(), 80.try_into().unwrap(), 20);
        recovered.process(snapshot.as_bytes());
        assert_eq!(recovered.screen().contents(), parser.screen().contents());
        assert_eq!(
            recovered.screen().cursor_position(),
            parser.screen().cursor_position()
        );
        assert_eq!(
            recovered.screen().input_mode_formatted(),
            parser.screen().input_mode_formatted()
        );
    }
}

use crate::{
    config::{CommandArg, CommandSpec, OutputFormat, OutputSpec, TaskConfig, duration},
    journal::Journal,
};
use anyhow::{Context, Result, ensure};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub result: Option<serde_json::Value>,
    pub stopped: Option<&'static str>,
    pub confirmed: bool,
    pub stdout_truncated: bool,
    output_error: Option<String>,
}
impl Output {
    /// Decode only a complete result. A display-sized prefix is never treated as
    /// valid JSON, nor exposed to downstream DAG nodes as complete text.
    pub fn result_value(&self, spec: &OutputSpec) -> Result<serde_json::Value> {
        if let Some(error) = &self.output_error {
            anyhow::bail!("{error}");
        }
        if self.stdout_truncated && !matches!(spec.format, OutputFormat::Events) {
            return Ok(crate::output_capture::preview(&self.stdout));
        }
        let mut value = match spec.format {
            OutputFormat::Text => serde_json::json!(self.stdout),
            OutputFormat::Json => serde_json::from_str(&self.stdout)?,
            OutputFormat::Jsonl => serde_json::Value::Array(
                self.stdout
                    .lines()
                    .map(serde_json::from_str)
                    .collect::<std::result::Result<_, _>>()?,
            ),
            OutputFormat::Events => self.result.clone().unwrap_or(serde_json::Value::Null),
        };
        // Bounded event results already carry an explicit incomplete marker.
        // Never report a missing pointer just because only a preview was retained.
        if value.get("truncated") != Some(&serde_json::Value::Bool(true))
            && let Some(pointer) = &spec.result_pointer
        {
            value = value
                .pointer(pointer)
                .cloned()
                .context("output result_pointer not found")?;
        }
        Ok(value)
    }
}
#[derive(Debug)]
pub struct CommandStartedError(pub anyhow::Error);
impl std::fmt::Display for CommandStartedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "command monitoring failed after start: {}", self.0)
    }
}
impl std::error::Error for CommandStartedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}
#[derive(Clone)]
pub struct LogSink {
    pub journal: Journal,
    pub id: Uuid,
    pub stdout: bool,
}
pub async fn execute(
    spec: &CommandSpec,
    task: &TaskConfig,
    id: Uuid,
    job: Option<&str>,
    timeout: Duration,
    cancel: CancellationToken,
    sink: Option<LogSink>,
) -> Result<Output> {
    execute_output(spec, task, id, job, timeout, cancel, sink, false).await
}
pub async fn execute_events(
    spec: &CommandSpec,
    task: &TaskConfig,
    id: Uuid,
    job: Option<&str>,
    timeout: Duration,
    cancel: CancellationToken,
    sink: Option<LogSink>,
) -> Result<Output> {
    execute_output(spec, task, id, job, timeout, cancel, sink, true).await
}
#[allow(clippy::too_many_arguments)]
pub async fn execute_output(
    spec: &CommandSpec,
    task: &TaskConfig,
    id: Uuid,
    job: Option<&str>,
    timeout: Duration,
    cancel: CancellationToken,
    sink: Option<LogSink>,
    events: bool,
) -> Result<Output> {
    execute_output_named(spec, task, id, job, timeout, cancel, sink, events, None).await
}
#[allow(clippy::too_many_arguments)]
pub async fn execute_named(
    spec: &CommandSpec,
    task: &TaskConfig,
    id: Uuid,
    job: Option<&str>,
    timeout: Duration,
    cancel: CancellationToken,
    sink: Option<LogSink>,
    title: Option<&str>,
) -> Result<Output> {
    execute_output_named(spec, task, id, job, timeout, cancel, sink, false, title).await
}
#[allow(clippy::too_many_arguments)]
pub async fn execute_output_named(
    spec: &CommandSpec,
    task: &TaskConfig,
    id: Uuid,
    job: Option<&str>,
    timeout: Duration,
    cancel: CancellationToken,
    sink: Option<LogSink>,
    events: bool,
    title: Option<&str>,
) -> Result<Output> {
    execute_output_named_format(
        spec,
        task,
        id,
        job,
        timeout,
        cancel,
        sink,
        if events {
            OutputFormat::Events
        } else {
            OutputFormat::Text
        },
        title,
    )
    .await
}
/// Runner execution knows the configured result format, unlike read-only probes.
#[allow(clippy::too_many_arguments)]
pub async fn execute_output_named_format(
    spec: &CommandSpec,
    task: &TaskConfig,
    id: Uuid,
    job: Option<&str>,
    timeout: Duration,
    cancel: CancellationToken,
    sink: Option<LogSink>,
    format: OutputFormat,
    title: Option<&str>,
) -> Result<Output> {
    let render = |value: &str, task: &TaskConfig, id: Uuid, job: Option<&str>| {
        crate::config::render_named(value, task, id, job, title)
    };
    #[cfg(windows)]
    let executable = which::which(&spec.command).context("executable not found")?;
    #[cfg(windows)]
    ensure!(
        !executable
            .extension()
            .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("cmd")
                || e.to_string_lossy().eq_ignore_ascii_case("bat")),
        "configure an explicit interpreter and script entry point on Windows"
    );
    #[cfg(windows)]
    let mut command = Command::new(executable);
    #[cfg(not(windows))]
    let mut command = Command::new(&spec.command);
    for arg in &spec.args {
        match arg {
            CommandArg::Literal(v) => {
                command.arg(render(v, task, id, job)?);
            }
            CommandArg::Group { when_set, args }
                if task.parameters.get(when_set).is_some_and(|v| !v.is_empty()) =>
            {
                for v in args {
                    command.arg(render(v, task, id, job)?);
                }
            }
            CommandArg::Group { .. } => {}
        }
    }
    let cwd = spec.cwd.as_deref().unwrap_or(&task.cwd);
    let cwd = render(cwd, task, id, job)?;
    command.current_dir(&cwd);
    for (key, value) in &spec.env {
        command.env(key, render(value, task, id, job)?);
    }
    for (key, source) in &spec.env_from {
        command.env(
            key,
            std::env::var(source)
                .with_context(|| format!("environment variable {source} missing"))?,
        );
    }
    command
        .env("PWD", &cwd)
        .env("WAKEIT_RUN_ID", id.to_string())
        .env("WAKEIT_TASK_ID", &task.id);
    command
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    {
        command.creation_flags(0x00000200);
        ensure!(
            !spec.command.to_lowercase().ends_with(".cmd")
                && !spec.command.to_lowercase().ends_with(".bat"),
            "use an explicit interpreter and script entry point on Windows"
        );
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("start {}", spec.command))?;
    let result: Result<Output> = async {
    let group = ProcessGroup::new(&child)?;
    let stdout = child.stdout.take().context("stdout")?;
    let stderr = child.stderr.take().context("stderr")?;
    let out_sink = sink.clone().filter(|s| s.stdout);
    let err_sink = sink;
    let mut stdout = tokio::spawn(read_output(stdout, out_sink, "stdout", format));
    let mut stderr = tokio::spawn(read_output(stderr, err_sink, "stderr", OutputFormat::Text));
    let input = spec
        .stdin
        .as_ref()
        .map(|s| render(s, task, id, job))
        .transpose()?;
    // Write independently so output pipes cannot deadlock while input is consumed.
    if let Some(input) = input
        && let Some(mut stdin) = child.stdin.take()
    {
        tokio::spawn(async move {
            let _ = stdin.write_all(input.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }
    let limit = spec
        .timeout
        .as_deref()
        .map(duration)
        .transpose()?
        .unwrap_or(timeout)
        .min(timeout);
    let mut stopped = None;
    let status = tokio::select! {
        result=child.wait()=>result?,
        _=cancel.cancelled()=>{stopped=Some("cancelled");group.terminate();match tokio::time::timeout(Duration::from_secs(5),child.wait()).await{Ok(v)=>v?,Err(_)=>{group.kill();child.kill().await.ok();child.wait().await?}}},
        _=tokio::time::sleep(limit)=>{stopped=Some("timed_out");group.terminate();match tokio::time::timeout(Duration::from_secs(5),child.wait()).await{Ok(v)=>v?,Err(_)=>{group.kill();child.kill().await.ok();child.wait().await?}}}
    };
    // Commands belong to a run; terminate any descendants holding the output pipes.
    let mut confirmed = group.finish().await;
    let out = match tokio::time::timeout(Duration::from_secs(5), &mut stdout).await {
        Ok(r) => r??,
        Err(_) => {
            stdout.abort();
            confirmed = false;
            ReadOutput::default()
        }
    };
    match tokio::time::timeout(Duration::from_secs(5), &mut stderr).await {
        Ok(r) => {
            r??;
        }
        Err(_) => {
            stderr.abort();
            confirmed = false;
        }
    };
    Ok(Output {
        code: status.code(),
        stdout: String::from_utf8_lossy(&out.bytes).into_owned(),
        result: out.result,
        stopped,
        confirmed,
        stdout_truncated: out.truncated,
        output_error: out.output_error,
    })
    }.await;
    result.map_err(|error| CommandStartedError(error).into())
}
#[derive(Default)]
struct ReadOutput {
    bytes: Vec<u8>,
    result: Option<serde_json::Value>,
    truncated: bool,
    output_error: Option<String>,
}
async fn stream_record(
    record: anyhow::Result<crate::trace::StreamRecord>,
    sink: &Option<LogSink>,
    result: &mut Option<serde_json::Value>,
    diagnosed: &mut std::collections::BTreeSet<crate::trace::RecordingIssue>,
) -> Result<()> {
    use crate::trace::RecordData;
    match record {
        Ok(record) => match record.record {
            RecordData::Trace(node) => {
                let shortened = node.input["truncated"] == true || node.output["truncated"] == true;
                if let Some(s) = sink {
                    let recorded = s
                        .journal
                        .content(s.id, "trace", serde_json::to_value(node)?)
                        .await?;
                    if recorded && shortened {
                        recording_issue(
                            sink,
                            diagnosed,
                            crate::trace::RecordingIssue::ContentShortened,
                        )
                        .await?;
                    }
                }
            }
            RecordData::Log(log) => {
                if let Some(s) = sink {
                    s.journal.log(s.id, &log.stream, &log.text).await?;
                }
                if let Some(issue) = log.recording_issue {
                    recording_issue(sink, diagnosed, issue).await?;
                }
            }
            RecordData::Session(info) => {
                if let Some(s) = sink {
                    s.journal.record_session(s.id, info).await?;
                }
            }
            RecordData::Result(value) => {
                let encoded = serde_json::to_string(&value)?;
                *result = Some(if encoded.len() > crate::trace::RECORD_LIMIT - 4096 {
                    let mut end = encoded.len().min(64 * 1024);
                    while !encoded.is_char_boundary(end) {
                        end -= 1;
                    }
                    serde_json::json!({"truncated":true,"preview":&encoded[..end]})
                } else {
                    value
                });
            }
        },
        Err(error) => {
            let issue = if error
                .downcast_ref::<crate::trace::RecordTooLarge>()
                .is_some()
            {
                crate::trace::RecordingIssue::EventTooLarge
            } else {
                crate::trace::RecordingIssue::EventParseFailed
            };
            let first = !diagnosed.contains(&issue);
            recording_issue(sink, diagnosed, issue).await?;
            if first && let Some(s) = sink {
                s.journal
                    .log(
                        s.id,
                        "stderr",
                        "Invalid or oversized Agent event omitted.\n",
                    )
                    .await?;
            }
        }
    }
    Ok(())
}
async fn recording_issue(
    sink: &Option<LogSink>,
    diagnosed: &mut std::collections::BTreeSet<crate::trace::RecordingIssue>,
    issue: crate::trace::RecordingIssue,
) -> Result<()> {
    if diagnosed.insert(issue)
        && let Some(s) = sink
    {
        s.journal
            .emit(
                s.id,
                "trace_status",
                serde_json::json!({"status": issue.status(), "reason": issue}),
                None,
            )
            .await?;
    }
    Ok(())
}
async fn read_output(
    mut reader: impl AsyncRead + Unpin,
    sink: Option<LogSink>,
    stream: &str,
    format: OutputFormat,
) -> Result<ReadOutput> {
    let events = matches!(format, OutputFormat::Events);
    let mut validation = crate::output_capture::Validation::new(&format);
    let mut truncated = false;
    let mut all = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut carry = Vec::new();
    let mut lines = crate::trace::Lines::default();
    let mut result = None;
    let mut diagnosed = std::collections::BTreeSet::new();
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        if events {
            for record in lines.push(&buffer[..n]) {
                stream_record(record, &sink, &mut result, &mut diagnosed).await?;
            }
            continue;
        }
        if let Some(validation) = &mut validation {
            validation.push(&buffer[..n]).await;
        }
        let retained = n.min(crate::output_capture::RETAIN_BYTES - all.len());
        all.extend_from_slice(&buffer[..retained]);
        truncated |= retained < n;
        if let Some(s) = &sink {
            carry.extend_from_slice(&buffer[..n]);
            let ready = match std::str::from_utf8(&carry) {
                Ok(_) => carry.len(),
                Err(e) if e.error_len().is_none() => e.valid_up_to(),
                Err(_) => carry.len(),
            };
            if ready > 0 {
                s.journal
                    .log(s.id, stream, &String::from_utf8_lossy(&carry[..ready]))
                    .await?;
                carry.drain(..ready);
            }
        }
    }
    if let Some(s) = &sink
        && !carry.is_empty()
    {
        s.journal
            .log(s.id, stream, &String::from_utf8_lossy(&carry))
            .await?;
    }
    if events {
        for record in lines.finish() {
            stream_record(record, &sink, &mut result, &mut diagnosed).await?;
        }
    }
    let output_error = match validation {
        Some(validation) => validation.finish().await,
        None => None,
    };
    Ok(ReadOutput {
        bytes: all,
        result,
        truncated,
        output_error,
    })
}
pub fn validate_cwd(cwd: &str, roots: &[std::path::PathBuf]) -> Result<()> {
    let cwd = Path::new(cwd)
        .canonicalize()
        .context("working directory does not exist")?;
    ensure!(cwd.is_dir(), "working directory must be a directory");
    ensure!(!roots.is_empty(), "allowed_roots is not configured");
    {
        let roots = roots
            .iter()
            .map(|p| p.canonicalize())
            .collect::<std::io::Result<Vec<_>>>()?;
        ensure!(
            roots.iter().any(|r| cwd.starts_with(r)),
            "working directory is outside allowed_roots"
        );
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) struct ProcessGroup(i32);
#[cfg(unix)]
impl ProcessGroup {
    pub(crate) fn new(child: &tokio::process::Child) -> Result<Self> {
        Ok(Self(child.id().context("process id")? as i32))
    }
    fn signal(&self, signal: i32) -> bool {
        let r = unsafe { libc::kill(-self.0, signal) };
        let error = std::io::Error::last_os_error();
        if r == 0 || error.raw_os_error() == Some(libc::ESRCH) {
            return true;
        }
        // The group may disappear between the child's exit and our final signal.
        let absent = unsafe { libc::kill(-self.0, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if !absent {
            tracing::debug!(group=self.0,signal,error=%error,"process group signal not confirmed");
        }
        absent
    }
    pub(crate) fn terminate(&self) -> bool {
        self.signal(libc::SIGTERM)
    }
    pub(crate) fn kill(&self) -> bool {
        self.signal(libc::SIGKILL)
    }
    pub(crate) async fn finish(&self) -> bool {
        if self.kill() {
            return true;
        }
        // Darwin can report EPERM while an exited, orphaned group is being reaped.
        // Only absence confirms that case; never treat EPERM itself as success.
        for _ in 0..40 {
            if unsafe { libc::kill(-self.0, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        false
    }
}
impl ProcessGroup {
    #[cfg(unix)]
    pub(crate) fn for_pty(child: &dyn portable_pty::Child, _terminal: uuid::Uuid) -> Result<Self> {
        Ok(Self(
            child.process_id().context("terminal process id")? as i32
        ))
    }
    #[cfg(windows)]
    pub(crate) fn for_pty(child: &dyn portable_pty::Child, terminal: uuid::Uuid) -> Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        let name = terminal_job_name(terminal);
        unsafe {
            use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
            let h = CreateJobObjectW(std::ptr::null(), name.as_ptr());
            ensure!(!h.is_null(), "CreateJobObject failed");
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(h);
                anyhow::bail!("terminal process job already exists");
            }
            let group = Self(h);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            ensure!(
                SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of_val(&info) as u32
                ) != 0,
                "SetInformationJobObject failed"
            );
            ensure!(
                AssignProcessToJobObject(
                    h,
                    child.as_raw_handle().context("terminal process handle")? as _
                ) != 0,
                "AssignProcessToJobObject failed"
            );
            Ok(group)
        }
    }
    /// Interactive continuation must not release its session lease until every owned process is gone.
    pub(crate) async fn finish_confirmed(&self) -> bool {
        self.kill();
        for _ in 0..200 {
            if self.empty() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        false
    }
    #[cfg(unix)]
    pub(crate) fn interrupt(&self) {
        self.signal(libc::SIGINT);
    }
    #[cfg(unix)]
    fn empty(&self) -> bool {
        (unsafe { libc::kill(-self.0, 0) }) == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
    #[cfg(windows)]
    fn empty(&self) -> bool {
        use windows_sys::Win32::System::JobObjects::*;
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        unsafe {
            QueryInformationJobObject(
                self.0,
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            ) != 0
                && info.ActiveProcesses == 0
        }
    }
}
#[cfg(unix)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.kill();
    }
}
#[cfg(windows)]
pub(crate) struct ProcessGroup(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for ProcessGroup {}
#[cfg(windows)]
unsafe impl Sync for ProcessGroup {}
#[cfg(windows)]
impl ProcessGroup {
    pub(crate) async fn finish(&self) -> bool {
        self.kill()
    }
    pub(crate) fn new(child: &tokio::process::Child) -> Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            ensure!(!h.is_null(), "CreateJobObject failed");
            let group = Self(h);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            ensure!(
                SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of_val(&info) as u32
                ) != 0,
                "SetInformationJobObject failed"
            );
            ensure!(
                AssignProcessToJobObject(h, child.raw_handle().context("process handle")? as _)
                    != 0,
                "AssignProcessToJobObject failed"
            );
            Ok(group)
        }
    }
    pub(crate) fn terminate(&self) -> bool {
        self.kill()
    }
    pub(crate) fn kill(&self) -> bool {
        unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 143) != 0 }
    }
}
#[cfg(windows)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
pub(crate) fn terminal_job_name(id: uuid::Uuid) -> Vec<u16> {
    format!("Local\\wakeit-terminal-{}", id.simple())
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

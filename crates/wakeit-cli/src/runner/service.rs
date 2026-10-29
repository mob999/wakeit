use crate::connection as c;
use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};

pub fn render(config: &Path, platform: &str) -> Result<String> {
    let config = config.canonicalize()?;
    let executable = std::env::current_exe()?;
    fn xml(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    Ok(match platform {
        "linux" => format!(
            "[Unit]\nDescription=wakeit runner\nAfter=network-online.target\n[Service]\nExecStart=\"{}\" runner start --config \"{}\"\nRestart=on-failure\nRestartSec=5\nKillMode=control-group\n[Install]\nWantedBy=default.target\n",
            executable
                .display()
                .to_string()
                .replace('%', "%%")
                .replace('"', "\\\""),
            config
                .display()
                .to_string()
                .replace('%', "%%")
                .replace('"', "\\\"")
        ),
        "macos" => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>dev.wakeit.runner</string><key>ProgramArguments</key><array><string>{}</string><string>runner</string><string>start</string><string>--config</string><string>{}</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><true/></dict></plist>\n",
            xml(&executable.to_string_lossy()),
            xml(&config.to_string_lossy())
        ),
        "windows" => format!(
            "$action = New-ScheduledTaskAction -Execute '{}' -Argument 'runner start --config \"{}\"'\n$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME\nRegister-ScheduledTask -TaskName 'wakeit runner' -Action $action -Trigger $trigger -Description 'wakeit agent runner'\n",
            executable.display().to_string().replace('\'', "''"),
            config.display().to_string().replace('\'', "''")
        ),
        _ => bail!("platform must be linux, macos or windows"),
    })
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
    let r = crate::runner::config::read_runner(&config)?;
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

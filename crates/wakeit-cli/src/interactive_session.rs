//! An interactive Agent owns its terminal and process tree for the lifetime of its local lease.
use crate::process::{CommandStartedError, ProcessGroup};
use anyhow::{Context, Result, ensure};
#[cfg(unix)]
use std::time::Duration;
use tokio::process::Command;

#[cfg(unix)]
struct Terminal {
    fd: std::os::fd::OwnedFd,
    foreground: libc::pid_t,
    settings: libc::termios,
}
#[cfg(unix)]
fn foreground(fd: std::os::fd::RawFd, group: libc::pid_t) -> std::io::Result<()> {
    // A background parent must restore the foreground without stopping itself with SIGTTOU.
    unsafe {
        let mut blocked: libc::sigset_t = std::mem::zeroed();
        let mut previous: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut blocked);
        libc::sigaddset(&mut blocked, libc::SIGTTOU);
        let changed = libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous);
        if changed != 0 {
            return Err(std::io::Error::from_raw_os_error(changed));
        }
        let result = libc::tcsetpgrp(fd, group);
        let error = std::io::Error::last_os_error();
        libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
        if result == -1 { Err(error) } else { Ok(()) }
    }
}
#[cfg(unix)]
impl Terminal {
    fn capture() -> Result<Option<Self>> {
        use std::os::fd::FromRawFd;
        if unsafe { libc::isatty(libc::STDIN_FILENO) } != 1 {
            return Ok(None);
        }
        let saved = unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) };
        ensure!(saved >= 0, "cannot read terminal foreground group");
        ensure!(
            saved == unsafe { libc::getpgrp() },
            "start session continuation in the terminal foreground"
        );
        let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
        ensure!(fd >= 0, "cannot retain the session terminal");
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
        let mut settings = unsafe { std::mem::zeroed() };
        use std::os::fd::AsRawFd;
        ensure!(
            unsafe { libc::tcgetattr(fd.as_raw_fd(), &mut settings) } == 0,
            "cannot read terminal settings"
        );
        Ok(Some(Self {
            fd,
            foreground: saved,
            settings,
        }))
    }
    fn prepare(&self, command: &mut Command) {
        use std::os::fd::AsRawFd;
        let fd = self.fd.as_raw_fd();
        // This runs after setpgid and before exec, so native terminal reads cannot race the handoff.
        unsafe {
            command.pre_exec(move || foreground(fd, libc::getpgrp()));
        }
    }
}
#[cfg(unix)]
impl Drop for Terminal {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        let fd = self.fd.as_raw_fd();
        if let Err(error) = foreground(fd, self.foreground) {
            tracing::warn!(%error, "could not restore terminal foreground");
        }
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, &self.settings);
        }
    }
}

/// Success means that the native process and its owned descendants have stopped.
/// An error tagged CommandStartedError requires retaining durable local occupancy.
pub(crate) async fn execute(mut command: Command) -> Result<i32> {
    #[cfg(unix)]
    let terminal = Terminal::capture()?;
    #[cfg(unix)]
    {
        command.process_group(0);
        if let Some(terminal) = &terminal {
            terminal.prepare(&mut command);
        }
    }
    let mut child = command
        .kill_on_drop(true)
        .spawn()
        .context("could not start session continuation")?;
    let result: Result<i32> = async {
        let group = ProcessGroup::new(&child)?;
        #[cfg(unix)]
        let mut terminated =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        #[cfg(unix)]
        let status = tokio::select! {
            result = child.wait() => result?,
            result = tokio::signal::ctrl_c() => {
                result?;
                group.interrupt();
                match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                    Ok(result) => result?,
                    Err(_) => { group.kill(); child.wait().await? },
                }
            },
            _ = terminated.recv() => {
                group.terminate();
                match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                    Ok(result) => result?,
                    Err(_) => { group.kill(); child.wait().await? },
                }
            }
        };
        #[cfg(windows)]
        let status = loop {
            tokio::select! {
                result = child.wait() => break result?,
                result = tokio::signal::ctrl_c() => {
                    // The native child shares the console and receives Ctrl-C itself.
                    // TUIs may cancel the current turn without exiting; keep their terminal alive.
                    result?;
                }
            }
        };
        ensure!(
            group.finish_confirmed().await,
            "could not confirm the continuation process tree has stopped"
        );
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            Ok(status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)))
        }
        #[cfg(windows)]
        Ok(status.code().unwrap_or(1))
    }
    .await;
    result.map_err(|error| CommandStartedError(error).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Stdio, time::Duration};

    #[test]
    #[ignore = "native child fixture"]
    fn descendant_leaf() {
        let target = std::env::var_os("WAKEIT_TEST_CONTINUATION_LEAF").unwrap();
        std::thread::sleep(Duration::from_secs(2));
        std::fs::write(target, "descendant survived").unwrap();
    }
    #[test]
    #[ignore = "native child fixture"]
    #[allow(clippy::zombie_processes)] // Intentionally orphan the leaf to verify the owning process group/Job cleans it up.
    fn descendant_parent() {
        let _child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "interactive_session::tests::descendant_leaf",
                "--nocapture",
            ])
            .spawn()
            .unwrap();
    }
    #[tokio::test]
    async fn native_exit_cleans_up_remaining_descendants() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("survived");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "interactive_session::tests::descendant_parent",
                "--nocapture",
            ])
            .env("WAKEIT_TEST_CONTINUATION_LEAF", &target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Some init processes retain orphan zombies. A conservative error is valid, but a writer must never survive.
        if let Err(error) = execute(command).await {
            assert!(error.downcast_ref::<CommandStartedError>().is_some());
        }
        tokio::time::sleep(Duration::from_millis(2200)).await;
        assert!(
            !target.exists(),
            "the native child exited but a descendant kept running"
        );
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "terminal child fixture"]
    fn native_tty_probe() {
        assert_eq!(unsafe { libc::isatty(0) }, 1);
        assert_eq!(unsafe { libc::tcgetpgrp(0) }, unsafe { libc::getpgrp() });
        let mut settings = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::tcgetattr(0, &mut settings) }, 0);
        settings.c_lflag &= !libc::ECHO;
        assert_eq!(unsafe { libc::tcsetattr(0, libc::TCSANOW, &settings) }, 0);
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        println!("tty-child-received:{}", line.trim());
    }
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "terminal owner fixture"]
    async fn terminal_owner_probe() {
        let mut original = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::tcgetattr(0, &mut original) }, 0);
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "interactive_session::tests::native_tty_probe",
                "--nocapture",
            ])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        assert_eq!(execute(command).await.unwrap(), 0);
        assert_eq!(unsafe { libc::tcgetpgrp(0) }, unsafe { libc::getpgrp() });
        let mut restored = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::tcgetattr(0, &mut restored) }, 0);
        assert_eq!(original.c_lflag, restored.c_lflag);
        println!("tty-parent-restored");
    }
    #[cfg(unix)]
    #[test]
    fn pseudo_terminal_is_given_to_native_child_and_restored_afterwards() {
        use std::{
            io::{Read, Write},
            os::{fd::FromRawFd, unix::process::CommandExt},
        };
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let mut master = unsafe { std::fs::File::from_raw_fd(master) };
        let slave = unsafe { std::fs::File::from_raw_fd(slave) };
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "interactive_session::tests::terminal_owner_probe",
                "--nocapture",
            ])
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        drop(slave);
        master.write_all(b"hello-session\n").unwrap();
        use std::os::fd::AsRawFd;
        let fd = master.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        let started = std::time::Instant::now();
        let mut transcript = vec![];
        let status = loop {
            let mut bytes = [0u8; 4096];
            match master.read(&mut bytes) {
                Ok(count) => transcript.extend_from_slice(&bytes[..count]),
                Err(error)
                    if matches!(error.kind(), std::io::ErrorKind::WouldBlock)
                        || error.raw_os_error() == Some(libc::EIO) => {}
                Err(error) => panic!("read terminal: {error}"),
            }
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > Duration::from_secs(12) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!(
                    "terminal fixture timed out: {}",
                    String::from_utf8_lossy(&transcript)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let output = String::from_utf8_lossy(&transcript);
        assert!(status.success(), "{output}");
        assert!(
            output.contains("tty-child-received:hello-session"),
            "{output}"
        );
        assert!(output.contains("tty-parent-restored"), "{output}");
    }
}

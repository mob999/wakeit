//! Generate system-level Gateway startup bundles; never install services here.
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub struct ServiceOptions {
    pub executable: PathBuf,
    pub config: PathBuf,
    pub env_file: PathBuf,
    pub user: String,
    pub platform: String,
    pub nats_bin: Option<PathBuf>,
    pub nats_config: Option<PathBuf>,
    pub postgres_service: Option<String>,
}

/// Parse without changing the environment, or revealing bad lines in errors.
pub fn read_environment(path: &Path) -> Result<BTreeMap<String, String>> {
    let size = std::fs::metadata(path)
        .context("cannot read environment file")?
        .len();
    ensure!(size <= 64 * 1024, "environment file exceeds 64 KiB");
    let iter = dotenvy::from_path_iter(path)
        .map_err(|_| anyhow::anyhow!("cannot parse environment file"))?;
    let mut vars = BTreeMap::new();
    for entry in iter {
        let (name, value) = entry.map_err(|_| {
            anyhow::anyhow!("invalid environment file; values have not been logged")
        })?;
        ensure!(
            !value.contains('\0'),
            "environment file contains an invalid value"
        );
        vars.insert(name, value);
    }
    Ok(vars)
}

fn absolute(path: &Path) -> Result<String> {
    let path = path
        .canonicalize()
        .with_context(|| format!("path does not exist: {}", path.display()))?;
    let value = path
        .to_str()
        .context("service paths must be valid Unicode")?;
    ensure!(!value.contains(['\n', '\r', '\0']), "invalid service path");
    #[cfg(windows)]
    let value = value.strip_prefix("\\\\?\\").unwrap_or(value);
    Ok(value.into())
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn unit_arg(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    )
}
fn ps(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn generate(options: &ServiceOptions) -> Result<BTreeMap<String, String>> {
    ensure!(
        !options.user.is_empty() && !options.user.contains(['\n', '\r', '\0', '"', '\'']),
        "invalid service user"
    );
    if options.platform != "windows" {
        ensure!(
            options
                .user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
            "use a Unix account short name for --user"
        );
    }
    if let Some(service) = &options.postgres_service {
        ensure!(
            !service.is_empty()
                && service
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._@-".contains(&c)),
            "invalid PostgreSQL service name"
        );
    }
    let executable = absolute(&options.executable)?;
    let config = absolute(&options.config)?;
    let env = absolute(&options.env_file)?;
    read_environment(&options.env_file)?;
    let gateway: crate::config::GatewayConfig = crate::config::read(&options.config)?;
    ensure!(
        gateway.version == 1,
        "unsupported Gateway configuration version"
    );
    ensure!(
        read_environment(&options.env_file)?.contains_key(&gateway.database_url_env),
        "environment file must define the configured database URL variable"
    );
    let nats = match (&options.nats_bin, &options.nats_config) {
        (Some(bin), Some(config)) => Some((absolute(bin)?, absolute(config)?)),
        (None, None) => None,
        _ => bail!("--nats-bin and --nats-config must be provided together"),
    };
    let mut files = BTreeMap::new();
    match options.platform.as_str() {
        "linux" => {
            let mut dependencies = Vec::new();
            if nats.is_some() {
                dependencies.push("wakeit-nats.service".to_string());
            }
            if let Some(service) = &options.postgres_service {
                dependencies.push(service.clone());
            }
            let after = format!("network-online.target {}", dependencies.join(" "));
            let requires = if dependencies.is_empty() {
                String::new()
            } else {
                format!("Requires={}\n", dependencies.join(" "))
            };
            files.insert("wakeit-gateway.service".into(),format!(
                "[Unit]\nDescription=Wakeit Gateway\nWants=network-online.target\nAfter={after}\n{requires}StartLimitIntervalSec=0\n[Service]\nType=simple\nUser={}\nExecStart={} start --config {} --env-file {}\nRestart=on-failure\nRestartSec=5\nKillMode=control-group\nUMask=0077\n[Install]\nWantedBy=multi-user.target\n",
                options.user,unit_arg(&executable),unit_arg(&config),unit_arg(&env)));
            if let Some((bin, config)) = &nats {
                files.insert("wakeit-nats.service".into(),format!("[Unit]\nDescription=Wakeit NATS\nAfter=network-online.target\nStartLimitIntervalSec=0\n[Service]\nUser={}\nExecStart={} --config {}\nRestart=on-failure\nRestartSec=5\nUMask=0077\n[Install]\nWantedBy=multi-user.target\n",options.user,unit_arg(bin),unit_arg(config)));
            }
            let units = if nats.is_some() {
                "wakeit-nats.service wakeit-gateway.service"
            } else {
                "wakeit-gateway.service"
            };
            files.insert("install.sh".into(),format!("#!/bin/sh\nset -eu\n[ \"$(id -u)\" = 0 ] || {{ echo 'Run as root' >&2; exit 1; }}\ncd \"$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\"\nfor unit in {units}; do\n  [ ! -e \"/etc/systemd/system/$unit\" ] || {{ echo \"Service already installed: $unit\" >&2; exit 1; }}\ndone\nfor unit in {units}; do install -m 644 \"$unit\" \"/etc/systemd/system/$unit\"; done\nsystemctl daemon-reload\nsystemctl enable --now {units}\n"));
            files.insert("uninstall.sh".into(),format!("#!/bin/sh\nset -eu\n[ \"$(id -u)\" = 0 ] || {{ echo 'Run as root' >&2; exit 1; }}\nsystemctl disable --now {units}\nfor unit in {units}; do rm -f \"/etc/systemd/system/$unit\"; done\nsystemctl daemon-reload\n"));
        }
        "macos" => {
            fn plist(label: &str, user: &str, args: &[&str], logs: &str) -> String {
                let args = args
                    .iter()
                    .map(|a| format!("<string>{}</string>", xml(a)))
                    .collect::<String>();
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>UserName</key><string>{}</string><key>ProgramArguments</key><array>{args}</array><key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>ThrottleInterval</key><integer>5</integer><key>Umask</key><integer>63</integer><key>StandardOutPath</key><string>{logs}/{label}.log</string><key>StandardErrorPath</key><string>{logs}/{label}.error.log</string></dict></plist>\n",
                    xml(user)
                )
            }
            let logs = "/Library/Logs/Wakeit";
            files.insert(
                "dev.wakeit.gateway.plist".into(),
                plist(
                    "dev.wakeit.gateway",
                    &options.user,
                    &[
                        &executable,
                        "start",
                        "--config",
                        &config,
                        "--env-file",
                        &env,
                    ],
                    logs,
                ),
            );
            if let Some((bin, config)) = &nats {
                files.insert(
                    "dev.wakeit.nats.plist".into(),
                    plist(
                        "dev.wakeit.nats",
                        &options.user,
                        &[bin, "--config", config],
                        logs,
                    ),
                );
            }
            let labels = if nats.is_some() {
                "dev.wakeit.nats dev.wakeit.gateway"
            } else {
                "dev.wakeit.gateway"
            };
            // Resolve the account's primary group at installation time.
            files.insert("install.sh".into(),format!("#!/bin/sh\nset -eu\n[ \"$(id -u)\" = 0 ] || {{ echo 'Run as root' >&2; exit 1; }}\ncd \"$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\"\nfor label in {labels}; do\n  [ ! -e \"/Library/LaunchDaemons/$label.plist\" ] || {{ echo \"Service already installed: $label\" >&2; exit 1; }}\ndone\ninstall -d -m 750 -o '{}' -g \"$(id -gn '{}')\" /Library/Logs/Wakeit\nfor label in {labels}; do\n  install -m 644 -o root -g wheel \"$label.plist\" \"/Library/LaunchDaemons/$label.plist\"\n  launchctl bootstrap system \"/Library/LaunchDaemons/$label.plist\"\ndone\n",options.user,options.user));
            files.insert("uninstall.sh".into(),format!("#!/bin/sh\nset -eu\n[ \"$(id -u)\" = 0 ] || {{ echo 'Run as root' >&2; exit 1; }}\nfor label in dev.wakeit.gateway {}; do\n  if [ -e \"/Library/LaunchDaemons/$label.plist\" ]; then\n    launchctl bootout \"system/$label\"\n    rm -f \"/Library/LaunchDaemons/$label.plist\"\n  fi\ndone\n",if nats.is_some() { "dev.wakeit.nats" } else { "" }));
        }
        "windows" => {
            ensure!(
                matches!(
                    options.user.as_str(),
                    "LocalService" | "NetworkService" | "LocalSystem"
                ),
                "Windows Gateway service uses LocalService, NetworkService or LocalSystem; Runner uses its own user identity"
            );
            // WinSW expands %NAME% in XML. Reject percent-bearing paths rather
            // than silently pointing a service at a different executable.
            ensure!(
                ![&executable, &config, &env].iter().any(|p| p.contains('%'))
                    && nats
                        .as_ref()
                        .is_none_or(|(b, c)| !b.contains('%') && !c.contains('%')),
                "Windows service paths cannot contain percent signs"
            );
            fn wrapper(
                id: &str,
                user: &str,
                executable: &str,
                args: &str,
                dependencies: &[String],
            ) -> String {
                let depends = dependencies
                    .iter()
                    .map(|d| format!("<depend>{}</depend>", xml(d)))
                    .collect::<String>();
                let domain = if user == "LocalSystem" {
                    ""
                } else {
                    "<domain>NT AUTHORITY</domain>"
                };
                format!(
                    "<service><id>{id}</id><name>{id}</name><executable>{}</executable><arguments>{}</arguments><startmode>Automatic</startmode><delayedAutoStart>true</delayedAutoStart><onfailure action=\"restart\" delay=\"5 sec\"/><stoptimeout>30 sec</stoptimeout><log mode=\"roll\"/><serviceaccount>{domain}<user>{user}</user></serviceaccount>{depends}</service>\n",
                    xml(executable),
                    xml(args)
                )
            }
            let mut dependencies = Vec::new();
            if nats.is_some() {
                dependencies.push("wakeit-nats".into());
            }
            if let Some(service) = &options.postgres_service {
                dependencies.push(service.clone());
            }
            files.insert(
                "wakeit-gateway.xml".into(),
                wrapper(
                    "wakeit-gateway",
                    &options.user,
                    &executable,
                    &format!("start --config \"{config}\" --env-file \"{env}\""),
                    &dependencies,
                ),
            );
            if let Some((bin, config)) = &nats {
                files.insert(
                    "wakeit-nats.xml".into(),
                    wrapper(
                        "wakeit-nats",
                        &options.user,
                        bin,
                        &format!("--config \"{config}\""),
                        &[],
                    ),
                );
            }
            let names = if nats.is_some() {
                "@('wakeit-nats', 'wakeit-gateway')"
            } else {
                "@('wakeit-gateway')"
            };
            let perms = format!(
                "& icacls {} /inheritance:r /grant:r 'Administrators:F' 'SYSTEM:F' {}\nif ($LASTEXITCODE -ne 0) {{ throw 'Cannot restrict environment file permissions' }}\n",
                ps(&env),
                ps(&format!(
                    "{}:R",
                    if options.user == "LocalSystem" {
                        "SYSTEM"
                    } else if options.user == "LocalService" {
                        "NT AUTHORITY\\LOCAL SERVICE"
                    } else {
                        "NT AUTHORITY\\NETWORK SERVICE"
                    }
                ))
            );
            files.insert("install.ps1".into(),format!("#Requires -RunAsAdministrator\n$ErrorActionPreference = 'Stop'\nSet-Location $PSScriptRoot\n$names = {names}\nforeach ($name in $names) {{ if (Get-Service $name -ErrorAction SilentlyContinue) {{ throw \"Service already installed: $name\" }} }}\n$wrapper = Join-Path $PSScriptRoot 'WinSW-x64.exe'\nif (-not (Test-Path $wrapper)) {{ Invoke-WebRequest 'https://github.com/winsw/winsw/releases/download/v2.12.0/WinSW-x64.exe' -OutFile $wrapper }}\nif ((Get-FileHash $wrapper -Algorithm SHA256).Hash -ne '05B82D46AD331CC16BDC00DE5C6332C1EF818DF8CEEFCD49C726553209B3A0DA') {{ throw 'WinSW checksum mismatch' }}\n{perms}foreach ($name in $names) {{\n  Copy-Item $wrapper \"$name.exe\"\n  & \".\\$name.exe\" install\n  if ($LASTEXITCODE -ne 0) {{ throw \"Could not install $name\" }}\n  & \".\\$name.exe\" start\n  if ($LASTEXITCODE -ne 0) {{ throw \"Could not start $name\" }}\n}}\n"));
            files.insert("uninstall.ps1".into(),format!("#Requires -RunAsAdministrator\n$ErrorActionPreference = 'Stop'\nSet-Location $PSScriptRoot\n$names = {names}\n[array]::Reverse($names)\nforeach ($name in $names) {{\n  & \".\\$name.exe\" stop\n  if ($LASTEXITCODE -ne 0) {{ throw \"Could not stop $name\" }}\n  & \".\\$name.exe\" uninstall\n  if ($LASTEXITCODE -ne 0) {{ throw \"Could not uninstall $name\" }}\n}}\n"));
        }
        _ => bail!("platform must be linux, macos or windows"),
    }
    Ok(files)
}

pub fn write_bundle(output: &Path, files: &BTreeMap<String, String>) -> Result<()> {
    ensure!(
        !output.exists() || output.read_dir()?.next().is_none(),
        "output directory must be empty; existing service files were not changed"
    );
    std::fs::create_dir_all(output)?;
    for (name, content) in files {
        let path = output.join(name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        std::io::Write::write_all(&mut file, content.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &path,
                std::fs::Permissions::from_mode(if name.ends_with(".sh") { 0o755 } else { 0o644 }),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(directory: &Path, platform: &str) -> ServiceOptions {
        let config = directory.join("网关 config.yaml");
        std::fs::write(&config,"version: 1\nlisten: 127.0.0.1:8080\npublic_url: http://localhost:8080\ndatabase_url_env: WAKEIT_DATABASE_URL\nnats_url: nats://localhost:4222\nnats_public_url: ws://localhost:8081\nnats_auth_dir: ./auth\nnats_replicas: 1\nlog_limit_bytes: 20971520\n").unwrap();
        let env = directory.join("gateway.env");
        std::fs::write(
            &env,
            "WAKEIT_DATABASE_URL='postgres://user:secret@localhost/db'\n",
        )
        .unwrap();
        ServiceOptions {
            executable: std::env::current_exe().unwrap(),
            config,
            env_file: env,
            user: if platform == "windows" {
                "LocalService".into()
            } else {
                "wakeit".into()
            },
            platform: platform.into(),
            nats_bin: Some(std::env::current_exe().unwrap()),
            nats_config: Some(directory.join("网关 config.yaml")),
            postgres_service: Some("postgresql".into()),
        }
    }
    #[test]
    fn bundles_are_system_startup_and_never_contain_credentials() {
        let temp = tempfile::tempdir().unwrap();
        for platform in ["linux", "macos", "windows"] {
            let files = generate(&options(temp.path(), platform)).unwrap();
            assert!(
                files
                    .values()
                    .all(|value| !value.contains("postgres://") && !value.contains("secret"))
            );
            match platform {
                "linux" => {
                    assert!(files["wakeit-gateway.service"].contains("WantedBy=multi-user.target"))
                }
                "macos" => assert!(files["install.sh"].contains("launchctl bootstrap system")),
                _ => {
                    assert!(
                        files["wakeit-gateway.xml"].contains("<startmode>Automatic</startmode>")
                    );
                    assert!(files["wakeit-gateway.xml"].contains("<depend>wakeit-nats</depend>"));
                }
            }
        }
    }
    #[test]
    fn bad_environment_lines_are_redacted_and_duplicates_are_last_wins() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("env");
        std::fs::write(&path, "A=one\nA='two words'\n").unwrap();
        assert_eq!(read_environment(&path).unwrap()["A"], "two words");
        std::fs::write(&path, "PASSWORD='secret\n").unwrap();
        assert!(
            !read_environment(&path)
                .unwrap_err()
                .to_string()
                .contains("secret")
        );
    }
    #[test]
    fn existing_bundle_is_not_overwritten_and_bad_names_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let mut opts = options(temp.path(), "linux");
        let output = temp.path().join("bundle");
        let files = generate(&opts).unwrap();
        write_bundle(&output, &files).unwrap();
        assert!(write_bundle(&output, &files).is_err());
        opts.postgres_service = Some("postgres\nExecStart=evil".into());
        assert!(generate(&opts).is_err());
    }
    #[test]
    fn systemd_arguments_preserve_literal_percent_dollar_quotes_and_spaces() {
        assert_eq!(unit_arg("/tmp/a b%$\""), "\"/tmp/a b%%$$\\\"\"");
    }
}

# Install the Wakeit CLI

[中文](install-zh.md)

On macOS or Linux with GNU libc, run:

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh
```

The CLI includes the Runner and is released independently of Gateway. The script detects the system and architecture, downloads CLI 0.1.0 by default (or the version selected with `--version`), verifies SHA-256 against its release manifest and checksum list, and installs into `~/.local/bin`. Progress is shown for each stage and the download. It configures PATH for the current user's shell; open a new terminal and run `wakeit --version` after installation. No sudo is needed. Existing configuration, service processes, device identity and Agent credentials are retained.

To install a specific release or choose a different directory:

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh -s -- --version 0.1.0 --bin-dir "$HOME/.local/bin"
```

Use `--no-modify-path` to manage PATH yourself. The installer supports zsh, bash, fish and POSIX shell startup files. For another shell, it prints the directory to add. The installer does not install or update Gateway and does not restart a running Runner; follow the [update guide](../skills/wakeit/references/updates.md) when upgrading services.

Agents can also pass `--skill-dir /path/to/agent/skills/wakeit` to install the entire matching Wakeit skill. Choose the Agent's actual configured skill directory. An identical existing skill is reused; different files are retained and reported instead of being overwritten. Reload the Agent if it requires that to discover installed skills.

To install only the Skill without downloading or replacing the CLI or changing PATH:

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh -s -- --version 0.1.0 --skill-only --skill-dir "/absolute/path/to/agent/skills/wakeit"
```

The installer is also included as `install.sh` in CLI release assets, alongside `SHA256SUMS`. It can be downloaded and inspected before running.

## Manual installation and Windows

For Windows or a manual installation, choose a `cli-v*` release from the [official releases](https://github.com/mob999/wakeit/releases). Download `wakeit-cli-release.json`, `SHA256SUMS`, and the binary for your platform. Compare the binary's SHA-256 with both release files before putting it in a persistent user bin directory on PATH.

| System | Asset |
| --- | --- |
| macOS Intel | `wakeit-x86_64-apple-darwin` |
| macOS Apple Silicon | `wakeit-aarch64-apple-darwin` |
| Linux GNU x86-64 | `wakeit-x86_64-unknown-linux-gnu` |
| Linux GNU ARM64 | `wakeit-aarch64-unknown-linux-gnu` |
| Windows x86-64 | `wakeit-x86_64-pc-windows-msvc.exe` |

On Windows, use `Get-FileHash -Algorithm SHA256` to calculate the checksum, rename the verified binary to `wakeit.exe`, and add its directory to the current user's PATH. Verify `wakeit --version` in a new shell. Do not run the Unix shell installer in native PowerShell.

A checksum or version failure stops installation before replacing the existing executable. Resolve the reported problem and rerun the same command; download files are temporary and no service data is removed.

To host a service, use the separate [Gateway deployment guide](gateway-service.md). Gateway versions are not valid CLI version selectors.

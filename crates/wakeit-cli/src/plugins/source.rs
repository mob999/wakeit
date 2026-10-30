use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Git {
        url: String,
        repository: String,
        reference: Option<String>,
    },
    Local {
        path: PathBuf,
    },
}
impl Source {
    pub fn parse(value: &str, base: &Path) -> Result<Self> {
        ensure!(!value.trim().is_empty(), "plugin source is required");
        ensure!(
            !value.starts_with("npm:"),
            "npm plugin sources are not supported; use Git or a local directory"
        );
        let explicit_git = value.starts_with("git:");
        let value = value.strip_prefix("git:").unwrap_or(value);
        let git = explicit_git
            || value.starts_with("https://")
            || value.starts_with("http://")
            || value.starts_with("ssh://")
            || value.starts_with("git@")
            || (!value.starts_with('.')
                && !Path::new(value).is_absolute()
                && value.split('/').next().is_some_and(|h| h.contains('.')));
        if !git {
            let path = if let Some(rest) = value.strip_prefix("~/") {
                PathBuf::from(std::env::var_os("HOME").context("HOME is unavailable")?).join(rest)
            } else {
                base.join(value)
            };
            let mut absolute = PathBuf::new();
            for part in path.components() {
                match part {
                    std::path::Component::CurDir => {}
                    std::path::Component::ParentDir => {
                        if absolute.file_name().is_some() {
                            absolute.pop();
                        }
                    }
                    _ => absolute.push(part.as_os_str()),
                }
            }
            return Ok(Self::Local { path: absolute });
        }
        let path_start = if value.starts_with("git@") {
            value.find(':').unwrap_or(value.len())
        } else {
            let authority = value.find("://").map(|n| n + 3).unwrap_or(0);
            value[authority..]
                .find('/')
                .map(|n| n + authority)
                .unwrap_or(value.len())
        };
        let (address, reference) = match value.rfind('@') {
            Some(i) if i > path_start => (&value[..i], Some(value[i + 1..].to_owned())),
            _ => (value, None),
        };
        ensure!(
            reference.as_ref().is_none_or(|r| !r.is_empty()
                && !r.starts_with('-')
                && !r.chars().any(char::is_control)),
            "invalid Git ref"
        );
        let url = if address.starts_with("git@") || address.contains("://") {
            address.to_owned()
        } else {
            format!("https://{address}")
        };
        let (host, path) = if let Some(ssh) = address.strip_prefix("git@") {
            let (host, path) = ssh
                .split_once(':')
                .context("expected git@host:owner/repository")?;
            (host.to_lowercase(), path.to_owned())
        } else {
            let parsed = reqwest::Url::parse(&url).context("invalid Git repository URL")?;
            ensure!(
                matches!(parsed.scheme(), "http" | "https" | "ssh"),
                "unsupported Git scheme"
            );
            ensure!(
                (parsed.scheme() == "ssh" || parsed.username().is_empty())
                    && parsed.password().is_none()
                    && parsed.query().is_none()
                    && parsed.fragment().is_none(),
                "use Git credentials and @ref rather than credentials, query or fragment in a plugin URL"
            );
            let host = parsed.host_str().context("Git host is required")?;
            (
                format!(
                    "{}{}",
                    host.to_lowercase(),
                    parsed.port().map(|p| format!(":{p}")).unwrap_or_default()
                ),
                parsed.path().trim_start_matches('/').to_owned(),
            )
        };
        let path = path.trim_end_matches('/').trim_end_matches(".git");
        ensure!(
            path.contains('/')
                && !path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".."),
            "Git source needs an owner/repository path"
        );
        Ok(Self::Git {
            url,
            repository: format!("{host}/{path}"),
            reference,
        })
    }
    pub fn identity(&self) -> String {
        match self {
            Self::Git { repository, .. } => format!("git:{repository}"),
            Self::Local { path } => format!("local:{}", path.display()),
        }
    }
    pub fn display(&self) -> String {
        match self {
            Self::Git { url, reference, .. } => format!(
                "{url}{}",
                reference
                    .as_ref()
                    .map(|r| format!("@{r}"))
                    .unwrap_or_default()
            ),
            Self::Local { path } => path.to_string_lossy().into(),
        }
    }
    pub fn git(&self) -> Result<(&str, Option<&str>)> {
        match self {
            Self::Git { url, reference, .. } => Ok((url, reference.as_deref())),
            _ => bail!("not a Git source"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn git_transport_and_ref_do_not_change_package_identity() {
        let sources = [
            "https://github.com/example/agent.git@v1.0.0",
            "git:github.com/example/agent@feature/new",
            "git@github.com:example/agent.git",
            "ssh://git@github.com/example/agent",
        ];
        for value in sources {
            let source = Source::parse(value, Path::new("/")).unwrap();
            assert_eq!(source.identity(), "git:github.com/example/agent");
        }
        let source = Source::parse(sources[1], Path::new("/")).unwrap();
        assert_eq!(source.git().unwrap().1, Some("feature/new"));
    }
    #[test]
    fn local_identity_does_not_require_the_package_to_still_exist() {
        let base = std::env::current_dir().unwrap();
        let source = Source::parse("./removed/../package", &base).unwrap();
        assert_eq!(
            source,
            Source::Local {
                path: base.join("package")
            }
        );
    }
}

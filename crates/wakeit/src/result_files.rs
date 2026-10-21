//! Result-scoped, bounded file previews. No shell, file catalog or persisted content.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use pulldown_cmark::{Options, Parser};
use scraper::{Html, Selector};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub const MAX_BYTES: usize = 512 * 1024;

pub fn report_text(result: &Value) -> Option<&str> {
    result.as_str().or_else(|| {
        (result["truncated"] == true)
            .then(|| result["preview"].as_str())
            .flatten()
    })
}
fn link_key(href: &str) -> std::borrow::Cow<'_, str> {
    percent_encoding::percent_decode_str(href.split(['#', '?']).next().unwrap_or(""))
        .decode_utf8_lossy()
}
pub fn linked(text: &str, href: &str) -> bool {
    let mut rendered = String::new();
    // Parsing to HTML preserves CommonMark's link/code distinction; HTML parsing
    // keeps comments, raw script text and escaped anchors out of the link set.
    pulldown_cmark::html::push_html(
        &mut rendered,
        Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH),
    );
    Html::parse_fragment(&rendered)
        .select(&Selector::parse("a[href]").expect("static selector"))
        .any(|a| {
            a.value()
                .attr("href")
                .is_some_and(|value| link_key(value) == link_key(href))
                && !a
                    .ancestors()
                    .filter_map(scraper::ElementRef::wrap)
                    .any(|e| {
                        matches!(
                            e.value().name(),
                            "script" | "style" | "iframe" | "object" | "template"
                        )
                    })
        })
}

pub fn local_link(href: &str) -> bool {
    if href.is_empty() || href.starts_with('#') || href.starts_with("//") {
        return false;
    }
    let lower = href.to_ascii_lowercase();
    if lower.starts_with("file:") {
        return true;
    }
    if href.as_bytes().get(1) == Some(&b':') && href.as_bytes()[0].is_ascii_alphabetic() {
        return true;
    }
    if href
        .split(['/', '\\', '?', '#'])
        .next()
        .is_some_and(|part| part.contains(':'))
    {
        return false;
    }
    ![
        "/",
        "/w",
        "/tasks",
        "/runs",
        "/runners",
        "/settings",
        "/login",
        "/authorize",
        "/api",
        "/assets",
    ]
    .iter()
    .any(|route| {
        href == *route
            || (*route != "/" && href.starts_with(&format!("{route}/")))
            || href.starts_with(&format!("{route}?"))
    })
}

pub fn path(href: &str, cwd: Option<&str>) -> Result<PathBuf> {
    ensure!(
        href.len() <= 8192 && !href.chars().any(char::is_control),
        "invalid file link"
    );
    ensure!(local_link(href), "not a local file link");
    let raw = if href.to_ascii_lowercase().starts_with("file:") {
        let rest = href
            .get(7..)
            .filter(|_| href[..7].eq_ignore_ascii_case("file://"))
            .context("file link requires file:///")?;
        ensure!(
            rest.starts_with('/') && !rest.starts_with("//"),
            "remote file hosts are not supported"
        );
        rest
    } else {
        href
    };
    let raw = raw.split(['#', '?']).next().unwrap_or("");
    let decoded = percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .context("invalid UTF-8 file link")?;
    ensure!(!decoded.chars().any(char::is_control), "invalid file link");
    let mut value = decoded.as_ref();
    if cfg!(windows) && value.starts_with('/') && value.as_bytes().get(2) == Some(&b':') {
        value = &value[1..];
    }
    let drive = value.as_bytes().get(1) == Some(&b':') && value.as_bytes()[0].is_ascii_alphabetic();
    ensure!(
        !value.starts_with("//") && !value.starts_with("\\\\"),
        "remote file paths are not supported"
    );
    if drive {
        ensure!(
            cfg!(windows) && matches!(value.as_bytes().get(2), Some(b'/') | Some(b'\\')),
            "file path does not match Runner OS"
        );
    }
    if cfg!(windows) {
        ensure!(
            !value[if drive { 2 } else { 0 }..].contains(':'),
            "alternate file streams are not supported"
        );
    } else {
        ensure!(!value.contains('\\'), "file path does not match Runner OS");
    }
    let p = Path::new(value);
    let absolute = if p.is_absolute() {
        p.to_owned()
    } else {
        let cwd = Path::new(cwd.context("execution working directory is unavailable")?);
        ensure!(
            cwd.is_absolute(),
            "execution working directory is unavailable"
        );
        cwd.join(p)
    };
    Ok(absolute)
}

pub fn read(roots: &[PathBuf], href: &str, cwd: Option<&str>, limit: usize) -> Result<Value> {
    let requested = path(href, cwd)?;
    let real = requested
        .canonicalize()
        .context("file does not exist or cannot be accessed")?;
    let root = roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .filter(|r| real.starts_with(r))
        .max_by_key(|r| r.components().count())
        .context("file is outside allowed_roots")?;
    let dir = Dir::open_ambient_dir(&root, ambient_authority())?;
    let relative = real.strip_prefix(&root)?;
    ensure!(
        dir.metadata(relative)?.is_file(),
        "only regular files can be previewed"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    // Capability-relative open prevents intermediate/final symlink races from
    // escaping the already-open allowed directory. Inspect the opened handle.
    let mut file = dir.open_with(relative, &options)?.into_std();
    let before = file.metadata()?;
    ensure!(before.is_file(), "only regular files can be previewed");
    let limit = limit.min(MAX_BYTES);
    ensure!(
        limit >= 4096,
        "Runner message limit is too small for file previews"
    );
    let mut bytes = Vec::new();
    (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    ensure!(
        before.len() == after.len() && before.modified().ok() == after.modified().ok(),
        "file changed while reading; try again"
    );
    let truncated = bytes.len() > limit || before.len() > limit as u64;
    bytes.truncate(limit);
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    };
    let extension = real
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (format, content, mime) = if let Some(mime) = mime {
        ensure!(!truncated, "image exceeds file preview limit");
        ("image", STANDARD.encode(&bytes), mime)
    } else {
        let text = match std::str::from_utf8(&bytes) {
            Ok(s) => s,
            Err(e) if truncated && e.error_len().is_none() => {
                std::str::from_utf8(&bytes[..e.valid_up_to()])?
            }
            Err(_) => {
                anyhow::bail!("file format is not supported; UTF-8 text or PNG/JPEG/WebP required")
            }
        };
        ensure!(!text.contains('\0'), "file format is not supported");
        let format = match extension.as_str() {
            "md" | "markdown" => "markdown",
            "json" => "json",
            _ => "text",
        };
        (format, text.to_owned(), "text/plain")
    };
    Ok(
        json!({"path":real,"name":real.file_name().and_then(|s|s.to_str()).unwrap_or("file"),"format":format,"mime":mime,"content":content,"size":before.len(),"modified_at":before.modified().ok().map(chrono::DateTime::<chrono::Utc>::from),"truncated":truncated}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_authored_links_only() {
        let text = "[ok](./report.md)\n`[no](/private)`\n\n```html\n<a href='/hidden'>hidden</a>\n```\n<a href='/also?x=1&amp;y=2'>yes</a>\n<script>\"<a href='/script'>\"</script>";
        assert!(linked(text, "./report.md"));
        assert!(linked("[文件](./你好.md)", "./%E4%BD%A0%E5%A5%BD.md"));
        assert!(!linked("[文件](/a%23b)", "/a#b"));
        assert!(linked(text, "/also?x=1&y=2"));
        for path in ["/private", "/hidden", "/script", "report.md"] {
            assert!(!linked(text, path), "{path}");
        }
    }
    #[test]
    fn uri_and_route_boundaries() {
        for href in [
            "https://example.com/a",
            "#part",
            "//host/a",
            "/runs?run=x",
            "/w/id/tasks",
            "javascript:alert(1)",
        ] {
            assert!(!local_link(href), "{href}");
        }
        for href in [
            "file:///workspace/a%20b.md",
            "/User/example/a",
            "./a.md",
            "../a.md",
            "a.md",
            "C:/data/a.md",
        ] {
            assert!(local_link(href), "{href}");
        }
        assert!(path("file://host/etc/passwd", Some("/workspace")).is_err());
        assert!(path("./a", None).is_err());
        assert!(path("/tmp/a%00b", None).is_err());
        #[cfg(unix)]
        {
            assert_eq!(
                path("a%2520b%23c.md#L1", Some("/workspace")).unwrap(),
                PathBuf::from("/workspace/a%20b#c.md")
            );
        }
    }
    #[test]
    fn bounded_read_and_current_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file = root.path().join("report.md");
        std::fs::write(&file, "x".repeat(7000)).unwrap();
        let value = read(
            &[root.path().to_owned()],
            file.to_str().unwrap(),
            None,
            4096,
        )
        .unwrap();
        assert_eq!(value["content"].as_str().unwrap().len(), 4096);
        assert_eq!(value["truncated"], true);
        let secret = outside.path().join("hidden");
        std::fs::write(&secret, "private").unwrap();
        assert!(
            read(
                &[root.path().to_owned()],
                secret.to_str().unwrap(),
                None,
                4096
            )
            .is_err()
        );
        assert!(
            read(
                &[root.path().to_owned()],
                root.path().to_str().unwrap(),
                None,
                4096
            )
            .is_err()
        );
        #[cfg(unix)]
        {
            let link = root.path().join("link");
            std::os::unix::fs::symlink(&secret, &link).unwrap();
            assert!(
                read(
                    &[root.path().to_owned()],
                    link.to_str().unwrap(),
                    None,
                    4096
                )
                .is_err()
            );
        }
    }
}

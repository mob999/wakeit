use crate::releases::{Github, MAX_METADATA, Release, verify_checksum};
use anyhow::{Context, Result, ensure};
use flate2::read::GzDecoder;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

const ARCHIVE: &str = "wakeit-skill.tar.gz";
const MAX_ARCHIVE: usize = 16 * 1024 * 1024;

async fn asset(github: &Github, release: &Release, name: &str, limit: usize) -> Result<Vec<u8>> {
    let entry = release
        .assets
        .iter()
        .find(|entry| entry.name == name)
        .with_context(|| format!("release is missing {name}"))?;
    ensure!(entry.size <= limit, "release asset exceeds size limit");
    let bytes = github.get(&entry.url, true, limit).await?;
    ensure!(
        bytes.len() == entry.size,
        "release asset size differs from metadata"
    );
    Ok(bytes)
}

async fn download(github: &Github, release: &Release) -> Result<Vec<u8>> {
    let checksums = asset(github, release, "SHA256SUMS", MAX_METADATA).await?;
    let checksums = std::str::from_utf8(&checksums).context("invalid release checksum list")?;
    let mut entries = checksums.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        let hash = fields.next()?;
        (fields.next()? == ARCHIVE).then_some(hash)
    });
    let hash = entries
        .next()
        .context("release is missing the Skill checksum")?;
    ensure!(entries.next().is_none(), "duplicate Skill checksum");
    let bytes = asset(github, release, ARCHIVE, MAX_ARCHIVE).await?;
    verify_checksum(&bytes, hash)?;
    Ok(bytes)
}

fn replace(bytes: &[u8], destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("Skill directory needs a parent")?;
    fs::create_dir_all(parent)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join(".wakeit-skill-install.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("another Skill installation is in progress")?;
    let existing = match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Skill destination must be a directory, not a file or symbolic link"
            );
            if fs::read_dir(destination)?.next().transpose()?.is_some() {
                let entry = fs::read_to_string(destination.join("SKILL.md")).context(
                    "destination is not an installed Wakeit Skill; choose another --dir",
                )?;
                ensure!(
                    entry.lines().any(|line| line.trim() == "name: wakeit"),
                    "destination belongs to another Skill; choose another --dir"
                );
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    let stage = tempfile::Builder::new()
        .prefix(".wakeit-skill-")
        .tempdir_in(parent)?;
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let mut total = 0u64;
    for entry in archive.entries().context("invalid Skill archive")? {
        let mut entry = entry?;
        let path = entry.path()?;
        let mut components = path.components();
        ensure!(
            components.next() == Some(Component::Normal("wakeit".as_ref()))
                && components.all(|part| matches!(part, Component::Normal(_))),
            "unsafe Skill archive path"
        );
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_file() || kind.is_dir(),
            "Skill archive contains links or special files"
        );
        ensure!(
            entry.size() <= 64 * 1024 * 1024 - total,
            "unpacked Skill exceeds size limit"
        );
        total += entry.size();
        ensure!(entry.unpack_in(stage.path())?, "unsafe Skill archive path");
    }
    let unpacked = stage.path().join("wakeit");
    ensure!(
        unpacked.join("SKILL.md").is_file(),
        "Skill archive is missing SKILL.md"
    );
    let previous = stage.path().join("previous");
    if existing {
        fs::rename(destination, &previous).context("could not replace the installed Skill")?;
    }
    if let Err(error) = fs::rename(unpacked, destination) {
        if existing && let Err(restore) = fs::rename(&previous, destination) {
            let saved = stage.keep();
            anyhow::bail!(
                "Skill installation failed: {error}; restore failed: {restore}; previous Skill retained in {}",
                saved.join("previous").display()
            );
        }
        return Err(error).context("could not install the Skill; previous installation retained");
    }
    Ok(())
}

pub(crate) async fn install(dir: Option<&Path>, json: bool) -> Result<()> {
    let destination = match dir {
        Some(path) => std::path::absolute(path)?,
        None => {
            let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
            PathBuf::from(std::env::var_os(home).context("home directory unavailable; use --dir")?)
                .join(".agents/skills/wakeit")
        }
    };
    if !json {
        eprintln!("[1/3] Finding the latest Wakeit Skill");
    }
    let github = Github::new().await?;
    let (version, release) = github
        .releases()
        .await?
        .context("no official CLI release is available")?;
    if !json {
        eprintln!("[2/3] Downloading and verifying Skill {version}");
    }
    let bytes = download(&github, &release).await?;
    if !json {
        eprintln!("[3/3] Installing the Skill");
    }
    replace(&bytes, &destination)?;
    if json {
        println!(
            "{}",
            serde_json::json!({"status":"installed", "version":version.to_string(), "path":destination})
        );
    } else {
        println!(
            "Wakeit Skill {version} installed at {}. Reload your Agent to use it.",
            destination.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};

    fn archive(files: &[(&str, &str)]) -> Vec<u8> {
        let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        for (path, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive
                .append_data(&mut header, path, content.as_bytes())
                .unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn installs_and_replaces_the_complete_skill_without_touching_neighbors() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("custom path");
        fs::write(root.path().join("another-skill"), "untouched").unwrap();
        replace(
            &archive(&[
                ("wakeit/SKILL.md", "---\nname: wakeit\n---"),
                ("wakeit/old.md", "old"),
            ]),
            &destination,
        )
        .unwrap();
        replace(
            &archive(&[
                ("wakeit/SKILL.md", "---\nname: wakeit\n---\nUpdated"),
                ("wakeit/references/connect.md", "new"),
            ]),
            &destination,
        )
        .unwrap();
        assert!(!destination.join("old.md").exists());
        assert_eq!(
            fs::read_to_string(destination.join("references/connect.md")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("another-skill")).unwrap(),
            "untouched"
        );
        for bad in [
            archive(&[("outside.txt", "bad")]),
            archive(&[("wakeit/missing.md", "bad")]),
        ] {
            assert!(replace(&bad, &destination).is_err());
            assert!(
                fs::read_to_string(destination.join("SKILL.md"))
                    .unwrap()
                    .ends_with("Updated")
            );
        }
    }

    #[test]
    fn does_not_replace_an_unrelated_directory() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("other");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("SKILL.md"), "name: other").unwrap();
        assert!(
            replace(
                &archive(&[("wakeit/SKILL.md", "name: wakeit")]),
                &destination
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "name: other"
        );
    }
}

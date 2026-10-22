//! Result-scoped image thumbnails and revision-checked original chunks.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::Metadata,
    io::{BufReader, Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
};

pub const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PIXELS: u64 = 40_000_000;
const EDGE: u32 = 1280;
#[derive(Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Thumbnail,
    Original,
}
#[derive(Default, Deserialize)]
pub struct Request {
    pub image: Option<Mode>,
    #[serde(default)]
    pub offset: u64,
    pub revision: Option<String>,
}
fn revision(_file: &std::fs::File, meta: &Metadata) -> Result<String> {
    let mut stamp = format!("{}:{:?}", meta.len(), meta.modified().ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        stamp.push_str(&format!(
            ":{}:{}:{}:{}",
            meta.dev(),
            meta.ino(),
            meta.ctime(),
            meta.ctime_nsec()
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandle,
            GetFileInformationByHandleEx,
        };
        let mut identity: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        let mut basic: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
        let handle = _file.as_raw_handle();
        let ok = unsafe {
            GetFileInformationByHandle(handle, &mut identity) != 0
                && GetFileInformationByHandleEx(
                    handle,
                    FileBasicInfo,
                    (&mut basic as *mut FILE_BASIC_INFO).cast(),
                    std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                ) != 0
        };
        ensure!(ok, "image revision is unavailable");
        stamp.push_str(&format!(
            ":{}:{}:{}:{}",
            identity.dwVolumeSerialNumber,
            identity.nFileIndexHigh,
            identity.nFileIndexLow,
            basic.ChangeTime
        ));
    }
    Ok(hex::encode(Sha256::digest(stamp)))
}
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(192 * 1024 * 1024);
    limits
}
fn dimensions(width: u32, height: u32) -> Result<()> {
    ensure!(
        width > 0
            && height > 0
            && width <= 16384
            && height <= 16384
            && u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "image dimensions exceed preview limit"
    );
    Ok(())
}
pub fn read(
    roots: &[PathBuf],
    href: &str,
    cwd: Option<&str>,
    limit: usize,
    request: Request,
) -> Result<Value> {
    let Some(mode) = request.image else {
        return crate::result_files::read(roots, href, cwd, limit);
    };
    let limit = limit.min(crate::result_files::MAX_BYTES);
    ensure!(
        limit >= 4096,
        "Runner message limit is too small for file previews"
    );
    let (mut file, real) = crate::result_files::open(roots, href, cwd)?;
    let before = file.metadata()?;
    let stamp = revision(&file, &before)?;
    let mut head = [0; 16];
    let count = file.read(&mut head)?;
    file.rewind()?;
    let format = image::guess_format(&head[..count])
        .ok()
        .filter(|f| matches!(f, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP));
    let Some(format) = format else {
        ensure!(
            matches!(mode, Mode::Thumbnail),
            "referenced file is not a supported image"
        );
        return crate::result_files::read(roots, href, cwd, limit);
    };
    ensure!(
        before.len() <= MAX_IMAGE_BYTES,
        "image exceeds 32 MiB limit"
    );
    ensure!(
        request
            .revision
            .as_ref()
            .is_none_or(|expected| expected == &stamp),
        "image changed; reload the preview"
    );
    let reader = ImageReader::with_format(BufReader::new(&mut file), format);
    let (width, height) = reader
        .into_dimensions()
        .context("image cannot be decoded")?;
    dimensions(width, height)?;
    file.rewind()?;
    let mut value = json!({"path":real,"name":real.file_name().and_then(|s|s.to_str()).unwrap_or("image"),"format":"image","mime":format.to_mime_type(),"size":before.len(),"modified_at":before.modified().ok().map(chrono::DateTime::<chrono::Utc>::from),"truncated":false,"image":mode,"revision":stamp});
    match mode {
        Mode::Thumbnail => {
            ensure!(request.offset == 0, "invalid image offset");
            let mut reader = ImageReader::with_format(BufReader::new(&mut file), format);
            reader.limits(limits());
            let mut decoder = reader.into_decoder().context("image cannot be decoded")?;
            let orientation = decoder.orientation().context("image cannot be decoded")?;
            let mut image =
                DynamicImage::from_decoder(decoder).context("image cannot be decoded")?;
            image.apply_orientation(orientation);
            let mut thumb = image.thumbnail(EDGE, EDGE);
            // Preserve transparency; resize further rather than exceed a NATS response.
            let alpha = thumb.color().has_alpha();
            let output = if alpha {
                ImageFormat::Png
            } else {
                ImageFormat::Jpeg
            };
            let encoded = loop {
                let mut buffer = Cursor::new(Vec::new());
                if alpha {
                    thumb.write_to(&mut buffer, output)?;
                } else {
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, 82)
                        .encode_image(&thumb.to_rgb8())?;
                }
                let bytes = buffer.into_inner();
                if bytes.len() <= limit {
                    break bytes;
                }
                ensure!(
                    thumb.width() > 1 || thumb.height() > 1,
                    "thumbnail exceeds transport limit"
                );
                thumb = thumb.thumbnail(
                    (thumb.width() * 3 / 4).max(1),
                    (thumb.height() * 3 / 4).max(1),
                );
            };
            value["content"] = json!(STANDARD.encode(encoded));
            value["mime"] = json!(output.to_mime_type());
        }
        Mode::Original => {
            ensure!(
                request.offset < before.len()
                    && (request.offset == 0 || request.revision.is_some()),
                "invalid image offset"
            );
            file.seek(SeekFrom::Start(request.offset))?;
            let mut bytes = vec![0; limit.min((before.len() - request.offset) as usize)];
            file.read_exact(&mut bytes)?;
            let end = request.offset + bytes.len() as u64;
            value["content"] = json!(STANDARD.encode(bytes));
            value["offset"] = json!(request.offset);
            value["next_offset"] = if end < before.len() {
                json!(end)
            } else {
                Value::Null
            };
        }
    }
    ensure!(
        revision(&file, &file.metadata()?)? == stamp,
        "image changed; reload the preview"
    );
    Ok(value)
}

use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::Value;
use std::{fs, io::Cursor};
use wakeit::runner::image_preview::{self, Mode, Request};

fn fixture(format: ImageFormat) -> (tempfile::TempDir, std::path::PathBuf, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let mut rng = 0x12345678u32;
    let image = RgbaImage::from_fn(1400, 1000, |_, _| {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        Rgba([rng as u8, (rng >> 8) as u8, (rng >> 16) as u8, 255])
    });
    let mut out = Cursor::new(Vec::new());
    let image = DynamicImage::ImageRgba8(image);
    if format == ImageFormat::Jpeg {
        image.to_rgb8().write_to(&mut out, format).unwrap();
    } else {
        image.write_to(&mut out, format).unwrap();
    }
    let bytes = out.into_inner();
    let path = dir.path().join("large-image"); // Magic, rather than extension.
    fs::write(&path, &bytes).unwrap();
    (dir, path, bytes)
}
#[test]
fn large_png_jpeg_webp_have_bounded_thumbnails_and_byte_identical_originals() {
    for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
        let (dir, path, original) = fixture(format);
        assert!(original.len() > wakeit::runner::result_files::MAX_BYTES);
        let roots = [dir.path().to_owned()];
        let read = |image, offset, revision| {
            image_preview::read(
                &roots,
                path.to_str().unwrap(),
                None,
                64 * 1024,
                Request {
                    image: Some(image),
                    offset,
                    revision,
                },
            )
        };
        let thumb = read(Mode::Thumbnail, 0, None).unwrap();
        assert_eq!(thumb["size"], original.len());
        assert_eq!(thumb["image"], "thumbnail");
        let bytes = STANDARD.decode(thumb["content"].as_str().unwrap()).unwrap();
        assert!(bytes.len() <= 64 * 1024);
        let image = image::load_from_memory(&bytes).unwrap();
        assert!(image.width() <= 1280 && image.height() <= 1280);
        let mut offset = 0;
        let mut restored = Vec::new();
        loop {
            let chunk = read(
                Mode::Original,
                offset,
                Some(thumb["revision"].as_str().unwrap().into()),
            )
            .unwrap();
            assert_eq!(chunk["offset"], offset);
            restored.extend(STANDARD.decode(chunk["content"].as_str().unwrap()).unwrap());
            match chunk["next_offset"].as_u64() {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(original, restored);
    }
}
#[test]
fn revisions_offsets_roots_and_resource_limits_are_checked() {
    let (dir, path, bytes) = fixture(ImageFormat::Jpeg);
    let roots = [dir.path().to_owned()];
    let read = |offset, revision| {
        image_preview::read(
            &roots,
            path.to_str().unwrap(),
            None,
            4096,
            Request {
                image: Some(Mode::Original),
                offset,
                revision,
            },
        )
    };
    let first = read(0, None).unwrap();
    assert!(read(1, None).unwrap_err().to_string().contains("offset"));
    assert!(
        read(
            bytes.len() as u64,
            Some(first["revision"].as_str().unwrap().into())
        )
        .is_err()
    );
    fs::write(&path, &bytes).unwrap();
    assert!(
        read(4096, Some(first["revision"].as_str().unwrap().into()))
            .unwrap_err()
            .to_string()
            .contains("changed")
    );
    assert!(
        image_preview::read(
            &[],
            path.to_str().unwrap(),
            None,
            4096,
            Request {
                image: Some(Mode::Thumbnail),
                ..Default::default()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("allowed_roots")
    );
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(image_preview::MAX_IMAGE_BYTES + 1).unwrap();
    assert!(read(0, None).unwrap_err().to_string().contains("32 MiB"));
    RgbaImage::from_pixel(16385, 1, Rgba([1, 2, 3, 255]))
        .save_with_format(&path, ImageFormat::Png)
        .unwrap();
    assert!(
        read(0, None)
            .unwrap_err()
            .to_string()
            .contains("dimensions")
    );
    fs::write(&path, b"\x89PNG\r\n\x1a\ntruncated").unwrap();
    assert!(read(0, None).unwrap_err().to_string().contains("decoded"));
}
#[test]
fn text_preview_keeps_its_existing_limit_and_transparent_thumbnails_keep_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let roots = [dir.path().to_owned()];
    let path = dir.path().join("report.md");
    fs::write(&path, "a".repeat(700_000)).unwrap();
    let text = image_preview::read(
        &roots,
        path.to_str().unwrap(),
        None,
        524288,
        Request {
            image: Some(Mode::Thumbnail),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(text["format"], "markdown");
    assert_eq!(text["truncated"], true);
    assert_eq!(text["content"].as_str().unwrap().len(), 524288);
    let path = dir.path().join("alpha.png");
    RgbaImage::from_pixel(10, 10, Rgba([12, 34, 56, 100]))
        .save(&path)
        .unwrap();
    let thumb = image_preview::read(
        &roots,
        path.to_str().unwrap(),
        None,
        4096,
        Request {
            image: Some(Mode::Thumbnail),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(thumb["mime"], "image/png");
    let img =
        image::load_from_memory(&STANDARD.decode(thumb["content"].as_str().unwrap()).unwrap())
            .unwrap()
            .into_rgba8();
    assert_eq!(img.get_pixel(0, 0)[3], 100);
    let _: Value = thumb;
}

use image::{DynamicImage, ImageFormat, RgbaImage};
use linguist_application::media::{ImageInspectionError, inspect_image};
use std::io::Cursor;

fn settings() -> linguist_config::Effective {
    linguist_config::resolve(
        &linguist_config::Registry::builtin(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

fn raster(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    let image = DynamicImage::ImageRgba8(RgbaImage::new(width, height));
    let image = if format == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(image.to_rgb8())
    } else {
        image
    };
    image.write_to(&mut out, format).unwrap();
    out.into_inner()
}

#[test]
fn formats_are_decoded_and_allowlist_is_enforced() {
    let mut config = settings();
    for (format, mime) in [
        (ImageFormat::Png, "image/png"),
        (ImageFormat::Jpeg, "image/jpeg"),
        (ImageFormat::WebP, "image/webp"),
        (ImageFormat::Gif, "image/gif"),
    ] {
        let bytes = raster(3, 2, format);
        let result = inspect_image(&bytes, &config).unwrap();
        assert_eq!(result.mime, mime);
        assert_eq!(
            (result.width, result.height, result.decoded_units),
            (3, 2, 1)
        );
        assert!(result.decoded_bytes > 0);
        assert!(inspect_image(&bytes[..bytes.len() / 2], &config).is_err());
    }
    config.values.insert(
        "media.allowed_image_types".into(),
        serde_json::json!(["image/jpeg"]),
    );
    assert_eq!(
        inspect_image(&raster(1, 1, ImageFormat::Png), &config).unwrap_err(),
        ImageInspectionError::ImageFormatDisallowed
    );
    assert!(inspect_image(&[1, 2, 3], &config).is_err());
}

#[test]
fn failures_are_typed_and_never_include_decoder_supplied_file_text() {
    let mut config = settings();
    let mut truncated = raster(2, 2, ImageFormat::Png);
    truncated.truncate(12);
    let failure = inspect_image(&truncated, &config).unwrap_err();
    assert_eq!(failure, ImageInspectionError::ImageDecodeFailed);
    assert_eq!(
        serde_json::to_value(&failure).unwrap(),
        serde_json::json!({"code":"IMAGE_DECODE_FAILED"})
    );
    assert!(failure.guidance().contains("corruption or truncation"));
    let oversized = raster(1024, 1024, ImageFormat::Png);
    config
        .values
        .insert("media.max_asset_mb".into(), serde_json::json!(1));
    let failure = inspect_image(&oversized, &config).unwrap_err();
    assert!(matches!(
        failure,
        ImageInspectionError::ImageDecodedLimit | ImageInspectionError::ImageDecoderResourceLimit
    ));
    assert!(failure.guidance().contains("media.max_asset_mb"));
    config
        .values
        .insert("media.max_asset_mb".into(), serde_json::json!(0));
    assert!(matches!(
        inspect_image(&oversized, &config),
        Err(ImageInspectionError::InvalidSettings { .. })
    ));
}

#[test]
fn compressed_pixel_bombs_and_animation_totals_are_bounded() {
    let mut config = settings();
    config
        .values
        .insert("media.max_asset_mb".into(), serde_json::json!(1));
    assert!(inspect_image(&raster(1024, 1024, ImageFormat::Png), &config).is_err());
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        for _ in 0..7 {
            encoder
                .encode_frame(image::Frame::new(RgbaImage::new(200, 200)))
                .unwrap();
        }
    }
    assert!(bytes.len() < 1024 * 1024);
    assert_eq!(
        inspect_image(&bytes, &config).unwrap_err(),
        ImageInspectionError::ImageDecodedLimit
    );
    config
        .values
        .insert("media.max_asset_mb".into(), serde_json::json!(10));
    let result = inspect_image(&bytes, &config).unwrap();
    assert_eq!(result.decoded_units, 7);
    assert_eq!(result.decoded_bytes, 7 * 200 * 200 * 4);
}

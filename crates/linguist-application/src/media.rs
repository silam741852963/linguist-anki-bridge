//! Raster inspection never transforms source bytes or authorizes a rendering role.
use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::Serialize;
use std::io::Cursor;

#[derive(Debug, Serialize)]
pub struct ImageInspection {
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub decoded_bytes: u64,
    /// APNG default image and animation buffers are counted separately.
    pub decoded_units: u64,
    pub decoder: &'static str,
}

/// Validate settings separately so invalid configuration cannot become a review issue.
pub fn validate_settings(settings: &linguist_config::Effective) -> Result<(), String> {
    for key in ["media.max_asset_mb", "media.allowed_image_types"] {
        linguist_config::Registry::builtin().validate_value(
            key,
            settings.values.get(key).ok_or("MEDIA_SETTING_MISSING")?,
        )?;
    }
    Ok(())
}

/// The cap bounds input bytes and cumulative decoded buffers. Decoder allocations are
/// best effort; this is not an operating-system memory or execution-time sandbox.
pub fn inspect_image(
    bytes: &[u8],
    settings: &linguist_config::Effective,
) -> Result<ImageInspection, String> {
    validate_settings(settings)?;
    let cap = settings.values["media.max_asset_mb"].as_u64().unwrap() * 1024 * 1024;
    if bytes.is_empty() || bytes.len() as u64 > cap {
        return Err("IMAGE_INPUT_LIMIT".into());
    }
    let format = image::guess_format(bytes).map_err(|_| "IMAGE_FORMAT_UNSUPPORTED")?;
    let mime = match format {
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Png => "image/png",
        ImageFormat::Gif => "image/gif",
        ImageFormat::WebP => "image/webp",
        _ => return Err("IMAGE_FORMAT_UNSUPPORTED".into()),
    };
    if !settings.values["media.allowed_image_types"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str() == Some(mime))
    {
        return Err("IMAGE_FORMAT_DISALLOWED".into());
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(cap);
    limits.max_image_width = Some(cap as u32);
    limits.max_image_height = Some(cap as u32);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits.clone());
    let decoder = reader.into_decoder().map_err(decode_error)?;
    let (width, height) = decoder.dimensions();
    let mut result = ImageInspection {
        mime: mime.into(),
        width,
        height,
        decoded_bytes: 0,
        decoded_units: 0,
        decoder: "image/0.25.10",
    };
    let mut add = |size: u64| -> Result<(), String> {
        result.decoded_bytes = result
            .decoded_bytes
            .checked_add(size)
            .ok_or("IMAGE_DECODED_LIMIT")?;
        if result.decoded_bytes > cap {
            return Err("IMAGE_DECODED_LIMIT".into());
        }
        result.decoded_units += 1;
        Ok(())
    };
    // Check the output buffer before decoding, including a full RGBA animation canvas.
    let canvas = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(4))
        .ok_or("IMAGE_DECODED_LIMIT")?;
    if decoder.total_bytes() > cap || canvas > cap || width == 0 || height == 0 {
        return Err("IMAGE_DECODED_LIMIT".into());
    }
    match format {
        ImageFormat::Gif => {
            drop(decoder);
            let mut decoder =
                image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(decode_error)?;
            decoder.set_limits(limits).map_err(decode_error)?;
            for frame in decoder.into_frames() {
                let frame = frame.map_err(decode_error)?;
                add(frame.buffer().as_raw().len() as u64)?;
            }
        }
        ImageFormat::WebP => {
            let mut webp =
                image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(decode_error)?;
            webp.set_limits(limits).map_err(decode_error)?;
            if webp.has_animation() {
                drop(decoder);
                for frame in webp.into_frames() {
                    let frame = frame.map_err(decode_error)?;
                    add(frame.buffer().as_raw().len() as u64)?;
                }
            } else {
                drop(webp);
                add(decoder.total_bytes())?;
                image::DynamicImage::from_decoder(decoder).map_err(decode_error)?;
            }
        }
        ImageFormat::Png => {
            add(decoder.total_bytes())?;
            image::DynamicImage::from_decoder(decoder).map_err(decode_error)?;
            let png = image::codecs::png::PngDecoder::with_limits(Cursor::new(bytes), limits)
                .map_err(decode_error)?;
            if png.is_apng().map_err(decode_error)? {
                for frame in png.apng().map_err(decode_error)?.into_frames() {
                    let frame = frame.map_err(decode_error)?;
                    add(frame.buffer().as_raw().len() as u64)?;
                }
            }
        }
        _ => {
            add(decoder.total_bytes())?;
            image::DynamicImage::from_decoder(decoder).map_err(decode_error)?;
        }
    }
    if result.decoded_units == 0 {
        return Err("IMAGE_NO_DECODED_BUFFERS".into());
    }
    Ok(result)
}

fn decode_error(error: image::ImageError) -> String {
    format!("IMAGE_DECODE_FAILED:{error}")
}

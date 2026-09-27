//! Raster inspection never transforms source bytes or authorizes a rendering role.
use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::Serialize;
use std::io::Cursor;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum SourceMediaInspection {
    Image(ImageInspection),
    Audio(crate::audio::AudioInspection),
}
impl SourceMediaInspection {
    pub fn mime(&self) -> &str {
        match self {
            Self::Image(i) => &i.mime,
            Self::Audio(i) => &i.mime,
        }
    }
    pub fn requires_audio_completeness_review(&self) -> bool {
        matches!(self, Self::Audio(i) if !i.container_extent_verified)
    }
}
#[derive(Debug, Serialize)]
pub struct SourceMediaFailure {
    pub code: String,
    pub guidance: String,
}
impl std::fmt::Display for SourceMediaFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.code)
    }
}
impl SourceMediaFailure {
    pub fn guidance(&self) -> &str {
        &self.guidance
    }
}
/// Only an unrecognized image format falls through to audio probing. A rejected
/// image cannot bypass its allowlist or decode limits by trying another decoder.
pub fn inspect_source_media(
    bytes: &[u8],
    settings: &linguist_config::Effective,
) -> Result<SourceMediaInspection, SourceMediaFailure> {
    match inspect_image(bytes, settings) {
        Ok(i) => Ok(SourceMediaInspection::Image(i)),
        Err(ImageInspectionError::ImageFormatUnsupported) => match crate::audio::inspect_audio(bytes, settings) {
            Ok(i) => Ok(SourceMediaInspection::Audio(i)),
            Err(crate::audio::AudioInspectionError::AudioFormatUnsupported) => Err(SourceMediaFailure {
                code: "MEDIA_FORMAT_UNSUPPORTED".into(),
                guidance: "Inspect the original file and provide supported image or audio content. A filename extension cannot establish format.".into(),
            }),
            Err(failure) => Err(SourceMediaFailure { code: failure.to_string(), guidance: failure.guidance().into() }),
        },
        Err(failure) => Err(SourceMediaFailure { code: failure.to_string(), guidance: failure.guidance().into() }),
    }
}

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

/// Stable failure categories are safe to persist without decoder-supplied file text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ImageInspectionError {
    InvalidSettings { message: String },
    ImageInputLimit,
    ImageFormatUnsupported,
    ImageFormatDisallowed,
    ImageDecodedLimit,
    ImageDecoderResourceLimit,
    ImageDecodeFailed,
    ImageDecoderUnsupported,
    ImageNoDecodedBuffers,
}
impl ImageInspectionError {
    pub fn guidance(&self) -> &'static str {
        match self {
            Self::InvalidSettings { .. } => {
                "Correct the media configuration before preparing again."
            }
            Self::ImageInputLimit | Self::ImageDecodedLimit | Self::ImageDecoderResourceLimit => {
                "Inspect the original asset and its dimensions or animation length. Prepare a smaller replacement, or deliberately adjust media.max_asset_mb before creating a new plan."
            }
            Self::ImageFormatDisallowed => {
                "Review media.allowed_image_types. Use an allowed replacement, or explicitly update that setting before creating a new plan."
            }
            Self::ImageFormatUnsupported | Self::ImageDecoderUnsupported => {
                "Inspect the original file. Audio decoding and unsupported image formats need separate validation; provide a supported JPEG, PNG, GIF or WebP replacement for an image role."
            }
            Self::ImageDecodeFailed | Self::ImageNoDecodedBuffers => {
                "Check the original file for corruption or truncation and obtain a valid replacement. A filename extension does not establish a valid image."
            }
        }
    }
}
impl std::fmt::Display for ImageInspectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSettings { message } => write!(f, "IMAGE_INVALID_SETTINGS:{message}"),
            _ => write!(
                f,
                "{}",
                serde_json::to_value(self).map_err(|_| std::fmt::Error)?["code"]
                    .as_str()
                    .ok_or(std::fmt::Error)?
            ),
        }
    }
}
impl std::error::Error for ImageInspectionError {}

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
) -> Result<ImageInspection, ImageInspectionError> {
    validate_settings(settings)
        .map_err(|message| ImageInspectionError::InvalidSettings { message })?;
    let cap = settings.values["media.max_asset_mb"].as_u64().unwrap() * 1024 * 1024;
    if bytes.is_empty() || bytes.len() as u64 > cap {
        return Err(ImageInspectionError::ImageInputLimit);
    }
    let format =
        image::guess_format(bytes).map_err(|_| ImageInspectionError::ImageFormatUnsupported)?;
    let mime = match format {
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Png => "image/png",
        ImageFormat::Gif => "image/gif",
        ImageFormat::WebP => "image/webp",
        _ => return Err(ImageInspectionError::ImageFormatUnsupported),
    };
    if !settings.values["media.allowed_image_types"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str() == Some(mime))
    {
        return Err(ImageInspectionError::ImageFormatDisallowed);
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
    let mut add = |size: u64| -> Result<(), ImageInspectionError> {
        result.decoded_bytes = result
            .decoded_bytes
            .checked_add(size)
            .ok_or(ImageInspectionError::ImageDecodedLimit)?;
        if result.decoded_bytes > cap {
            return Err(ImageInspectionError::ImageDecodedLimit);
        }
        result.decoded_units += 1;
        Ok(())
    };
    // Check the output buffer before decoding, including a full RGBA animation canvas.
    let canvas = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(4))
        .ok_or(ImageInspectionError::ImageDecodedLimit)?;
    if decoder.total_bytes() > cap || canvas > cap || width == 0 || height == 0 {
        return Err(ImageInspectionError::ImageDecodedLimit);
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
        return Err(ImageInspectionError::ImageNoDecodedBuffers);
    }
    Ok(result)
}

fn decode_error(error: image::ImageError) -> ImageInspectionError {
    match error {
        image::ImageError::Limits(_) => ImageInspectionError::ImageDecoderResourceLimit,
        image::ImageError::Unsupported(_) => ImageInspectionError::ImageDecoderUnsupported,
        _ => ImageInspectionError::ImageDecodeFailed,
    }
}

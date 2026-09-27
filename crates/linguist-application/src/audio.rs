//! Decode source audio into bounded packet buffers without rewriting source bytes.
mod mp3;
mod ogg;
use serde::Serialize;
use std::io::Cursor;
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    common::Limit,
    formats::{FormatOptions, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

#[derive(Debug, Serialize)]
pub struct AudioInspection {
    pub mime: String,
    pub container: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub decoded_frames: u64,
    /// Conservative budget: eight bytes per sample, independent of decoder type.
    pub decoded_sample_budget_bytes: u64,
    pub decoded_packets: u64,
    pub stream_end_observed: bool,
    pub container_extent_verified: bool,
    pub decoder_verification: Option<bool>,
    pub decoder: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AudioInspectionError {
    AudioInputLimit,
    AudioFormatUnsupported,
    AudioFormatDisallowed,
    AudioContainerInvalid,
    AudioTrackUnsupported,
    AudioDecodeFailed,
    AudioDecodedLimit,
    AudioStreamChanged,
    AudioNoDecodedSamples,
    AudioInvalidSettings,
}
impl AudioInspectionError {
    pub fn guidance(&self) -> &'static str {
        match self {
            Self::AudioFormatDisallowed => {
                "Review media.allowed_audio_types before creating a new plan."
            }
            Self::AudioInputLimit | Self::AudioDecodedLimit => {
                "Use a shorter or smaller replacement, or deliberately adjust media.max_asset_mb before creating a new plan."
            }
            Self::AudioInvalidSettings => "Correct the media configuration before preparing again.",
            Self::AudioContainerInvalid | Self::AudioDecodeFailed | Self::AudioNoDecodedSamples => {
                "Check the original recording for truncation or corruption and obtain a valid replacement."
            }
            _ => {
                "Inspect the original recording and provide a single-track MP3, Ogg/Vorbis or WAV/PCM replacement. Unsupported codecs and changing or chained streams require separate review."
            }
        }
    }
}
impl std::fmt::Display for AudioInspectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_value(self).map_err(|_| std::fmt::Error)?["code"]
                .as_str()
                .ok_or(std::fmt::Error)?
        )
    }
}
impl std::error::Error for AudioInspectionError {}

pub fn validate_settings(settings: &linguist_config::Effective) -> Result<(), String> {
    for key in ["media.max_asset_mb", "media.allowed_audio_types"] {
        linguist_config::Registry::builtin().validate_value(
            key,
            settings.values.get(key).ok_or("MEDIA_SETTING_MISSING")?,
        )?;
    }
    Ok(())
}

pub fn inspect_audio(
    bytes: &[u8],
    settings: &linguist_config::Effective,
) -> Result<AudioInspection, AudioInspectionError> {
    use AudioInspectionError::*;
    validate_settings(settings).map_err(|_| AudioInvalidSettings)?;
    let cap = settings.values["media.max_asset_mb"].as_u64().unwrap() * 1024 * 1024;
    if bytes.is_empty() || bytes.len() as u64 > cap {
        return Err(AudioInputLimit);
    }
    let ogg_extent_verified = if bytes.starts_with(b"OggS") {
        ogg::validate_extent(bytes)?;
        true
    } else {
        false
    };
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            source,
            FormatOptions::default(),
            MetadataOptions::default()
                .limit_tag_bytes(Limit::Maximum(cap.min(64 * 1024) as usize))
                .limit_visual_bytes(Limit::Maximum(0)),
        )
        .map_err(|_| AudioFormatUnsupported)?;
    let container = format.format_info().short_name;
    let mime = match container {
        "wave" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        _ => return Err(AudioFormatUnsupported),
    };
    if !settings.values["media.allowed_audio_types"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some(mime))
    {
        return Err(AudioFormatDisallowed);
    }
    let expected_frames = if container == "wave" {
        Some(validate_wave_extent(bytes)?)
    } else {
        None
    };
    let mp3_extent_verified = if container == "mp3" {
        mp3::validate_extent(bytes)?;
        true
    } else {
        false
    };
    if format.tracks().len() != 1 {
        return Err(AudioTrackUnsupported);
    }
    let track = format
        .default_track(TrackType::Audio)
        .ok_or(AudioTrackUnsupported)?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or(AudioTrackUnsupported)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default().verify(true))
        .map_err(|_| AudioTrackUnsupported)?;
    let track_id = track.id;
    let mut result = AudioInspection {
        mime: mime.into(),
        container: container.into(),
        sample_rate: 0,
        channels: 0,
        decoded_frames: 0,
        decoded_sample_budget_bytes: 0,
        decoded_packets: 0,
        stream_end_observed: false,
        container_extent_verified: expected_frames.is_some()
            || (container == "ogg" && ogg_extent_verified)
            || mp3_extent_verified,
        decoder_verification: None,
        decoder: "symphonia/0.6.1",
    };
    while let Some(packet) = format.next_packet().map_err(audio_error)? {
        if packet.track_id != track_id {
            return Err(AudioStreamChanged);
        }
        let decoded = decoder.decode(&packet).map_err(audio_error)?;
        let rate = decoded.spec().rate();
        let channels = decoded.spec().channels().count();
        if rate == 0 || channels == 0 {
            return Err(AudioNoDecodedSamples);
        }
        if result.decoded_packets == 0 {
            result.sample_rate = rate;
            result.channels = channels;
        }
        if result.sample_rate != rate || result.channels != channels {
            return Err(AudioStreamChanged);
        }
        result.decoded_frames = result
            .decoded_frames
            .checked_add(decoded.frames() as u64)
            .ok_or(AudioDecodedLimit)?;
        result.decoded_sample_budget_bytes = result
            .decoded_frames
            .checked_mul(channels as u64)
            .and_then(|n| n.checked_mul(8))
            .ok_or(AudioDecodedLimit)?;
        if result.decoded_sample_budget_bytes > cap {
            return Err(AudioDecodedLimit);
        }
        result.decoded_packets += 1;
        while !format.metadata().is_latest() {
            format.metadata().pop();
        }
    }
    if result.decoded_frames == 0 {
        return Err(AudioNoDecodedSamples);
    }
    if expected_frames.is_some_and(|expected| expected != result.decoded_frames) {
        return Err(AudioContainerInvalid);
    }
    result.stream_end_observed = true;
    result.decoder_verification = decoder.finalize().verify_ok;
    if result.decoder_verification == Some(false) {
        return Err(AudioDecodeFailed);
    }
    Ok(result)
}

fn audio_error(error: symphonia::core::errors::Error) -> AudioInspectionError {
    match error {
        symphonia::core::errors::Error::ResetRequired => AudioInspectionError::AudioStreamChanged,
        _ => AudioInspectionError::AudioDecodeFailed,
    }
}

/// Standard RIFF/WAVE only: every declared chunk, padding byte and container byte
/// must be present. RF64 and multiple data chunks are left for explicit review.
fn validate_wave_extent(bytes: &[u8]) -> Result<u64, AudioInspectionError> {
    use AudioInspectionError::AudioContainerInvalid;
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(AudioContainerInvalid);
    }
    let size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as u64 + 8;
    if size != bytes.len() as u64 {
        return Err(AudioContainerInvalid);
    }
    let mut offset = 12usize;
    let mut data_chunks = 0;
    let mut format_chunks = 0;
    let mut block_align = None;
    let mut data_bytes = 0;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset.checked_add(8).ok_or(AudioContainerInvalid)?)
            .ok_or(AudioContainerInvalid)?;
        let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        if &header[..4] == b"data" {
            data_chunks += 1;
            if block_align.is_none() {
                return Err(AudioContainerInvalid);
            }
            data_bytes = size as u64;
        }
        if &header[..4] == b"fmt " {
            format_chunks += 1;
            if size < 16 {
                return Err(AudioContainerInvalid);
            }
            let fmt = bytes
                .get(offset + 8..offset + 24)
                .ok_or(AudioContainerInvalid)?;
            let align = u16::from_le_bytes(fmt[12..14].try_into().unwrap());
            if align == 0 {
                return Err(AudioContainerInvalid);
            }
            block_align = Some(u64::from(align));
        }
        offset = offset
            .checked_add(8)
            .and_then(|n| n.checked_add(size))
            .and_then(|n| n.checked_add(size % 2))
            .ok_or(AudioContainerInvalid)?;
        if offset > bytes.len() {
            return Err(AudioContainerInvalid);
        }
    }
    if data_chunks != 1 || format_chunks != 1 {
        return Err(AudioContainerInvalid);
    }
    let align = block_align.ok_or(AudioContainerInvalid)?;
    if data_bytes % align != 0 {
        return Err(AudioContainerInvalid);
    }
    Ok(data_bytes / align)
}

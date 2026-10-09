//! Japanese speech from a local VOICEVOX engine (`audio.provider=voicevox`, or
//! `audio.synthesis_fallback=voicevox` when the dictionary has no recording).
//!
//! The engine's HTTP API is used as documented: `POST /audio_query` builds the
//! accent phrases for the text and style, `POST /synthesis` renders them to
//! WAV. The character's name comes from `GET /speakers` for the credit line
//! VOICEVOX requires ("VOICEVOX:<character>"). The result is a candidate that a
//! reviewer listens to, like every other audio candidate.
use crate::speech::Synthesis;
use linguist_config::Effective;
use linguist_provider::sha256_hex;
use serde_json::Value;
use std::{io::Read, time::Duration};

const MAX_WAV: u64 = 16 * 1024 * 1024;

/// The character and style names of one VOICEVOX style id.
fn character(speakers: &Value, style: u64) -> Option<(String, String)> {
    speakers.as_array()?.iter().find_map(|speaker| {
        speaker["styles"].as_array()?.iter().find_map(|s| {
            (s["id"].as_u64() == Some(style)).then(|| {
                (
                    speaker["name"].as_str().unwrap_or_default().to_owned(),
                    s["name"].as_str().unwrap_or_default().to_owned(),
                )
            })
        })
    })
}

/// Sample rate of a RIFF/WAVE file, or `None` when it is not one.
fn wav_rate(bytes: &[u8]) -> Option<u32> {
    (bytes.len() > 44 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE")
        .then(|| u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]))
}

fn with_query(endpoint: &str, path: &str, pairs: &[(&str, &str)]) -> Result<url::Url, String> {
    let mut url =
        url::Url::parse(&format!("{endpoint}/{path}")).map_err(|_| "VOICEVOX_SETTING_INVALID")?;
    url.query_pairs_mut().extend_pairs(pairs);
    Ok(url)
}

/// The engine's version, or why it cannot be reached (`doctor`).
pub fn probe(settings: &Effective) -> Result<String, String> {
    let endpoint = settings
        .values
        .get("audio.voicevox.endpoint")
        .and_then(Value::as_str)
        .ok_or("VOICEVOX_SETTING_MISSING")?
        .trim_end_matches('/');
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .build()
        .map_err(|_| "VOICEVOX_UNAVAILABLE")?
        .get(format!("{endpoint}/version"))
        .send()
        .map_err(|_| "VOICEVOX_UNAVAILABLE")?;
    if !response.status().is_success() {
        return Err("VOICEVOX_UNAVAILABLE".into());
    }
    response
        .json::<String>()
        .map_err(|_| "VOICEVOX_RESPONSE_INVALID".into())
}

pub fn synthesize(text: &str, settings: &Effective) -> Result<Synthesis, String> {
    let endpoint = settings
        .values
        .get("audio.voicevox.endpoint")
        .and_then(Value::as_str)
        .ok_or("VOICEVOX_SETTING_MISSING")?
        .trim_end_matches('/')
        .to_owned();
    let style = settings
        .values
        .get("audio.voicevox.speaker")
        .and_then(Value::as_u64)
        .ok_or("VOICEVOX_SETTING_MISSING")?;
    let speed = settings
        .values
        .get("audio.speed")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let timeout = Duration::from_secs(
        settings
            .values
            .get("audio.timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(60),
    );
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 200 || text.chars().any(char::is_control) {
        return Err("VOICEVOX_TEXT_INVALID".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| "VOICEVOX_UNAVAILABLE")?;
    let unavailable = |_| {
        "VOICEVOX_UNAVAILABLE: start the VOICEVOX engine or set audio.voicevox.endpoint".to_owned()
    };
    let read = |response: reqwest::blocking::Response, limit: u64| -> Result<Vec<u8>, String> {
        let status = response.status();
        let mut bytes = Vec::new();
        response
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "VOICEVOX_READ_FAILED")?;
        if bytes.len() as u64 > limit {
            return Err("VOICEVOX_OUTPUT_LIMIT".into());
        }
        if !status.is_success() {
            return Err(format!("VOICEVOX_HTTP_FAILED: {}", status.as_u16()));
        }
        Ok(bytes)
    };
    let version: String = serde_json::from_slice(&read(
        client
            .get(format!("{endpoint}/version"))
            .send()
            .map_err(unavailable)?,
        4096,
    )?)
    .map_err(|_| "VOICEVOX_RESPONSE_INVALID")?;
    let speakers: Value = serde_json::from_slice(&read(
        client
            .get(format!("{endpoint}/speakers"))
            .send()
            .map_err(unavailable)?,
        4 * 1024 * 1024,
    )?)
    .map_err(|_| "VOICEVOX_RESPONSE_INVALID")?;
    let (name, style_name) = character(&speakers, style).ok_or("VOICEVOX_SPEAKER_UNKNOWN")?;
    let mut query: Value = serde_json::from_slice(&read(
        client
            .post(with_query(
                &endpoint,
                "audio_query",
                &[("text", text), ("speaker", &style.to_string())],
            )?)
            .send()
            .map_err(unavailable)?,
        1024 * 1024,
    )?)
    .map_err(|_| "VOICEVOX_RESPONSE_INVALID")?;
    if !query.is_object() {
        return Err("VOICEVOX_RESPONSE_INVALID".into());
    }
    query["speedScale"] = speed.into();
    let wav = read(
        client
            .post(with_query(
                &endpoint,
                "synthesis",
                &[("speaker", &style.to_string())],
            )?)
            .json(&query)
            .send()
            .map_err(unavailable)?,
        MAX_WAV,
    )?;
    let sample_rate = wav_rate(&wav).ok_or("VOICEVOX_AUDIO_INVALID")?;
    Ok(Synthesis {
        provider: "voicevox",
        engine_version: version,
        executable_sha256: String::new(),
        voice_sha256: String::new(),
        voice_config_sha256: String::new(),
        voice_language: "ja".into(),
        voice_dataset: Some(format!("VOICEVOX:{name}")),
        speaker: Some(format!("{style} ({name} {style_name})")),
        length_scale: speed,
        text_sha256: sha256_hex(text.as_bytes()),
        mime: "audio/wav".into(),
        sample_rate,
        sha256: sha256_hex(&wav),
        size_bytes: wav.len() as u64,
        review_required: true,
        bytes: wav,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_names_come_from_the_style_id() {
        let speakers = serde_json::json!([
            {"name": "四国めたん", "styles": [{"id": 2, "name": "ノーマル"}]},
            {"name": "春日部つむぎ", "styles": [{"id": 8, "name": "ノーマル"}]}]);
        assert_eq!(
            character(&speakers, 8),
            Some(("春日部つむぎ".into(), "ノーマル".into()))
        );
        assert_eq!(character(&speakers, 99), None);
        assert_eq!(wav_rate(b"not a wav"), None);
    }
}

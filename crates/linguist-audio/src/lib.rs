//! Audio selection and provider boundaries; all pronunciations remain visible.

use std::{future::Future, pin::Pin};
pub type AudioFuture<'a> = Pin<Box<dyn Future<Output = Result<AudioClip, AudioError>> + Send + 'a>>;
pub trait DictionaryAudioPort: Send + Sync {
    fn fetch<'a>(&'a self, url: &'a str) -> AudioFuture<'a>;
}
pub trait TtsPort: Send + Sync {
    fn synthesize<'a>(&'a self, text: &'a str, voice: &'a Voice) -> AudioFuture<'a>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pronunciation {
    pub text: String,
    pub locale: String,
    pub audio_url: Option<String>,
    pub source: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Voice {
    pub id: String,
    pub locale: String,
    pub local: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioClip {
    pub data: Vec<u8>,
    pub mime: String,
    pub source: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioError {
    Unavailable(String),
    Invalid(String),
}
impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(v) | Self::Invalid(v) => f.write_str(v),
        }
    }
}
impl std::error::Error for AudioError {}

pub fn select_voice<'a>(voices: &'a [Voice], locale: &str) -> Option<&'a Voice> {
    let language = locale.split('-').next().unwrap_or(locale);
    voices
        .iter()
        .filter(|voice| voice.local)
        .find(|voice| voice.locale.eq_ignore_ascii_case(locale))
        .or_else(|| {
            voices.iter().filter(|voice| voice.local).find(|voice| {
                voice
                    .locale
                    .split('-')
                    .next()
                    .unwrap_or(&voice.locale)
                    .eq_ignore_ascii_case(language)
            })
        })
        .or_else(|| voices.iter().find(|voice| voice.local))
}
pub fn media_filename(expression: &str, locale: &str, ordinal: usize) -> String {
    format!(
        "audio-{}-{}-{:02}.mp3",
        stem(expression),
        stem(locale),
        ordinal + 1
    )
}
pub fn usable_pronunciations(rows: impl IntoIterator<Item = Pronunciation>) -> Vec<Pronunciation> {
    rows.into_iter()
        .filter(|row| {
            !row.text.trim().is_empty()
                && (row
                    .audio_url
                    .as_ref()
                    .is_none_or(|url| url.starts_with("https://")))
        })
        .collect()
}
fn stem(value: &str) -> String {
    let output = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let output = output.trim_matches('-');
    if output.is_empty() {
        "term".into()
    } else {
        output.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chooses_local_locale_then_language() {
        let voices = vec![
            Voice {
                id: "en".into(),
                locale: "en-US".into(),
                local: true,
            },
            Voice {
                id: "ja".into(),
                locale: "ja-JP".into(),
                local: true,
            },
            Voice {
                id: "remote".into(),
                locale: "ja-JP".into(),
                local: false,
            },
        ];
        assert_eq!(select_voice(&voices, "ja-JP").unwrap().id, "ja");
        assert_eq!(select_voice(&voices, "en-GB").unwrap().id, "en");
    }
    #[test]
    fn preserves_many_pronunciations_and_names_them() {
        let rows = usable_pronunciations(vec![
            Pronunciation {
                text: "食べる".into(),
                locale: "ja-JP".into(),
                audio_url: Some("https://x/a.mp3".into()),
                source: "dict".into(),
            },
            Pronunciation {
                text: "たべる".into(),
                locale: "ja-JP".into(),
                audio_url: None,
                source: "tts".into(),
            },
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            media_filename("食べる", "ja-JP", 1),
            "audio-term-ja-jp-02.mp3"
        );
    }
}

//! Audio selection and provider boundaries; all pronunciations remain visible.

use std::time::Duration;
use std::{
    collections::BTreeSet,
    future::Future,
    pin::Pin,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_AUDIO_BYTES: usize = 6 * 1024 * 1024;
pub type AudioFuture<'a> = Pin<Box<dyn Future<Output = Result<AudioClip, AudioError>> + Send + 'a>>;
pub trait DictionaryAudioPort: Send + Sync {
    fn fetch<'a>(&'a self, url: &'a str) -> AudioFuture<'a>;
}
pub trait TtsPort: Send + Sync {
    fn synthesize<'a>(&'a self, text: &'a str, voice: &'a Voice) -> AudioFuture<'a>;
}

#[derive(Clone, Debug)]
pub struct HttpAudioFetcher {
    client: reqwest::Client,
    trusted_hosts: BTreeSet<String>,
    max_bytes: usize,
}

impl HttpAudioFetcher {
    pub fn dictionary_defaults() -> Result<Self, AudioError> {
        let client = network_client()?;
        Ok(Self {
            client,
            trusted_hosts: [
                "dictionary.cambridge.org",
                "t.moedict.tw",
                "203146b5091e8f0aafda-15d8553a928a30eef40a64ebd36ed408.ssl.cf2.rackcdn.com",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            max_bytes: MAX_AUDIO_BYTES,
        })
    }

    fn trusted_url(&self, value: &str) -> Result<reqwest::Url, AudioError> {
        let url =
            reqwest::Url::parse(value).map_err(|error| AudioError::Invalid(error.to_string()))?;
        if url.scheme() != "https"
            || !url
                .host_str()
                .is_some_and(|host| self.trusted_hosts.contains(host))
        {
            return Err(AudioError::Invalid("untrusted dictionary audio URL".into()));
        }
        Ok(url)
    }
}

impl DictionaryAudioPort for HttpAudioFetcher {
    fn fetch<'a>(&'a self, url: &'a str) -> AudioFuture<'a> {
        Box::pin(async move {
            let url = self.trusted_url(url)?;
            let response = self
                .client
                .get(url)
                .send()
                .await
                .map_err(|error| AudioError::Unavailable(error.to_string()))?;
            if response.status().is_redirection() {
                return Err(AudioError::Invalid("audio redirect rejected".into()));
            }
            let mut response = response
                .error_for_status()
                .map_err(|error| AudioError::Unavailable(error.to_string()))?;
            self.trusted_url(response.url().as_str())?;
            if response
                .content_length()
                .is_some_and(|size| size > self.max_bytes as u64)
            {
                return Err(AudioError::Invalid("dictionary audio is too large".into()));
            }
            let mime = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .unwrap_or("audio/mpeg")
                .to_ascii_lowercase();
            if !mime.starts_with("audio/") && mime != "application/octet-stream" {
                return Err(AudioError::Invalid(format!(
                    "dictionary returned {mime}, not audio"
                )));
            }
            let mut data = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| AudioError::Unavailable(error.to_string()))?
            {
                if data.len().saturating_add(chunk.len()) > self.max_bytes {
                    return Err(AudioError::Invalid("dictionary audio is too large".into()));
                }
                data.extend_from_slice(&chunk);
            }
            if data.is_empty() {
                return Err(AudioError::Invalid(
                    "dictionary returned empty audio".into(),
                ));
            }
            Ok(AudioClip {
                data,
                mime: if mime == "application/octet-stream" {
                    "audio/mpeg".into()
                } else {
                    mime
                },
                source: "dictionary".into(),
            })
        })
    }
}

#[derive(Clone, Debug)]
pub struct GoogleTts {
    client: reqwest::Client,
    endpoint: reqwest::Url,
}

impl GoogleTts {
    pub fn new() -> Result<Self, AudioError> {
        Ok(Self {
            client: network_client()?,
            endpoint: reqwest::Url::parse("https://translate.google.com/translate_tts")
                .map_err(|error| AudioError::Invalid(error.to_string()))?,
        })
    }
}

impl TtsPort for GoogleTts {
    fn synthesize<'a>(&'a self, text: &'a str, voice: &'a Voice) -> AudioFuture<'a> {
        Box::pin(async move {
            let text = text.trim();
            if text.is_empty() || text.chars().count() > 200 {
                return Err(AudioError::Invalid(
                    "remote TTS text must contain 1-200 characters".into(),
                ));
            }
            let response = self
                .client
                .get(self.endpoint.clone())
                .query(&[
                    ("ie", "UTF-8"),
                    ("client", "tw-ob"),
                    ("tl", voice.id.as_str()),
                    ("q", text),
                ])
                .send()
                .await
                .map_err(|error| AudioError::Unavailable(error.to_string()))?;
            if response.status().is_redirection() {
                return Err(AudioError::Invalid("remote TTS redirect rejected".into()));
            }
            let mut response = response
                .error_for_status()
                .map_err(|error| AudioError::Unavailable(error.to_string()))?;
            if !response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.to_ascii_lowercase().starts_with("audio/"))
            {
                return Err(AudioError::Invalid(
                    "remote TTS returned non-audio content".into(),
                ));
            }
            if response
                .content_length()
                .is_some_and(|size| size > MAX_AUDIO_BYTES as u64)
            {
                return Err(AudioError::Invalid("remote TTS audio is too large".into()));
            }
            let mut data = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| AudioError::Unavailable(error.to_string()))?
            {
                if data.len().saturating_add(chunk.len()) > MAX_AUDIO_BYTES {
                    return Err(AudioError::Invalid("remote TTS audio is too large".into()));
                }
                data.extend_from_slice(&chunk);
            }
            if data.is_empty() {
                return Err(AudioError::Invalid(
                    "remote TTS returned invalid audio".into(),
                ));
            }
            Ok(AudioClip {
                data,
                mime: "audio/mpeg".into(),
                source: "Google TTS".into(),
            })
        })
    }
}

fn network_client() -> Result<reqwest::Client, AudioError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("LinguistAnkiBridge/0.1")
        .build()
        .map_err(|error| AudioError::Unavailable(error.to_string()))
}

#[derive(Clone, Debug)]
pub struct EspeakTts {
    binary: String,
}

impl Default for EspeakTts {
    fn default() -> Self {
        Self::new("espeak-ng")
    }
}

impl EspeakTts {
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl TtsPort for EspeakTts {
    fn synthesize<'a>(&'a self, text: &'a str, voice: &'a Voice) -> AudioFuture<'a> {
        let binary = self.binary.clone();
        let text = text.to_owned();
        let voice = voice.clone();
        Box::pin(async move {
            let (output, voice_id) = tokio::task::spawn_blocking(move || {
                Command::new(binary)
                    .args(["--stdout", "-v", &voice.id, "--", &text])
                    .output()
                    .map(|output| (output, voice.id))
            })
            .await
            .map_err(|error| AudioError::Unavailable(error.to_string()))?
            .map_err(|error| AudioError::Unavailable(error.to_string()))?;
            if !output.status.success() {
                return Err(AudioError::Unavailable(
                    String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                ));
            }
            if output.stdout.len() < 12 || &output.stdout[..4] != b"RIFF" {
                return Err(AudioError::Invalid(
                    "espeak-ng returned invalid WAV audio".into(),
                ));
            }
            Ok(AudioClip {
                data: output.stdout,
                mime: "audio/wav".into(),
                source: format!("espeak-ng · {voice_id}"),
            })
        })
    }
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
pub struct PreparedPronunciation {
    pub pronunciation: Pronunciation,
    pub filename: String,
    pub clip: AudioClip,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AudioDiscovery {
    pub clips: Vec<PreparedPronunciation>,
    pub issues: Vec<String>,
    pub cancelled: bool,
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

pub async fn discover_audio<D, L, R>(
    dictionary: &D,
    local_tts: &L,
    remote_tts: &R,
    pronunciations: impl IntoIterator<Item = Pronunciation>,
    voices: &[Voice],
    cancelled: Arc<AtomicBool>,
) -> AudioDiscovery
where
    D: DictionaryAudioPort + ?Sized,
    L: TtsPort + ?Sized,
    R: TtsPort + ?Sized,
{
    let mut result = AudioDiscovery::default();
    for pronunciation in usable_pronunciations(pronunciations) {
        if cancelled.load(Ordering::Relaxed) {
            result.cancelled = true;
            break;
        }
        let clip = if let Some(url) = pronunciation.audio_url.as_deref() {
            match dictionary.fetch(url).await {
                Ok(clip) => Some(clip),
                Err(error) => {
                    result
                        .issues
                        .push(format!("Dictionary audio {}: {error}", pronunciation.text));
                    synthesize_with_fallback(
                        local_tts,
                        remote_tts,
                        &pronunciation,
                        voices,
                        &mut result.issues,
                    )
                    .await
                }
            }
        } else {
            synthesize_with_fallback(
                local_tts,
                remote_tts,
                &pronunciation,
                voices,
                &mut result.issues,
            )
            .await
        };
        let Some(clip) = clip else { continue };
        if clip.data.is_empty() || !clip.mime.starts_with("audio/") {
            result
                .issues
                .push(format!("Invalid audio returned for {}", pronunciation.text));
            continue;
        }
        let filename = media_filename_for_mime(
            &pronunciation.text,
            &pronunciation.locale,
            result.clips.len(),
            &clip.mime,
        );
        result.clips.push(PreparedPronunciation {
            pronunciation,
            filename,
            clip,
        });
    }
    result
}

async fn synthesize_with_fallback<L: TtsPort + ?Sized, R: TtsPort + ?Sized>(
    local_tts: &L,
    remote_tts: &R,
    pronunciation: &Pronunciation,
    voices: &[Voice],
    issues: &mut Vec<String>,
) -> Option<AudioClip> {
    if let Some(voice) = select_voice(voices, &pronunciation.locale) {
        match local_tts.synthesize(&pronunciation.text, voice).await {
            Ok(clip) => return Some(clip),
            Err(error) => issues.push(format!("Local TTS {}: {error}", pronunciation.text)),
        }
    }
    let Some(voice) = select_remote_voice(voices, &pronunciation.locale) else {
        issues.push(format!("No TTS voice for {}", pronunciation.locale));
        return None;
    };
    match remote_tts.synthesize(&pronunciation.text, voice).await {
        Ok(clip) => Some(clip),
        Err(error) => {
            issues.push(format!("Remote TTS {}: {error}", pronunciation.text));
            None
        }
    }
}

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
pub fn select_remote_voice<'a>(voices: &'a [Voice], locale: &str) -> Option<&'a Voice> {
    select_by_locale(voices.iter().filter(|voice| !voice.local), locale)
}
fn select_by_locale<'a>(
    voices: impl Iterator<Item = &'a Voice> + Clone,
    locale: &str,
) -> Option<&'a Voice> {
    let language = locale.split('-').next().unwrap_or(locale);
    voices
        .clone()
        .find(|voice| voice.locale.eq_ignore_ascii_case(locale))
        .or_else(|| {
            voices.clone().find(|voice| {
                voice
                    .locale
                    .split('-')
                    .next()
                    .unwrap_or(&voice.locale)
                    .eq_ignore_ascii_case(language)
            })
        })
        .or_else(|| voices.into_iter().next())
}
pub fn media_filename(expression: &str, locale: &str, ordinal: usize) -> String {
    media_filename_for_mime(expression, locale, ordinal, "audio/mpeg")
}
pub fn media_filename_for_mime(
    expression: &str,
    locale: &str,
    ordinal: usize,
    mime: &str,
) -> String {
    let extension = match mime {
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/ogg" => "ogg",
        _ => "mp3",
    };
    format!(
        "audio-{}-{}-{:08x}-{:02}.{extension}",
        stem(expression),
        stem(locale),
        stable_hash(expression.as_bytes().iter().copied()),
        ordinal + 1
    )
}
pub fn cache_key(provider_revision: &str, text: &str, voice: &Voice) -> String {
    let hash = stable_hash(
        provider_revision
            .bytes()
            .chain([0])
            .chain(text.trim().bytes())
            .chain([0])
            .chain(voice.id.bytes()),
    );
    format!("audio-v1-{hash:016x}")
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
fn stable_hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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
    fn dictionary_fetcher_rejects_untrusted_audio_urls() {
        let fetcher = HttpAudioFetcher::dictionary_defaults().unwrap();
        assert!(
            fetcher
                .trusted_url("https://dictionary.cambridge.org/media.mp3")
                .is_ok()
        );
        assert!(
            fetcher
                .trusted_url("http://dictionary.cambridge.org/media.mp3")
                .is_err()
        );
        assert!(
            fetcher
                .trusted_url("https://dictionary.cambridge.org.evil.test/media.mp3")
                .is_err()
        );
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
        let filename = media_filename("食べる", "ja-JP", 1);
        assert!(filename.starts_with("audio-term-ja-jp-"));
        assert!(filename.ends_with("-02.mp3"));
    }

    struct MissingDictionary;
    impl DictionaryAudioPort for MissingDictionary {
        fn fetch<'a>(&'a self, _: &'a str) -> AudioFuture<'a> {
            Box::pin(async { Err(AudioError::Unavailable("offline".into())) })
        }
    }
    struct LocalFailure;
    impl TtsPort for LocalFailure {
        fn synthesize<'a>(&'a self, _: &'a str, _: &'a Voice) -> AudioFuture<'a> {
            Box::pin(async { Err(AudioError::Unavailable("no local engine".into())) })
        }
    }
    struct RemoteSuccess;
    impl TtsPort for RemoteSuccess {
        fn synthesize<'a>(&'a self, _: &'a str, voice: &'a Voice) -> AudioFuture<'a> {
            Box::pin(async move {
                Ok(AudioClip {
                    data: vec![1, 2, 3],
                    mime: "audio/mpeg".into(),
                    source: voice.id.clone(),
                })
            })
        }
    }

    #[tokio::test]
    async fn falls_back_from_dictionary_and_local_to_remote_tts() {
        let pronunciation = Pronunciation {
            text: "食べる".into(),
            locale: "ja-JP".into(),
            audio_url: Some("https://audio.test/taberu.mp3".into()),
            source: "dictionary".into(),
        };
        let voices = vec![
            Voice {
                id: "local-ja".into(),
                locale: "ja-JP".into(),
                local: true,
            },
            Voice {
                id: "remote-ja".into(),
                locale: "ja-JP".into(),
                local: false,
            },
        ];
        let result = discover_audio(
            &MissingDictionary,
            &LocalFailure,
            &RemoteSuccess,
            [pronunciation],
            &voices,
            Arc::new(AtomicBool::new(false)),
        )
        .await;
        assert_eq!(result.clips.len(), 1);
        assert_eq!(result.clips[0].clip.source, "remote-ja");
        assert_eq!(result.issues.len(), 2);
    }

    #[tokio::test]
    async fn cancellation_stops_before_audio_providers() {
        let result = discover_audio(
            &MissingDictionary,
            &LocalFailure,
            &RemoteSuccess,
            [Pronunciation {
                text: "term".into(),
                locale: "en-US".into(),
                audio_url: None,
                source: "test".into(),
            }],
            &[],
            Arc::new(AtomicBool::new(true)),
        )
        .await;
        assert!(result.cancelled);
        assert!(result.clips.is_empty());
    }

    #[tokio::test]
    async fn local_tts_reports_missing_binary_without_panicking() {
        let error = EspeakTts::new("linguist-missing-espeak-binary")
            .synthesize(
                "食べる",
                &Voice {
                    id: "ja".into(),
                    locale: "ja-JP".into(),
                    local: true,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, AudioError::Unavailable(_)));
    }
}

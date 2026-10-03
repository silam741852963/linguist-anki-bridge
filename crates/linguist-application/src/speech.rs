//! Local speech synthesis through Piper (`audio.provider=piper`).
//!
//! Text reaches the child on stdin, never as an argument. The voice model and
//! its JSON config are hashed as provenance, the voice language must match the
//! target language, and the output is decoded under the media limits. Results
//! are review candidates, never accepted pronunciation facts.
use linguist_config::Effective;
use linguist_core::Language;
use linguist_provider::{
    Gates, Service,
    process::{ProcessError, ProcessLimits, TempDir, command, hash_file, run, run_with_input},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SETTINGS: [&str; 10] = [
    "audio.provider",
    "audio.voice",
    "audio.executable",
    "audio.voice_resource",
    "audio.speed",
    "audio.timeout_seconds",
    "services.tts.concurrency",
    "helpers.memory_limit_mb",
    "helpers.max_output_mb",
    "storage.temp_dir",
];
pub const MAX_TEXT_CHARS: usize = 1000;
const VOICE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const CONFIG_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpeechError {
    InvalidSettings {
        message: String,
    },
    SpeechDisabled,
    /// The selected provider has no adapter in this build.
    SpeechProviderUnavailable {
        provider: String,
    },
    SpeechExecutableUnavailable {
        status: String,
    },
    SpeechVoiceUnavailable {
        status: String,
    },
    SpeechVoiceConfigInvalid,
    SpeechVoiceLanguageMismatch {
        voice: String,
        target: String,
    },
    SpeechSpeakerInvalid,
    SpeechTextInvalid,
    SpeechWorkspaceFailed,
    SpeechProcessTimeout,
    SpeechProcessFailed {
        exit_code: Option<i32>,
    },
    SpeechOutputLimit,
    SpeechOutputInvalid {
        audio_code: String,
    },
}
impl SpeechError {
    pub fn code(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v["code"].as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}
impl std::fmt::Display for SpeechError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.code())
    }
}
impl std::error::Error for SpeechError {}
impl From<ProcessError> for SpeechError {
    fn from(error: ProcessError) -> Self {
        match error {
            ProcessError::SpawnFailed => Self::SpeechProcessFailed { exit_code: None },
            ProcessError::Timeout => Self::SpeechProcessTimeout,
            ProcessError::Failed { exit_code } => Self::SpeechProcessFailed { exit_code },
            ProcessError::OutputLimit => Self::SpeechOutputLimit,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Synthesis {
    pub provider: &'static str,
    pub engine_version: String,
    pub executable_sha256: String,
    pub voice_sha256: String,
    pub voice_config_sha256: String,
    pub voice_language: String,
    pub voice_dataset: Option<String>,
    pub speaker: Option<String>,
    pub length_scale: f64,
    pub text_sha256: String,
    pub mime: String,
    pub sample_rate: u32,
    pub sha256: String,
    pub size_bytes: u64,
    /// Pronunciation and voice suitability always need review.
    pub review_required: bool,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

struct Voice {
    model: PathBuf,
    config: PathBuf,
    language: String,
    dataset: Option<String>,
    speaker: Option<String>,
}

fn voice(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    target: Option<&Language>,
) -> Result<Voice, SpeechError> {
    let unavailable = |status: &str| SpeechError::SpeechVoiceUnavailable {
        status: status.into(),
    };
    let reference = settings.values["audio.voice_resource"]
        .as_str()
        .ok_or_else(|| unavailable("unset"))?;
    let model = linguist_config::expand_path(reference, environment)
        .map_err(|_| unavailable("invalid_path_expansion"))?;
    if !model.is_absolute() {
        return Err(unavailable("relative_path"));
    }
    let config = PathBuf::from(format!("{}.json", model.display()));
    for path in [&model, &config] {
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => return Err(unavailable("symlink")),
            Ok(m) if m.is_file() => {}
            Ok(_) => return Err(unavailable("wrong_file_type")),
            Err(_) => return Err(unavailable("missing")),
        }
    }
    let metadata = std::fs::metadata(&config).map_err(|_| unavailable("missing"))?;
    if metadata.len() > CONFIG_LIMIT {
        return Err(SpeechError::SpeechVoiceConfigInvalid);
    }
    let parsed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config).map_err(|_| unavailable("unreadable"))?)
            .map_err(|_| SpeechError::SpeechVoiceConfigInvalid)?;
    let language = parsed["language"]["code"]
        .as_str()
        .ok_or(SpeechError::SpeechVoiceConfigInvalid)?
        .to_owned();
    let voice_primary = language
        .split(['_', '-'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if let Some(target) = target {
        let target_primary = target
            .as_str()
            .split('-')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if voice_primary != target_primary {
            return Err(SpeechError::SpeechVoiceLanguageMismatch {
                voice: language,
                target: target.as_str().into(),
            });
        }
    }
    let dataset = parsed["dataset"].as_str().map(str::to_owned);
    let speakers = parsed["num_speakers"].as_u64().unwrap_or(1);
    let requested = settings.values["audio.voice"].as_str();
    let speaker = if speakers > 1 {
        // Multi-speaker voices need an explicit named speaker from the config.
        let name = requested.ok_or(SpeechError::SpeechSpeakerInvalid)?;
        let id = parsed["speaker_id_map"][name]
            .as_u64()
            .ok_or(SpeechError::SpeechSpeakerInvalid)?;
        Some(id.to_string())
    } else {
        let stem = model
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if requested.is_some_and(|name| Some(name) != dataset.as_deref() && name != stem) {
            return Err(SpeechError::SpeechSpeakerInvalid);
        }
        None
    };
    Ok(Voice {
        model,
        config,
        language,
        dataset,
        speaker,
    })
}

/// Validate provider selection and resources without synthesizing. Without a
/// target the voice language is not compared (diagnostics only).
pub fn preflight(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    target: Option<&Language>,
) -> Result<(), SpeechError> {
    let registry = linguist_config::Registry::builtin();
    for key in SETTINGS {
        registry
            .validate_value(
                key,
                settings
                    .values
                    .get(key)
                    .ok_or(SpeechError::SpeechDisabled)?,
            )
            .map_err(|message| SpeechError::InvalidSettings { message })?;
    }
    match settings.values["audio.provider"].as_str() {
        Some("piper") => {}
        Some("preserve" | "disabled") => return Err(SpeechError::SpeechDisabled),
        other => {
            return Err(SpeechError::SpeechProviderUnavailable {
                provider: other.unwrap_or_default().into(),
            });
        }
    }
    let reference = settings.values["audio.executable"].as_str().ok_or(
        SpeechError::SpeechExecutableUnavailable {
            status: "unset".into(),
        },
    )?;
    linguist_config::resources::resolve_executable(reference, environment).map_err(|status| {
        SpeechError::SpeechExecutableUnavailable {
            status: status.into(),
        }
    })?;
    voice(settings, environment, target).map(|_| ())
}

/// Synthesize one short text in the target language.
pub fn synthesize(
    text: &str,
    target: &Language,
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<Synthesis, SpeechError> {
    preflight(settings, environment, Some(target))?;
    let text = text.trim();
    if text.is_empty()
        || text.chars().count() > MAX_TEXT_CHARS
        || text.chars().any(|c| c.is_control() && c != '\n')
    {
        return Err(SpeechError::SpeechTextInvalid);
    }
    let v = &settings.values;
    let executable = linguist_config::resources::resolve_executable(
        v["audio.executable"].as_str().unwrap(),
        environment,
    )
    .map_err(|status| SpeechError::SpeechExecutableUnavailable {
        status: status.into(),
    })?;
    let voice = voice(settings, environment, Some(target))?;
    let timeout = Duration::from_secs(v["audio.timeout_seconds"].as_u64().unwrap());
    let deadline = Instant::now() + timeout;
    let limits = ProcessLimits {
        deadline,
        output: v["helpers.max_output_mb"].as_u64().unwrap() as usize * 1024 * 1024,
        memory: v["helpers.memory_limit_mb"].as_u64().unwrap() * 1024 * 1024,
    };
    let _permit = Gates::global()
        .service(Service::Tts)
        .acquire(
            v["services.tts.concurrency"].as_u64().unwrap() as usize,
            deadline,
        )
        .map_err(|_| SpeechError::SpeechProcessTimeout)?;
    let hash = |path: &Path, limit| hash_file(path, limit).map(|h| h.0);
    let executable_sha256 =
        hash(&executable, u64::MAX).map_err(|_| SpeechError::SpeechExecutableUnavailable {
            status: "unreadable".into(),
        })?;
    let voice_sha256 =
        hash(&voice.model, VOICE_LIMIT).map_err(|_| SpeechError::SpeechVoiceUnavailable {
            status: "unreadable".into(),
        })?;
    let voice_config_sha256 =
        hash(&voice.config, CONFIG_LIMIT).map_err(|_| SpeechError::SpeechVoiceUnavailable {
            status: "unreadable".into(),
        })?;
    let mut version = command(&executable);
    version.arg("--version");
    let engine_version = String::from_utf8_lossy(&run(version, &limits)?)
        .trim()
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    if engine_version.is_empty() {
        return Err(SpeechError::SpeechProcessFailed { exit_code: Some(0) });
    }
    let temp_root =
        linguist_config::expand_path(v["storage.temp_dir"].as_str().unwrap(), environment)
            .ok()
            .filter(|p| p.is_absolute())
            .ok_or(SpeechError::SpeechWorkspaceFailed)?;
    let workspace =
        TempDir::create_in(&temp_root).map_err(|_| SpeechError::SpeechWorkspaceFailed)?;
    let output = workspace.0.join("speech.wav");
    let length_scale = 1.0 / v["audio.speed"].as_f64().unwrap();
    let mut synthesize = command(&executable);
    synthesize
        .arg("--model")
        .arg(&voice.model)
        .arg("--config")
        .arg(&voice.config)
        .arg("--output_file")
        .arg(&output)
        .arg("--length_scale")
        .arg(format!("{length_scale:.4}"));
    if let Some(speaker) = &voice.speaker {
        synthesize.arg("--speaker").arg(speaker);
    }
    run_with_input(synthesize, Some(format!("{text}\n").into_bytes()), &limits)?;
    let cap = v["media.max_asset_mb"].as_u64().unwrap_or(10) * 1024 * 1024;
    let metadata =
        std::fs::symlink_metadata(&output).map_err(|_| SpeechError::SpeechOutputInvalid {
            audio_code: "AUDIO_OUTPUT_MISSING".into(),
        })?;
    if !metadata.is_file() || metadata.len() > cap {
        return Err(SpeechError::SpeechOutputLimit);
    }
    let bytes = std::fs::read(&output).map_err(|_| SpeechError::SpeechWorkspaceFailed)?;
    drop(workspace);
    let inspection = crate::audio::inspect_audio(&bytes, settings).map_err(|e| {
        SpeechError::SpeechOutputInvalid {
            audio_code: e.to_string(),
        }
    })?;
    Ok(Synthesis {
        provider: "piper",
        engine_version,
        executable_sha256,
        voice_sha256,
        voice_config_sha256,
        voice_language: voice.language,
        voice_dataset: voice.dataset,
        speaker: voice.speaker,
        length_scale,
        text_sha256: linguist_provider::sha256_hex(text.as_bytes()),
        mime: inspection.mime,
        sample_rate: inspection.sample_rate,
        sha256: linguist_provider::sha256_hex(&bytes),
        size_bytes: bytes.len() as u64,
        review_required: true,
        bytes,
    })
}

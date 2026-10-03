use linguist_application::speech::{SpeechError, synthesize};
use linguist_core::Language;
use serde_json::json;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, path::PathBuf};

fn wave(samples: usize) -> Vec<u8> {
    let size = (samples * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((size + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(22050u32.to_le_bytes());
    bytes.extend(44100u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    bytes.resize(44 + size as usize, 0);
    bytes
}

struct Fixture {
    root: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    /// Fake Piper: records argv and stdin, then copies a fixed WAV to --output_file.
    fn new(body: &str, config: serde_json::Value) -> Self {
        let root = std::env::temp_dir().join(format!("lab-piper-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("fixed.wav"), wave(2205)).unwrap();
        std::fs::write(root.join("voice.onnx"), b"model").unwrap();
        std::fs::write(
            root.join("voice.onnx.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let script = format!(
            "#!/bin/sh\n[ \"$1\" = --version ] && {{ echo 'piper 1.2.0-fake'; exit 0; }}\n\
             printf '%s\\n' \"$@\" > {root}/args\ncat > {root}/stdin\n\
             out=''; prev=''; for a in \"$@\"; do [ \"$prev\" = --output_file ] && out=\"$a\"; prev=\"$a\"; done\n{body}\n",
            root = root.display()
        );
        let path = root.join("piper");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        while let Err(error) = std::process::Command::new(&path).arg("--version").output() {
            assert_eq!(error.raw_os_error(), Some(libc::ETXTBSY));
        }
        Self { root }
    }
    fn settings(&self) -> linguist_config::Effective {
        let mut settings = linguist_config::resolve(
            &linguist_config::Registry::builtin(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        for (key, value) in [
            ("audio.provider", json!("piper")),
            ("audio.executable", json!(self.root.join("piper"))),
            ("audio.voice_resource", json!(self.root.join("voice.onnx"))),
            ("audio.timeout_seconds", json!(2)),
            ("storage.temp_dir", json!(self.root.join("tmp"))),
        ] {
            settings.values.insert(key.into(), value);
        }
        settings
    }
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.root.join(name)).unwrap()
    }
}

const COPY: &str = "/bin/cp \"$(dirname \"$0\")/fixed.wav\" \"$out\"";

fn en() -> Language {
    "en".to_owned().try_into().unwrap()
}
fn env() -> BTreeMap<String, String> {
    BTreeMap::new()
}

#[test]
fn piper_receives_text_on_stdin_and_output_is_decoded_with_provenance() {
    let fixture = Fixture::new(
        COPY,
        json!({"language": {"code": "en_US"}, "dataset": "lessac", "num_speakers": 1}),
    );
    let mut settings = fixture.settings();
    settings.values.insert("audio.speed".into(), json!(1.25));
    settings
        .values
        .insert("audio.voice".into(), json!("lessac"));
    let text = "--model /etc/passwd; $(rm -rf /)";
    let result = synthesize(text, &en(), &settings, &env()).unwrap();
    assert_eq!(fixture.read("stdin"), format!("{text}\n"));
    let args: Vec<String> = fixture.read("args").lines().map(str::to_owned).collect();
    assert_eq!(args[0], "--model");
    assert!(args[1].ends_with("voice.onnx"));
    assert_eq!(args[2..3], ["--config".to_owned()]);
    assert_eq!(
        args[6..8],
        ["--length_scale".to_owned(), "0.8000".to_owned()]
    );
    assert!(!args.iter().any(|a| a.contains("passwd")));
    assert_eq!(result.mime, "audio/wav");
    assert_eq!(result.sample_rate, 22050);
    assert_eq!(result.engine_version, "piper 1.2.0-fake");
    assert_eq!(result.voice_language, "en_US");
    assert_eq!(result.voice_sha256.len(), 64);
    assert!(result.review_required);
    assert_eq!(result.bytes, wave(2205));
    // The temporary output directory is removed.
    assert_eq!(
        std::fs::read_dir(fixture.root.join("tmp")).unwrap().count(),
        0
    );
}

#[test]
fn multi_speaker_voices_require_a_named_speaker() {
    let fixture = Fixture::new(
        COPY,
        json!({"language": {"code": "en_GB"}, "num_speakers": 3, "speaker_id_map": {"alba": 0, "jenny": 2}}),
    );
    let mut settings = fixture.settings();
    assert_eq!(
        synthesize("hi", &en(), &settings, &env()).unwrap_err(),
        SpeechError::SpeechSpeakerInvalid
    );
    settings.values.insert("audio.voice".into(), json!("jenny"));
    let result = synthesize("hi", &en(), &settings, &env()).unwrap();
    assert_eq!(result.speaker.as_deref(), Some("2"));
    let args = fixture.read("args");
    assert!(args.ends_with("--speaker\n2\n"));
    settings
        .values
        .insert("audio.voice".into(), json!("nobody"));
    assert_eq!(
        synthesize("hi", &en(), &settings, &env()).unwrap_err(),
        SpeechError::SpeechSpeakerInvalid
    );
}

#[test]
fn voice_language_resources_and_provider_selection_are_checked_first() {
    let fixture = Fixture::new(COPY, json!({"language": {"code": "vi_VN"}}));
    let settings = fixture.settings();
    assert_eq!(
        synthesize("xin chào", &en(), &settings, &env()).unwrap_err(),
        SpeechError::SpeechVoiceLanguageMismatch {
            voice: "vi_VN".into(),
            target: "en".into()
        }
    );
    assert!(!fixture.root.join("args").exists());
    let vi: Language = "vi".to_owned().try_into().unwrap();
    synthesize("xin chào", &vi, &settings, &env()).unwrap();
    for (key, value, expected) in [
        (
            "audio.provider",
            json!("preserve"),
            SpeechError::SpeechDisabled,
        ),
        (
            "audio.provider",
            json!("disabled"),
            SpeechError::SpeechDisabled,
        ),
        (
            "audio.provider",
            json!("dictionary"),
            SpeechError::SpeechProviderUnavailable {
                provider: "dictionary".into(),
            },
        ),
        (
            "audio.provider",
            json!("custom"),
            SpeechError::SpeechProviderUnavailable {
                provider: "custom".into(),
            },
        ),
        (
            "audio.voice_resource",
            json!("/nonexistent/voice.onnx"),
            SpeechError::SpeechVoiceUnavailable {
                status: "missing".into(),
            },
        ),
        (
            "audio.executable",
            json!("/nonexistent/piper"),
            SpeechError::SpeechExecutableUnavailable {
                status: "missing".into(),
            },
        ),
    ] {
        let mut changed = settings.clone();
        changed.values.insert(key.into(), value);
        assert_eq!(
            synthesize("xin chào", &vi, &changed, &env()).unwrap_err(),
            expected,
            "{key}"
        );
    }
    for text in ["", "   ", "a\u{7}b"] {
        assert_eq!(
            synthesize(text, &vi, &settings, &env()).unwrap_err(),
            SpeechError::SpeechTextInvalid
        );
    }
    assert_eq!(
        synthesize(&"a".repeat(1001), &vi, &settings, &env()).unwrap_err(),
        SpeechError::SpeechTextInvalid
    );
}

#[test]
fn process_faults_and_invalid_output_are_typed() {
    let config = json!({"language": {"code": "en_US"}});
    let fixture = Fixture::new("while :; do :; done", config.clone());
    let mut settings = fixture.settings();
    settings
        .values
        .insert("audio.timeout_seconds".into(), json!(1));
    assert_eq!(
        synthesize("hi", &en(), &settings, &env()).unwrap_err(),
        SpeechError::SpeechProcessTimeout
    );
    let fixture = Fixture::new("exit 4", config.clone());
    assert_eq!(
        synthesize("hi", &en(), &fixture.settings(), &env()).unwrap_err(),
        SpeechError::SpeechProcessFailed { exit_code: Some(4) }
    );
    let fixture = Fixture::new("exit 0", config.clone());
    assert!(matches!(
        synthesize("hi", &en(), &fixture.settings(), &env()).unwrap_err(),
        SpeechError::SpeechOutputInvalid { .. }
    ));
    let fixture = Fixture::new("echo 'not audio' > \"$out\"", config);
    assert!(matches!(
        synthesize("hi", &en(), &fixture.settings(), &env()).unwrap_err(),
        SpeechError::SpeechOutputInvalid { .. }
    ));
}

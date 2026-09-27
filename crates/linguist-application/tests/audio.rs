use linguist_application::{audio::*, media::inspect_source_media};
fn settings() -> linguist_config::Effective {
    linguist_config::resolve(
        &linguist_config::Registry::builtin(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}
/// Deterministic mono signed-16 PCM fixture; test content has no third-party rights.
fn wave(samples: usize) -> Vec<u8> {
    let size = (samples * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((size + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(32000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    bytes.resize(44 + size as usize, 0);
    bytes
}

#[test]
fn decoded_audio_formats_report_scope_and_enforce_audio_policy() {
    let mut config = settings();
    let pcm = wave(1600);
    for (bytes, mime, complete) in [
        (pcm.as_slice(), "audio/wav", true),
        (
            include_bytes!("fixtures/audio/tone.mp3").as_slice(),
            "audio/mpeg",
            true,
        ),
        (
            include_bytes!("fixtures/audio/tone.ogg").as_slice(),
            "audio/ogg",
            true,
        ),
    ] {
        let inspection = inspect_audio(bytes, &config).unwrap();
        assert_eq!(inspection.mime, mime);
        assert_eq!(inspection.sample_rate, 16000);
        assert_eq!(inspection.channels, 1);
        assert!(inspection.decoded_frames > 0);
        assert!(inspection.stream_end_observed);
        assert_eq!(inspection.container_extent_verified, complete);
        let media = inspect_source_media(bytes, &config).unwrap();
        assert_eq!(media.mime(), mime);
        assert_eq!(media.requires_audio_completeness_review(), !complete);
    }
    config.values.insert(
        "media.allowed_audio_types".into(),
        serde_json::json!(["audio/mpeg"]),
    );
    assert_eq!(
        inspect_audio(&pcm, &config).unwrap_err(),
        AudioInspectionError::AudioFormatDisallowed
    );
    assert_eq!(
        inspect_source_media(&pcm, &config).unwrap_err().code,
        "AUDIO_FORMAT_DISALLOWED"
    );
}

#[test]
fn mpeg_versions_and_variable_bitrate_decode_with_verified_frame_extents() {
    for (bytes, rate, channels) in [
        (
            include_bytes!("fixtures/audio/tone-vbr.mp3").as_slice(),
            44100,
            2,
        ),
        (
            include_bytes!("fixtures/audio/tone-low-rate.mp3").as_slice(),
            8000,
            1,
        ),
    ] {
        let inspection = inspect_audio(bytes, &settings()).unwrap();
        assert_eq!(inspection.sample_rate, rate);
        assert_eq!(inspection.channels, channels);
        assert!(inspection.container_extent_verified);
        assert!(inspection.decoded_frames > 0);
        assert!(inspect_audio(&bytes[..bytes.len() - 1], &settings()).is_err());
    }
}

#[test]
fn wave_extent_and_decoded_budget_reject_corruption_without_success_receipts() {
    let mut config = settings();
    let bytes = wave(1600);
    let inspection = inspect_audio(&bytes, &config).unwrap();
    assert_eq!(inspection.decoded_frames, 1600);
    assert_eq!(inspection.decoded_sample_budget_bytes, 1600 * 8);
    for mutated in [
        bytes[..bytes.len() - 1].to_vec(),
        {
            let mut b = bytes.clone();
            b.push(0);
            b
        },
        {
            let mut b = bytes.clone();
            b[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
            b
        },
        {
            // RIFF extent and odd-byte padding are valid, but a partial PCM
            // sample cannot be certified by silently dropping the last byte.
            let mut b = bytes.clone();
            b[40..44].copy_from_slice(&3199u32.to_le_bytes());
            b
        },
    ] {
        assert!(inspect_audio(&mutated, &config).is_err());
    }
    config
        .values
        .insert("media.max_asset_mb".into(), serde_json::json!(1));
    // Encoded PCM fits the input limit but decoded eight-byte sample budget does not.
    assert_eq!(
        inspect_audio(&wave(140000), &config).unwrap_err(),
        AudioInspectionError::AudioDecodedLimit
    );
    assert_eq!(
        inspect_audio(&[], &config).unwrap_err(),
        AudioInspectionError::AudioInputLimit
    );
    config
        .values
        .insert("media.allowed_audio_types".into(), serde_json::json!(42));
    assert_eq!(
        inspect_audio(&bytes, &config).unwrap_err(),
        AudioInspectionError::AudioInvalidSettings
    );
}

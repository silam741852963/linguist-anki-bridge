//! Structural extent checks for indexed-bitrate MPEG Layer III streams.
//! This cannot detect removal of whole frames without a trusted external length.
use super::AudioInspectionError;

pub(super) fn validate_extent(bytes: &[u8]) -> Result<(), AudioInspectionError> {
    use AudioInspectionError::*;
    let mut offset = 0usize;
    if bytes.starts_with(b"ID3") {
        let header = bytes.get(..10).ok_or(AudioContainerInvalid)?;
        let allowed_flags = match header[3] {
            2 => 0xc0,
            3 => 0xe0,
            4 => 0xf0,
            _ => return Err(AudioContainerInvalid),
        };
        if header[4] == 255
            || header[5] & !allowed_flags != 0
            || header[6..10].iter().any(|b| b & 128 != 0)
        {
            return Err(AudioContainerInvalid);
        }
        let size = header[6..10]
            .iter()
            .fold(0usize, |size, byte| (size << 7) | usize::from(*byte));
        offset = 10usize.checked_add(size).ok_or(AudioContainerInvalid)?;
        if offset > bytes.len() {
            return Err(AudioContainerInvalid);
        }
        if header[3] == 4 && header[5] & 0x10 != 0 {
            let footer = bytes
                .get(offset..offset.checked_add(10).ok_or(AudioContainerInvalid)?)
                .ok_or(AudioContainerInvalid)?;
            if &footer[..3] != b"3DI" || footer[3..] != header[3..] {
                return Err(AudioContainerInvalid);
            }
            offset += 10;
        }
    }
    let mut end = bytes.len();
    if end >= 128 && &bytes[end - 128..end - 125] == b"TAG" {
        end -= 128;
    }
    let mut frames = 0u64;
    let mut specification = None;
    while offset < end {
        let header = bytes
            .get(offset..offset.checked_add(4).ok_or(AudioContainerInvalid)?)
            .filter(|_| offset + 4 <= end)
            .ok_or(AudioContainerInvalid)?;
        let word = u32::from_be_bytes(header.try_into().unwrap());
        let version = (word >> 19) & 3;
        let layer = (word >> 17) & 3;
        let bitrate_index = ((word >> 12) & 15) as usize;
        let rate_index = ((word >> 10) & 3) as usize;
        if word >> 21 != 0x7ff
            || version == 1
            || layer != 1
            || rate_index == 3
            || word & 3 == 2
            || bitrate_index == 15
        {
            return Err(AudioContainerInvalid);
        }
        // Free-format MP3 requires a different framing algorithm. Reject explicitly.
        if bitrate_index == 0 {
            return Err(AudioTrackUnsupported);
        }
        let bitrate = if version == 3 {
            [
                0u32, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
            ][bitrate_index]
        } else {
            [
                0u32, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160,
            ][bitrate_index]
        };
        let rate = [44100u32, 48000, 32000][rate_index]
            / match version {
                3 => 1,
                2 => 2,
                _ => 4,
            };
        let channels = if (word >> 6) & 3 == 3 { 1 } else { 2 };
        let spec = (version, rate, channels);
        if specification.is_some_and(|prior| prior != spec) {
            return Err(AudioStreamChanged);
        }
        specification = Some(spec);
        let factor = if version == 3 { 144000 } else { 72000 };
        let size = (factor * bitrate / rate + ((word >> 9) & 1)) as usize;
        offset = offset.checked_add(size).ok_or(AudioContainerInvalid)?;
        if offset > end {
            return Err(AudioContainerInvalid);
        }
        frames += 1;
    }
    if frames == 0 || offset != end {
        return Err(AudioContainerInvalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const TONE: &[u8] = include_bytes!("../../tests/fixtures/audio/tone.mp3");
    #[test]
    fn independently_encoded_fixture_passes_and_partial_tail_or_junk_fails() {
        assert!(validate_extent(TONE).is_ok());
        for removed in 1..20 {
            assert!(validate_extent(&TONE[..TONE.len() - removed]).is_err());
        }
        for extra in [vec![0], vec![255; 4]] {
            let mut changed = TONE.to_vec();
            changed.extend(extra);
            assert!(validate_extent(&changed).is_err());
        }
        let mut prefix = vec![0];
        prefix.extend(TONE);
        assert!(validate_extent(&prefix).is_err());
    }
    #[test]
    fn tags_are_bounded_and_footer_copies_must_match() {
        let mut invalid_size = TONE.to_vec();
        invalid_size[6] = 128;
        assert!(validate_extent(&invalid_size).is_err());
        let mut empty_tag = b"ID3\x04\x00\x10\x00\x00\x00\x00".to_vec();
        empty_tag.extend(b"3DI\x04\x00\x10\x00\x00\x00\x00");
        let size = TONE[6..10]
            .iter()
            .fold(0usize, |n, b| (n << 7) | usize::from(*b));
        empty_tag.extend(&TONE[10 + size..]);
        assert!(validate_extent(&empty_tag).is_ok());
        empty_tag[14] = 1;
        assert!(validate_extent(&empty_tag).is_err());
        let mut id3v1 = TONE.to_vec();
        id3v1.extend(b"TAG");
        id3v1.resize(TONE.len() + 128, 0);
        assert!(validate_extent(&id3v1).is_ok());
    }
}

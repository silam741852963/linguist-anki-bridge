//! RFC 3533 page framing for a single, complete logical stream.
use super::AudioInspectionError;

pub(super) fn validate_extent(bytes: &[u8]) -> Result<(), AudioInspectionError> {
    use AudioInspectionError::*;
    let mut offset = 0usize;
    let mut serial = None;
    let mut sequence = 0u32;
    let mut pending_packet = false;
    let mut ended = false;
    while offset < bytes.len() {
        if ended {
            return Err(AudioStreamChanged);
        }
        let header = bytes
            .get(offset..offset.checked_add(27).ok_or(AudioContainerInvalid)?)
            .ok_or(AudioContainerInvalid)?;
        if &header[..4] != b"OggS" || header[4] != 0 || header[5] & !7 != 0 {
            return Err(AudioContainerInvalid);
        }
        let flags = header[5];
        let page_serial = u32::from_le_bytes(header[14..18].try_into().unwrap());
        let page_sequence = u32::from_le_bytes(header[18..22].try_into().unwrap());
        if serial.is_none() {
            if flags & 2 == 0 || flags & 1 != 0 || page_sequence != 0 {
                return Err(AudioContainerInvalid);
            }
            serial = Some(page_serial);
        } else if serial != Some(page_serial) {
            return Err(AudioTrackUnsupported);
        } else if flags & 2 != 0 {
            return Err(AudioStreamChanged);
        }
        if page_sequence != sequence || (flags & 1 != 0) != pending_packet {
            return Err(AudioContainerInvalid);
        }
        let table_end = offset
            .checked_add(27 + usize::from(header[26]))
            .ok_or(AudioContainerInvalid)?;
        let lacing = bytes
            .get(offset + 27..table_end)
            .ok_or(AudioContainerInvalid)?;
        let payload_bytes: usize = lacing.iter().map(|n| usize::from(*n)).sum();
        let end = table_end
            .checked_add(payload_bytes)
            .ok_or(AudioContainerInvalid)?;
        let page = bytes.get(offset..end).ok_or(AudioContainerInvalid)?;
        let expected_crc = u32::from_le_bytes(header[22..26].try_into().unwrap());
        if checksum(page) != expected_crc {
            return Err(AudioContainerInvalid);
        }
        if let Some(last) = lacing.last() {
            pending_packet = *last == 255;
        }
        ended = flags & 4 != 0;
        if ended && pending_packet {
            return Err(AudioContainerInvalid);
        }
        sequence = sequence.wrapping_add(1);
        offset = end;
    }
    if serial.is_none() || !ended || pending_packet {
        return Err(AudioContainerInvalid);
    }
    Ok(())
}

/// Ogg uses a non-reflected CRC with polynomial 0x04c11db7 and zero initial
/// remainder. The stored checksum bytes are zeroed for the calculation.
fn checksum(page: &[u8]) -> u32 {
    let mut crc = 0u32;
    for (index, byte) in page.iter().enumerate() {
        let byte = if (22..26).contains(&index) { 0 } else { *byte };
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x80000000 != 0 {
                (crc << 1) ^ 0x04c11db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    const TONE: &[u8] = include_bytes!("../../tests/fixtures/audio/tone.ogg");
    fn pages(bytes: &[u8]) -> Vec<(usize, usize)> {
        let mut result = Vec::new();
        let mut offset = 0;
        while offset < bytes.len() {
            let table_end = offset + 27 + usize::from(bytes[offset + 26]);
            let end = table_end
                + bytes[offset + 27..table_end]
                    .iter()
                    .map(|v| usize::from(*v))
                    .sum::<usize>();
            result.push((offset, end));
            offset = end;
        }
        result
    }
    fn repair_crc(bytes: &mut [u8], range: (usize, usize)) {
        let crc = checksum(&bytes[range.0..range.1]);
        bytes[range.0 + 22..range.0 + 26].copy_from_slice(&crc.to_le_bytes());
    }
    #[test]
    fn complete_external_fixture_passes_and_truncation_or_corruption_fails() {
        assert!(validate_extent(TONE).is_ok());
        let ranges = pages(TONE);
        for (offset, end) in &ranges {
            if *offset != 0 {
                assert!(validate_extent(&TONE[..*offset]).is_err());
            }
            for cut in [offset + 1, offset + 26, end - 1] {
                assert!(validate_extent(&TONE[..cut]).is_err());
            }
        }
        let mut corrupted = TONE.to_vec();
        *corrupted.last_mut().unwrap() ^= 1;
        assert_eq!(
            validate_extent(&corrupted),
            Err(AudioInspectionError::AudioContainerInvalid)
        );
    }
    #[test]
    fn valid_checksums_cannot_hide_bad_flags_sequences_or_serials() {
        let ranges = pages(TONE);
        for (page, relative, value) in [
            (0, 5, 0),
            (0, 4, 1),
            (0, 5, 3),
            (1, 5, 2),
            (1, 18, 99),
            (1, 14, TONE[ranges[1].0 + 14] ^ 1),
        ] {
            let mut changed = TONE.to_vec();
            changed[ranges[page].0 + relative] = value;
            repair_crc(&mut changed, ranges[page]);
            assert!(validate_extent(&changed).is_err());
        }
        let last = *ranges.last().unwrap();
        let mut missing_eos = TONE.to_vec();
        missing_eos[last.0 + 5] &= !4;
        repair_crc(&mut missing_eos, last);
        assert!(validate_extent(&missing_eos).is_err());
        let mut chained = TONE.to_vec();
        chained.extend(TONE);
        assert_eq!(
            validate_extent(&chained),
            Err(AudioInspectionError::AudioStreamChanged)
        );
    }
}

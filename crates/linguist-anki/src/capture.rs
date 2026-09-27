//! Optimistic read capture. Repeated matching reads are not an atomic native snapshot.
use super::*;
use std::collections::BTreeSet;
#[derive(Debug, Serialize)]
pub struct ReadCapture {
    pub note: Value,
    pub model: ModelInspection,
    pub cards: Vec<Value>,
    pub repeated_reads_matched: bool,
    pub atomic_snapshot_verified: bool,
    pub native_history_verified: bool,
}
impl Client {
    /// Capture complete read-port results twice; fail closed on any source or scheduler drift.
    /// This never retries collection changes, creates state or authorizes mutation.
    pub fn capture_note(&self, note_id: &str) -> Result<ReadCapture> {
        numeric_id(note_id)?;
        let first = self.capture_once(note_id)?;
        let second = self.capture_once(note_id)?;
        if canonical::bytes(&first).map_err(|_| "ANKI_CAPTURE_SCHEMA_INVALID")?
            != canonical::bytes(&second).map_err(|_| "ANKI_CAPTURE_SCHEMA_INVALID")?
        {
            return Err("ANKI_CAPTURE_SOURCE_CONFLICT".into());
        }
        Ok(ReadCapture {
            repeated_reads_matched: true,
            ..first
        })
    }
    fn capture_once(&self, note_id: &str) -> Result<ReadCapture> {
        let note = self
            .notes_info(&[note_id.to_owned()])?
            .into_iter()
            .next()
            .filter(|n| n.get("noteId").is_some())
            .ok_or("ANKI_NOTE_NOT_FOUND")?;
        let note = normalize_note_ids(note)?;
        let model_name = note["modelName"].as_str().ok_or("ANKI_NOTE_INVALID")?;
        // Resolve by exact name without interpreting a numeric-looking name as an ID.
        let named = self
            .models()?
            .into_iter()
            .find(|m| m.name == model_name)
            .ok_or("ANKI_CAPTURE_MODEL_MISSING")?;
        let model = self.inspect_model(&named.id)?;
        let ids = note["cards"]
            .as_array()
            .filter(|ids| ids.len() <= 10000)
            .ok_or("ANKI_NOTE_INVALID")?
            .iter()
            .map(wire_id)
            .collect::<Result<Vec<_>>>()?;
        let unique: BTreeSet<_> = ids.iter().collect();
        if unique.len() != ids.len() {
            return Err("ANKI_CAPTURE_CARD_CONFLICT".into());
        }
        let cards = self
            .cards_info(&ids)?
            .into_iter()
            .map(normalize_card_ids)
            .collect::<Result<Vec<_>>>()?;
        if cards
            .iter()
            .any(|card| card["note"].as_str() != Some(note_id))
        {
            return Err("ANKI_CAPTURE_CARD_CONFLICT".into());
        }
        Ok(ReadCapture {
            note,
            model,
            cards,
            repeated_reads_matched: false,
            atomic_snapshot_verified: false,
            native_history_verified: false,
        })
    }
}

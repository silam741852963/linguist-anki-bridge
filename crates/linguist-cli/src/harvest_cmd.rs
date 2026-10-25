//! WP-25 word harvester commands: `harvest scan` reads the deck read-only
//! from Anki and the applied documents from the store; every other command
//! works on the store's harvest tables alone. Nothing here writes Anki.
use crate::{emit, state_root};
use clap::{Args, Subcommand};
use linguist_application::harvest;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum HarvestCommand {
    /// Read the deck's words and example sentences and update the
    /// candidates; a rescan reads only notes whose document changed.
    Scan {
        /// japanese_vocab or english_vocab.
        #[arg(long)]
        purpose: String,
        /// Anki deck whose notes count as known words; defaults to the
        /// purpose's target deck, then its source deck.
        #[arg(long)]
        deck: Option<String>,
        /// Forget the scan marks first and read every note again.
        #[arg(long)]
        full: bool,
        /// Read at most this many changed notes.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Ranked candidates: i+1 sentences, notes, frequency, provenance.
    List {
        /// japanese_vocab or english_vocab.
        #[arg(long)]
        purpose: String,
        /// new, queued, ignored, known or added; default new and queued.
        #[arg(long)]
        status: Option<String>,
        /// Met in at least this many notes.
        #[arg(long, default_value_t = 1)]
        min_notes: usize,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// One candidate with every sentence it was met in.
    Show {
        /// japanese_vocab or english_vocab.
        #[arg(long)]
        purpose: String,
        /// The candidate's base form.
        word: String,
    },
    /// Never suggest these words again.
    Ignore(Marked),
    /// Count these words as known (the deck has them under another form).
    Known(Marked),
    /// Put ignored, known or queued words back to new.
    Unignore(Marked),
    /// Queue words for export; --top takes the best ranked new words.
    Queue(Marked),
    /// Write the candidates of one status as v2 vocabulary JSONL for
    /// `vocab add --format jsonl`.
    Export {
        /// japanese_vocab or english_vocab.
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "queued")]
        status: String,
        /// JSONL file to create; it must not exist.
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(Args)]
pub(crate) struct Marked {
    /// japanese_vocab or english_vocab.
    #[arg(long)]
    purpose: String,
    /// Candidate base forms.
    #[arg(required_unless_present = "top")]
    words: Vec<String>,
    /// The best ranked new candidates (queue only).
    #[arg(long, conflicts_with = "words")]
    top: Option<usize>,
}

fn now_ms() -> u64 {
    linguist_application::campaign::now_ms()
}

fn deck_name(
    settings: &linguist_config::Effective,
    purpose: &str,
    deck: Option<&str>,
) -> Result<String, String> {
    if let Some(deck) = deck {
        return Ok(deck.to_owned());
    }
    ["target_deck", "source_deck"]
        .iter()
        .find_map(|key| {
            settings
                .values
                .get(&format!("purposes.{purpose}.{key}"))
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
        })
        .ok_or_else(|| format!("HARVEST_DECK_UNSET: pass --deck or map purposes.{purpose}"))
}

/// The deck's notes, read in pages: the word is the Expression field, else
/// Word, else the first field.
fn deck_notes(
    settings: &linguist_config::Effective,
    deck: &str,
) -> Result<Vec<harvest::DeckNote>, String> {
    let client = crate::anki_client(settings)?;
    let ids = client.find_notes(&linguist_anki::deck_query(deck)?)?;
    let mut notes = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(200) {
        for note in client.notes_info(chunk)? {
            if note.get("noteId").is_none() {
                continue;
            }
            let note = linguist_anki::normalize_note_ids(note)?;
            let note_id = note["noteId"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| note["noteId"].as_i64())
                .ok_or("ANKI_NOTE_INVALID")?;
            let mut fields: Vec<(i64, String, String)> = note["fields"]
                .as_object()
                .ok_or("ANKI_NOTE_INVALID")?
                .iter()
                .map(|(name, field)| {
                    (
                        field["order"].as_i64().unwrap_or(i64::MAX),
                        name.clone(),
                        field["value"].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect();
            fields.sort();
            let expression = ["Expression", "Word"]
                .iter()
                .find_map(|want| fields.iter().find(|(_, n, _)| n == want))
                .or(fields.first())
                .map(|(_, _, v)| v.clone())
                .unwrap_or_default();
            notes.push(harvest::DeckNote {
                note_id,
                expression,
                fields: fields.into_iter().map(|(_, n, v)| (n, v)).collect(),
            });
        }
    }
    Ok(notes)
}

fn ranked_json(r: &harvest::Ranked) -> Value {
    json!({"word": r.base, "reading": r.reading, "pos": r.pos, "status": r.status,
        "i_plus_one": r.i_plus_one, "notes": r.notes, "sentences": r.sentences,
        "frequency": r.frequency, "attested": r.attested,
        "sentence": r.best.as_ref().map(|b| &b.sentence),
        "translation": r.best.as_ref().map(|b| &b.translation)})
}

pub(crate) fn handle(
    settings: &linguist_config::Effective,
    command: &HarvestCommand,
) -> Result<u8, String> {
    let root = state_root(settings)?;
    match command {
        HarvestCommand::Scan {
            purpose,
            deck,
            full,
            limit,
        } => {
            harvest::language(purpose)?;
            let deck = deck_name(settings, purpose, deck.as_deref())?;
            let jmdict = match settings
                .values
                .get("harvest.jmdict_path")
                .and_then(Value::as_str)
            {
                Some(path) => {
                    let env = std::env::vars().collect();
                    let path = linguist_config::expand_path(path, &env)?;
                    Some(linguist_text::jmdict::Jmdict::load(&path)?)
                }
                None => None,
            };
            let notes = deck_notes(settings, &deck)?;
            let mut store = linguist_store::Store::open(&root)?;
            let summary = harvest::scan(
                &mut store,
                &harvest::ScanRequest {
                    purpose,
                    deck: &notes,
                    jmdict: jmdict.as_ref(),
                    full: *full,
                    limit: *limit,
                    now_ms: now_ms(),
                },
            )?;
            let mut out = serde_json::to_value(&summary).map_err(|e| e.to_string())?;
            out["deck"] = json!(deck);
            out["jmdict_entries"] = json!(jmdict.as_ref().map(|d| d.len()));
            out["anki_writes"] = json!(false);
            emit(&out)?;
            Ok(0)
        }
        HarvestCommand::List {
            purpose,
            status,
            min_notes,
            limit,
        } => {
            if let Some(status) = status
                && !linguist_store::harvest::STATUSES.contains(&status.as_str())
            {
                return Err(format!("HARVEST_STATUS_INVALID: {status}"));
            }
            let store = linguist_store::Store::read_only(&root)?;
            let ranked = harvest::rank(&store, purpose)?;
            let matching: Vec<&harvest::Ranked> = ranked
                .iter()
                .filter(|r| match status {
                    Some(s) => &r.status == s,
                    None => matches!(r.status.as_str(), "new" | "queued"),
                })
                .filter(|r| r.notes >= *min_notes)
                .collect();
            let shown: Vec<Value> = matching
                .iter()
                .take(*limit)
                .map(|r| ranked_json(r))
                .collect();
            emit(&json!({"purpose": purpose, "total": matching.len(),
                "shown": shown.len(), "candidates": shown}))?;
            Ok(0)
        }
        HarvestCommand::Show { purpose, word } => {
            let store = linguist_store::Store::read_only(&root)?;
            let ranked = harvest::rank(&store, purpose)?;
            let found = ranked
                .iter()
                .find(|r| &r.base == word)
                .ok_or_else(|| format!("HARVEST_WORD_NOT_FOUND: {word}"))?;
            let contexts: Vec<Value> = store
                .harvest_contexts(purpose)?
                .into_iter()
                .filter(|c| &c.base == word)
                .map(|c| {
                    json!({"note": c.note, "sentence": c.sentence, "translation": c.translation,
                    "provenance": c.provenance, "other_unknown": c.others})
                })
                .collect();
            let mut out = ranked_json(found);
            out["contexts"] = json!(contexts);
            emit(&out)?;
            Ok(0)
        }
        HarvestCommand::Ignore(m)
        | HarvestCommand::Known(m)
        | HarvestCommand::Unignore(m)
        | HarvestCommand::Queue(m) => {
            let status = match command {
                HarvestCommand::Ignore(_) => "ignored",
                HarvestCommand::Known(_) => "known",
                HarvestCommand::Unignore(_) => "new",
                _ => "queued",
            };
            if m.top.is_some() && status != "queued" {
                return Err("HARVEST_TOP_QUEUE_ONLY: --top works with queue".into());
            }
            let mut store = linguist_store::Store::open_existing(&root)?;
            let words: Vec<String> = match m.top {
                Some(n) => harvest::rank(&store, &m.purpose)?
                    .into_iter()
                    .filter(|r| r.status == "new")
                    .take(n)
                    .map(|r| r.base)
                    .collect(),
                None => m.words.clone(),
            };
            harvest::language(&m.purpose)?;
            let changed = store.harvest_set_status(&m.purpose, &words, status, now_ms())?;
            let missing: Vec<&String> = words.iter().filter(|w| !changed.contains(w)).collect();
            emit(
                &json!({"purpose": m.purpose, "status": status, "changed": changed,
                "not_found": missing}),
            )?;
            Ok(if missing.is_empty() { 0 } else { 1 })
        }
        HarvestCommand::Export {
            purpose,
            status,
            output,
        } => {
            let store = linguist_store::Store::read_only(&root)?;
            let records = harvest::export(&store, purpose, status)?;
            let mut text = String::new();
            for record in &records {
                text.push_str(&serde_json::to_string(record).map_err(|e| e.to_string())?);
                text.push('\n');
            }
            use std::io::Write;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)
                .and_then(|mut f| f.write_all(text.as_bytes()))
                .map_err(|e| format!("HARVEST_EXPORT_UNWRITABLE: {}: {e}", output.display()))?;
            emit(
                &json!({"purpose": purpose, "status": status, "records": records.len(),
                "output": output, "next_command": format!(
                    "linguist-anki-bridge vocab add --document {} --format jsonl", output.display())}),
            )?;
            Ok(0)
        }
    }
}

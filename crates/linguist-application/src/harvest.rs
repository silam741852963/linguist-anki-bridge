//! WP-25 word harvester: words met in the deck's own example sentences that
//! the deck has no card for. The scan reads each note's latest applied
//! document from the store (examples, collocations, usage and nuance, with
//! their provenance); a note never revamped gives the visible text of its
//! example fields. It writes only the harvest tables, never Anki.
use linguist_core::{LearningContent, LearningDocument, Provenance, canonical};
use linguist_store::Store;
use linguist_store::harvest::{HarvestCandidate, HarvestContext};
use linguist_text::jmdict::Jmdict;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, String>;

/// The target language of a vocabulary purpose; grammar notes of that
/// language are scanned too.
pub fn language(purpose: &str) -> Result<&'static str> {
    match purpose {
        "japanese_vocab" => Ok("ja"),
        "english_vocab" => Ok("en"),
        other => Err(format!(
            "HARVEST_PURPOSE_UNSUPPORTED: {other} (japanese_vocab or english_vocab)"
        )),
    }
}

/// A note of the deck as Anki holds it: its word and, for a note no apply
/// wrote, the fields whose example text is scanned.
#[derive(Clone, Debug, Default)]
pub struct DeckNote {
    pub note_id: i64,
    pub expression: String,
    /// Field name and raw HTML.
    pub fields: Vec<(String, String)>,
}

/// The words of the deck that count as known.
#[derive(Debug, Default)]
pub struct Known {
    words: BTreeSet<String>,
}

impl Known {
    fn key(word: &str) -> String {
        linguist_text::hiragana(&word.trim().to_lowercase())
    }
    pub fn insert(&mut self, word: &str) {
        let word = word.trim();
        if word.is_empty() {
            return;
        }
        self.words.insert(Self::key(word));
        let head = linguist_core::review::head_word(word);
        self.words.insert(Self::key(&head));
        // A listed form with a する ending (勉強する) also knows its noun.
        if let Some(stem) = head.strip_suffix("する").filter(|s| !s.is_empty()) {
            self.words.insert(Self::key(stem));
        }
    }
    pub fn contains(&self, word: &str) -> bool {
        self.words.contains(&Self::key(word))
    }
    pub fn len(&self) -> usize {
        self.words.len()
    }
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

/// Every form a vocabulary document names its word by.
fn document_words(doc: &LearningDocument, known: &mut Known) {
    if let LearningContent::Vocabulary(v) = &doc.content {
        known.insert(&v.expression);
        known.insert(&linguist_core::review::dictionary_form(v));
        known.insert(&v.reading);
        for spelling in &v.spellings {
            known.insert(spelling);
        }
        for entry in v
            .dictionary
            .iter()
            .filter(|e| e.senses.iter().any(|s| s.key == v.sense_key))
        {
            for form in entry.forms.iter().chain(&entry.readings) {
                known.insert(form);
            }
        }
    }
}

fn provenance(p: Provenance) -> &'static str {
    match p {
        Provenance::Source => "source",
        Provenance::Dictionary => "dictionary",
        Provenance::Ocr => "ocr",
        Provenance::Generated => "generated",
        Provenance::User => "user",
        Provenance::Provider => "provider",
    }
}

/// One scanned sentence: text, translation and provenance.
type Sentence = (String, String, &'static str);

fn split_sentences(text: &str, provenance: &'static str) -> Vec<Sentence> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if matches!(c, '。' | '！' | '？' | '.' | '!' | '?' | '\n') {
            let s = current.trim().to_owned();
            if s.chars().count() > 1 {
                out.push((s, String::new(), provenance));
            }
            current.clear();
        }
    }
    let s = current.trim().to_owned();
    if s.chars().count() > 1 {
        out.push((s, String::new(), provenance));
    }
    out
}

/// The sentences a document offers: examples with their translations,
/// collocations, and the sentences of usage and nuance (decision 2: not the
/// meaning).
pub fn sentences(doc: &LearningDocument) -> Vec<Sentence> {
    let mut out: Vec<Sentence> = Vec::new();
    let (examples, usage, nuance) = match &doc.content {
        LearningContent::Vocabulary(v) => {
            for c in &v.collocations {
                out.push((c.phrase.clone(), c.gloss.clone(), "generated"));
            }
            (&v.examples, &v.usage, &v.nuance)
        }
        LearningContent::Grammar(g) => (&g.examples, &g.usage, &g.nuance),
        // A drill's sentences are the source's own keigo practice, read
        // from its answers.
        LearningContent::Drill(d) => {
            for answer in &d.answers {
                out.push((answer.trim().to_owned(), String::new(), "source"));
            }
            return out;
        }
    };
    for e in examples {
        out.push((
            e.sentence.trim().to_owned(),
            e.translation.trim().to_owned(),
            provenance(e.provenance),
        ));
    }
    out.extend(split_sentences(usage, "generated"));
    for n in nuance {
        out.extend(split_sentences(&n.difference, "generated"));
    }
    out.retain(|(s, _, _)| !s.is_empty());
    out
}

/// Visible text of a note's example fields (a field whose name holds
/// "example" or "sentence"), one sentence per item.
fn fallback_sentences(note: &DeckNote) -> Vec<Sentence> {
    let mut out = Vec::new();
    for (name, html) in &note.fields {
        let lower = name.to_lowercase();
        if !(lower.contains("example") || lower.contains("sentence")) {
            continue;
        }
        let text = crate::revamp::visible_text_of(html);
        out.extend(split_sentences(&text, "source"));
    }
    out
}

/// Inputs of one scan.
pub struct ScanRequest<'a> {
    pub purpose: &'a str,
    /// Every note of the purpose's deck, as Anki holds it.
    pub deck: &'a [DeckNote],
    pub jmdict: Option<&'a Jmdict>,
    pub full: bool,
    /// At most this many changed notes are read in this run.
    pub limit: Option<usize>,
    pub now_ms: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct ScanSummary {
    pub purpose: String,
    pub deck_notes: usize,
    pub known_words: usize,
    pub notes_read: usize,
    pub notes_unchanged: usize,
    pub from_documents: usize,
    pub from_fields: usize,
    pub sentences: usize,
    pub candidates: usize,
    pub new_candidates: usize,
    pub became_added: usize,
    pub pruned: usize,
    pub more: bool,
}

struct Source {
    note: String,
    digest: String,
    sentences: Vec<Sentence>,
}

fn words_of(
    language: &str,
    sentence: &str,
    known: &Known,
    jmdict: Option<&Jmdict>,
    irregular: &BTreeMap<String, String>,
) -> Result<Vec<linguist_text::Word>> {
    if language == "ja" {
        linguist_text::ja::words(sentence, &|w| {
            known.contains(w) || jmdict.is_some_and(|d| d.contains(w))
        })
    } else {
        Ok(linguist_text::en::words(sentence, irregular))
    }
}

/// Scan the purpose's notes and update the candidates.
pub fn scan(store: &mut Store, request: &ScanRequest) -> Result<ScanSummary> {
    let language = language(request.purpose)?;
    let purpose = request.purpose;
    let mut summary = ScanSummary {
        purpose: purpose.into(),
        deck_notes: request.deck.len(),
        ..Default::default()
    };
    // Known words: every deck note's word, every applied document's forms,
    // and the words the user marked.
    let mut known = Known::default();
    for note in request.deck {
        let word = crate::revamp::visible_text_of(&note.expression);
        known.insert(&word);
        // A deck word of one token is also known by its reading, so a kana
        // spelling (できる) meets its kanji note (出来る). Kanji candidates are
        // matched by spelling only, so homophones stay apart.
        if language == "ja" {
            let head = linguist_core::review::head_word(&word);
            if let Ok(tokens) = linguist_text::ja::tokenize(head.trim())
                && let [token] = tokens.as_slice()
                && !token.reading.is_empty()
            {
                known.insert(&token.reading);
            }
        }
    }
    let latest = crate::retag::latest_by_note(store)?;
    let mut revisions: BTreeMap<(uuid::Uuid, u32), linguist_core::records::PlanRevision> =
        BTreeMap::new();
    let mut documents: BTreeMap<i64, LearningDocument> = BTreeMap::new();
    let mut irregular: BTreeMap<String, String> = BTreeMap::new();
    for (note, applied) in &latest {
        let Ok(applied) = applied else { continue };
        let key = (applied.plan_id, applied.revision);
        if let std::collections::btree_map::Entry::Vacant(e) = revisions.entry(key) {
            e.insert(store.revision(applied.plan_id, applied.revision)?);
        }
        let Some(doc) = revisions[&key]
            .documents
            .iter()
            .find(|d| d.id == applied.item_id)
        else {
            continue;
        };
        if !doc.target_language.as_str().starts_with(language) {
            continue;
        }
        document_words(doc, &mut known);
        if let LearningContent::Vocabulary(v) = &doc.content {
            for entry in &v.dictionary {
                for form in entry.metadata.get("irregular_forms").into_iter().flatten() {
                    irregular.entry(form.to_lowercase()).or_insert_with(|| {
                        linguist_core::review::head_word(&v.expression).to_lowercase()
                    });
                }
            }
        }
        documents.insert(*note, doc.clone());
    }
    let candidates_before = store.harvest_candidates(purpose)?;
    let marked: BTreeSet<String> = candidates_before
        .iter()
        .filter(|c| matches!(c.status.as_str(), "known" | "ignored" | "added"))
        .map(|c| c.base.clone())
        .collect();
    for base in &marked {
        known.insert(base);
    }
    summary.known_words = known.len();
    // The notes to read: an applied document (any deck or none: a grammar
    // note of the language counts), else a deck note's example fields.
    let mut sources: Vec<Source> = Vec::new();
    for (note, doc) in &documents {
        sources.push(Source {
            note: note.to_string(),
            digest: doc.semantic_digest().map_err(|e| e.to_string())?,
            sentences: sentences(doc),
        });
    }
    for note in request.deck {
        if documents.contains_key(&note.note_id) {
            continue;
        }
        let found = fallback_sentences(note);
        let text: Vec<&str> = found.iter().map(|(s, _, _)| s.as_str()).collect();
        sources.push(Source {
            note: note.note_id.to_string(),
            digest: canonical::asset_digest(text.join("\n").as_bytes()),
            sentences: found,
        });
    }
    if request.full {
        store.harvest_reset(purpose)?;
    }
    let marks = store.harvest_marks(purpose)?;
    let changed: Vec<&Source> = sources
        .iter()
        .filter(|s| marks.get(&s.note) != Some(&s.digest))
        .collect();
    summary.notes_unchanged = sources.len() - changed.len();
    let take = request.limit.unwrap_or(usize::MAX);
    summary.more = changed.len() > take;
    let existing: BTreeSet<String> = candidates_before.iter().map(|c| c.base.clone()).collect();
    let mut new_bases = BTreeSet::new();
    for source in changed.into_iter().take(take) {
        let mut contexts = Vec::new();
        let mut candidates: BTreeMap<String, HarvestCandidate> = BTreeMap::new();
        for (sentence, translation, provenance) in &source.sentences {
            summary.sentences += 1;
            let words = words_of(language, sentence, &known, request.jmdict, &irregular)?;
            let unknown: Vec<&linguist_text::Word> = words
                .iter()
                .filter(|w| !w.forms.iter().any(|f| known.contains(f)))
                .collect();
            let bases: BTreeSet<&str> = unknown.iter().map(|w| w.base.as_str()).collect();
            for word in &unknown {
                let others: Vec<String> = bases
                    .iter()
                    .filter(|b| **b != word.base)
                    .map(|b| b.to_string())
                    .collect();
                contexts.push(HarvestContext {
                    note: source.note.clone(),
                    base: word.base.clone(),
                    sentence: sentence.clone(),
                    translation: translation.clone(),
                    provenance: (*provenance).into(),
                    others,
                });
                let entries = request
                    .jmdict
                    .map(|d| d.lookup(&word.base))
                    .unwrap_or_default();
                let reading = if word.reading.is_empty() {
                    entries
                        .first()
                        .and_then(|e| e.readings.first().cloned())
                        .unwrap_or_default()
                } else {
                    word.reading.clone()
                };
                candidates
                    .entry(word.base.clone())
                    .or_insert(HarvestCandidate {
                        base: word.base.clone(),
                        reading,
                        pos: word.pos.clone(),
                        status: "new".into(),
                        frequency: entries.iter().filter_map(|e| e.frequency).min(),
                    });
                if !existing.contains(&word.base) {
                    new_bases.insert(word.base.clone());
                }
            }
        }
        let candidates: Vec<HarvestCandidate> = candidates.into_values().collect();
        store.harvest_replace_note(
            purpose,
            &source.note,
            &source.digest,
            &contexts,
            &candidates,
            request.now_ms,
        )?;
        summary.notes_read += 1;
        if documents.contains_key(&source.note.parse().unwrap_or_default()) {
            summary.from_documents += 1;
        } else {
            summary.from_fields += 1;
        }
    }
    summary.new_candidates = new_bases.len();
    // A queued word that now has a note is added; a new one is no longer a
    // candidate (decision 9).
    let became: Vec<String> = store
        .harvest_candidates(purpose)?
        .into_iter()
        .filter(|c| c.status == "queued" && known.contains(&c.base))
        .map(|c| c.base)
        .collect();
    summary.became_added = store
        .harvest_set_status(purpose, &became, "added", request.now_ms)?
        .len();
    let now_known: Vec<String> = store
        .harvest_candidates(purpose)?
        .into_iter()
        .filter(|c| c.status == "new" && known.contains(&c.base) && !marked.contains(&c.base))
        .map(|c| c.base)
        .collect();
    summary.pruned = store.harvest_forget(purpose, &now_known)? + store.harvest_prune(purpose)?;
    summary.candidates = store.harvest_candidates(purpose)?.len();
    Ok(summary)
}

/// One ranked candidate with its scores and best context sentence.
#[derive(Clone, Debug, Serialize)]
pub struct Ranked {
    pub base: String,
    pub reading: String,
    pub pos: String,
    pub status: String,
    /// Context sentences where it is the only unknown word.
    pub i_plus_one: usize,
    /// Distinct notes it was met in.
    pub notes: usize,
    pub sentences: usize,
    pub frequency: Option<u8>,
    /// Met in dictionary or source text, not only generated text.
    pub attested: bool,
    pub best: Option<HarvestContext>,
}

/// Candidates ranked by decision 8: i+1 sentences, spread, frequency,
/// provenance.
pub fn rank(store: &Store, purpose: &str) -> Result<Vec<Ranked>> {
    language(purpose)?;
    let candidates = store.harvest_candidates(purpose)?;
    let open: BTreeSet<String> = candidates
        .iter()
        .filter(|c| matches!(c.status.as_str(), "new" | "queued"))
        .map(|c| c.base.clone())
        .collect();
    let mut by_base: BTreeMap<String, Vec<HarvestContext>> = BTreeMap::new();
    for c in store.harvest_contexts(purpose)? {
        by_base.entry(c.base.clone()).or_default().push(c);
    }
    let mut out: Vec<Ranked> = candidates
        .into_iter()
        .map(|c| {
            let contexts = by_base.remove(&c.base).unwrap_or_default();
            let alone = |x: &HarvestContext| x.others.iter().all(|o| !open.contains(o));
            let attested_kind =
                |x: &HarvestContext| matches!(x.provenance.as_str(), "dictionary" | "source");
            let best = contexts
                .iter()
                .min_by_key(|x| (!alone(x), !attested_kind(x), x.sentence.chars().count()))
                .cloned();
            Ranked {
                i_plus_one: contexts.iter().filter(|x| alone(x)).count(),
                notes: contexts
                    .iter()
                    .map(|x| &x.note)
                    .collect::<BTreeSet<_>>()
                    .len(),
                sentences: contexts.len(),
                attested: contexts.iter().any(attested_kind),
                best,
                base: c.base,
                reading: c.reading,
                pos: c.pos,
                status: c.status,
                frequency: c.frequency,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.i_plus_one
            .cmp(&a.i_plus_one)
            .then(b.notes.cmp(&a.notes))
            .then(
                a.frequency
                    .unwrap_or(u8::MAX)
                    .cmp(&b.frequency.unwrap_or(u8::MAX)),
            )
            .then(b.attested.cmp(&a.attested))
            .then(a.base.cmp(&b.base))
    });
    Ok(out)
}

/// The v2 vocabulary records `vocab add --format jsonl` reads, one per
/// candidate: the base form, its reading and the best context sentence as
/// the first example (provenance `source`, naming the note it came from).
pub fn export(store: &Store, purpose: &str, status: &str) -> Result<Vec<Value>> {
    let language = language(purpose)?;
    let ranked = rank(store, purpose)?;
    Ok(ranked
        .into_iter()
        .filter(|r| r.status == status)
        .map(|r| {
            let mut body = json!({"expression": r.base});
            if language == "ja" && !r.reading.is_empty() {
                body["reading"] = json!(r.reading);
            }
            let mut summary = format!("WP-25 harvest: met in {} note(s)", r.notes);
            if let Some(best) = &r.best {
                body["examples"] = json!([{
                    "sentence": best.sentence,
                    "translation": best.translation,
                    "provenance": "source",
                }]);
                summary = format!("{summary}; first example from note {}", best.note);
            }
            json!({
                "kind": "vocabulary",
                "schema_version": 2,
                "target_language": language,
                "body": body,
                "source_summary": summary,
                "tags": ["lab::harvest"],
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_words_take_head_words_and_suru_stems() {
        let mut known = Known::default();
        known.insert("費用(がかかる)");
        known.insert("勉強する");
        known.insert("Apple");
        assert!(known.contains("費用"));
        assert!(known.contains("勉強"));
        assert!(known.contains("apple"));
    }

    #[test]
    fn sentences_split_usage_but_keep_examples_whole() {
        let s = split_sentences("一つ目。二つ目！ three", "generated");
        assert_eq!(s.len(), 3);
    }
}

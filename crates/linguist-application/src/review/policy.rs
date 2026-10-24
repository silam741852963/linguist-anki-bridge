//! Campaign decision policy (WP-24): the user's standing review preferences
//! from WP-20 to WP-23 as rules that fill a `plans resolve-batch` template.
//!
//! The policy is pure: it reads the template and the plan it was made from
//! and returns the decisions it can make plus every issue it cannot decide,
//! with a reason. It never invents content: each choice is one of the
//! template's own choices (or a source media role built from the document's
//! own evidence), so `resolve` revalidates every decision as for a reviewer.
use linguist_core::{
    LearningContent, LearningDocument,
    records::{MediaOwner, PlanRevision},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// `contracts/v2/review-policy.schema.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema_version: u16,
    /// Actor recorded on every policy decision.
    pub actor: String,
    /// SOURCE_ORDINAL=TASK entries for every native history review.
    pub history_map: Vec<String>,
    /// OCR image classes kept as the card's picture (vocabulary only).
    pub keep_picture_classes: Vec<String>,
    /// OCR image classes archived (not rendered).
    pub archive_picture_classes: Vec<String>,
    /// Keep the note's own recording and decline synthesized/dictionary audio.
    pub keep_own_audio: bool,
    /// Accept generated facts (the agent spot-checks a share of them).
    pub accept_generated_facts: bool,
    /// Issue codes closed with their first template choice.
    pub accept_first_template: Vec<String>,
    /// Issue codes always left to the agent.
    pub agent_codes: Vec<String>,
}

impl Policy {
    /// The policy WP-24 ships (`resources/policies/campaign-default.json`).
    pub fn builtin() -> Self {
        serde_json::from_str(include_str!(
            "../../../../resources/policies/campaign-default.json"
        ))
        .expect("builtin policy")
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("REVIEW_POLICY_UNSUPPORTED_VERSION".into());
        }
        if self.actor.trim().is_empty() {
            return Err("REVIEW_POLICY_INVALID: actor is empty".into());
        }
        super::batch::parse_history_map(&self.history_map)?;
        let classes = [
            "illustration",
            "dictionary",
            "grammar",
            "mixed",
            "uncertain",
        ];
        for class in self
            .keep_picture_classes
            .iter()
            .chain(&self.archive_picture_classes)
        {
            if !classes.contains(&class.to_ascii_lowercase().as_str()) {
                return Err(format!(
                    "REVIEW_POLICY_INVALID: unknown image class {class}"
                ));
            }
        }
        if self.keep_picture_classes.iter().any(|c| {
            self.archive_picture_classes
                .iter()
                .any(|a| a.eq_ignore_ascii_case(c))
        }) {
            return Err("REVIEW_POLICY_INVALID: a class is both kept and archived".into());
        }
        Ok(())
    }
}

/// One review issue the policy left open.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Undecided {
    pub document_id: uuid::Uuid,
    pub issue_id: String,
    pub code: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct Filled {
    /// A resolution batch with only the decided entries (none: nothing to submit).
    pub batch: Option<Value>,
    pub decided: usize,
    pub undecided: Vec<Undecided>,
}

/// The OCR classification recorded for one source image, if it was inspected.
pub fn image_class(document: &LearningDocument, digest: &str) -> Option<String> {
    let field = format!("ocr:{digest}");
    document
        .evidence
        .iter()
        .filter(|e| e.field == field && e.region_id.is_none())
        .find_map(|e| {
            let claim: Value = serde_json::from_str(&e.claim).ok()?;
            claim["classification"]["class"].as_str().map(str::to_owned)
        })
}

fn spoken(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Katakana folded to hiragana (a reading written either way is the same).
fn hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{30a1}'..='\u{30f6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// A note's reading field as tiers of readings, best first: the whole field
/// without spaces (`だい どころ` is だいどころ); each part between separators or
/// line breaks,
/// without spaces (`に じゅう なな; に じゅう しち`); each space-separated word
/// (`そふ おじいさん`), only when nothing better matches.
fn readings(raw: &str) -> [Vec<String>; 3] {
    let clean =
        |v: Vec<String>| -> Vec<String> { v.into_iter().filter(|r| !r.is_empty()).collect() };
    [
        clean(vec![hiragana(&spoken(raw))]),
        clean(
            raw.split(|c: char| ",、;；/／・\n".contains(c))
                .map(|part| hiragana(&spoken(part)))
                .collect(),
        ),
        clean(raw.split_whitespace().map(hiragana).collect()),
    ]
}

fn is_stroke_gif(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".gif")
        && (lower.starts_with("lab_stroke_")
            || lower
                .trim_end_matches(".gif")
                .chars()
                .all(|c| c.is_ascii_hexdigit()))
}

/// The role a source image takes, or why the policy cannot say.
fn picture_role(
    policy: &Policy,
    document: &LearningDocument,
    asset: &linguist_core::records::MediaAsset,
) -> Result<&'static str, String> {
    let name = asset
        .original_filename
        .as_deref()
        .unwrap_or(&asset.filename);
    if is_stroke_gif(name) {
        return Ok("archive");
    }
    // Grammar screenshots are the OCR source; their text is the card.
    if matches!(document.content, LearningContent::Grammar(_)) {
        return Ok("archive");
    }
    // The OCR report writes classes in snake case (`illustration`).
    let listed = |list: &[String], class: &str| list.iter().any(|c| c.eq_ignore_ascii_case(class));
    match image_class(document, &asset.digest) {
        Some(class) if listed(&policy.keep_picture_classes, &class) => Ok("picture"),
        Some(class) if listed(&policy.archive_picture_classes, &class) => Ok("archive"),
        Some(class) => Err(format!(
            "IMAGE_CLASS_{}: look at the image and choose picture or archive",
            class.to_ascii_uppercase()
        )),
        None => Err(
            "IMAGE_NOT_CLASSIFIED: run with images.existing_policy=inspect or look at the image"
                .into(),
        ),
    }
}

/// True when the policy keeps one of the document's own pictures.
fn keeps_own_picture(policy: &Policy, document: &LearningDocument) -> bool {
    document.media.iter().any(|m| {
        m.owner == MediaOwner::Source
            && m.mime.starts_with("image/")
            && picture_role(policy, document, m) == Ok("picture")
    })
}

fn has_own_audio(document: &LearningDocument) -> bool {
    document
        .media
        .iter()
        .any(|m| m.owner == MediaOwner::Source && m.mime.starts_with("audio/"))
}

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '々')
}

/// Some occurrence of `word` in `phrase` is not glued to a neighbouring
/// kanji (a word that starts or ends in kana may touch kanji there).
fn stands_alone(phrase: &str, word: &str) -> bool {
    let (first, last) = (word.chars().next(), word.chars().last());
    // A noun written with trailing kana (物干し) is glued to a kanji or
    // katakana noun right after it (物干し台, 物干しロープ); a verb or
    // adjective ending (見る人, 高い山) is not.
    let inflecting = last.is_some_and(|c| "るうくすつぬぶむぐい".contains(c));
    let katakana = |c: char| matches!(c, '\u{30a1}'..='\u{30fa}' | 'ー');
    phrase.match_indices(word).any(|(at, _)| {
        let before = phrase[..at].chars().last();
        let after = phrase[at + word.len()..].chars().next();
        !(first.is_some_and(is_kanji) && before.is_some_and(is_kanji))
            && !(last.is_some_and(is_kanji) && after.is_some_and(is_kanji))
            && !(!inflecting
                && last.is_some_and(|c| !is_kanji(c))
                && after.is_some_and(|c| is_kanji(c) || katakana(c)))
    })
}

/// Words of three or more letters, lower case, without the commonest ones.
fn words(text: &str) -> std::collections::BTreeSet<String> {
    const COMMON: &[&str] = &[
        "the",
        "and",
        "for",
        "with",
        "one",
        "one's",
        "someone",
        "something",
        "etc",
        "noun",
        "verb",
        "suru",
        "adjective",
        "adverb",
        "usually",
        "written",
        "using",
        "kana",
        "alone",
        "links",
        "details",
        "common",
        "word",
        "jlpt",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| w.chars().count() >= 3 && !COMMON.contains(w))
        .map(str::to_owned)
        .collect()
}

/// The sense, among templates of equal reading, whose definitions share the
/// most words with the OCR text of the note's own images (a dictionary
/// screenshot the learner kept), when one shares strictly more than every
/// other.
fn screenshot_sense<'a>(
    vocab: &linguist_core::Vocabulary,
    document: &LearningDocument,
    matching: &[&'a Value],
) -> Option<&'a Value> {
    if matching.len() < 2 {
        return None;
    }
    let seen = words(
        &document
            .regions
            .iter()
            .map(|region| region.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );
    if seen.is_empty() {
        return None;
    }
    let mut scored: Vec<(usize, &'a Value)> = matching
        .iter()
        .map(|template| {
            let key = template["choice"]["value"]["key"]
                .as_str()
                .unwrap_or_default();
            let definitions = vocab
                .dictionary
                .iter()
                .flat_map(|entry| &entry.senses)
                .filter(|sense| sense.key == key)
                .flat_map(|sense| &sense.definitions)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            (words(&definitions).intersection(&seen).count(), *template)
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    (scored[0].0 > 0 && scored[0].0 > scored[1].0).then_some(scored[0].1)
}

/// A prefix, suffix or counter (副, 総, 代) is only ever written inside
/// compounds (副社長, 総人口, 電気代): the compound rules do not apply.
fn affix(vocab: &linguist_core::Vocabulary) -> bool {
    let expression = spoken(&vocab.expression);
    vocab
        .dictionary
        .iter()
        .filter(|entry| entry.forms.iter().any(|form| spoken(form) == expression))
        .flat_map(|entry| &entry.senses)
        .flat_map(|sense| &sense.labels)
        .any(|label| matches!(label.as_str(), "Prefix" | "Suffix" | "Counter"))
}

/// True when a generated collocation or nuance list names the expression
/// (or its reading) itself as a phrase or contrast.
fn repeats_the_word(document: &LearningDocument, field: &str, claim: &str) -> bool {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return false;
    };
    // An example of a word written in kanji uses the word, not only
    // compounds that hold it (球 drew 球体 and 球場).
    // An example uses the word at all, in some form (直ちに drew a sentence
    // with たちまち): its written form, its reading, or their stem.
    if field == "examples" {
        let expression = spoken(&vocab.expression);
        let Some(sentence) = serde_json::from_str::<Value>(claim)
            .ok()
            .and_then(|v| v["sentence"].as_str().map(spoken))
        else {
            return false;
        };
        let stem = |t: &str| -> String {
            let chars: Vec<char> = t.chars().collect();
            if chars.len() > 1
                && chars
                    .last()
                    .is_some_and(|c| matches!(c, '\u{3040}'..='\u{309f}'))
            {
                chars[..chars.len() - 1].iter().collect()
            } else {
                t.to_owned()
            }
        };
        let reading = spoken(&vocab.reading);
        let mut forms = vec![
            expression.clone(),
            reading.clone(),
            stem(&expression),
            stem(&reading),
        ];
        // The written forms of the entry holding the selected sense count too:
        // a note written in kana (ほうとう) is used in kanji (宝刀) in a sentence.
        for form in vocab
            .dictionary
            .iter()
            .filter(|entry| entry.senses.iter().any(|s| s.key == vocab.sense_key))
            .flat_map(|entry| &entry.forms)
        {
            forms.push(spoken(form));
            forms.push(stem(&spoken(form)));
        }
        // A single kana (た from たま) matches anything: it does not count.
        let telling = |f: &String| f.chars().count() > 1 || f.chars().any(is_kanji);
        if !forms
            .iter()
            .any(|f| telling(f) && sentence.contains(f.as_str()))
        {
            return true;
        }
        return !affix(vocab)
            && expression.chars().any(is_kanji)
            && sentence.contains(&expression)
            && !stands_alone(&sentence, &expression);
    }
    let key = match field {
        "collocations" => "phrase",
        "nuance" => "expression",
        _ => return false,
    };
    let own: Vec<String> = [&vocab.expression, &vocab.reading]
        .into_iter()
        .map(|t| spoken(t))
        .filter(|t| !t.is_empty())
        .collect();
    let Some(items) = serde_json::from_str::<Value>(claim)
        .ok()
        .and_then(|v| v.as_array().cloned())
    else {
        return false;
    };
    let phrases: Vec<String> = items
        .iter()
        .filter_map(|item| item[key].as_str().map(spoken))
        .collect();
    // The word itself, as a phrase or a contrast; one of its own forms is no
    // contrast (節約 and 節約する, 綺麗 and 綺麗な) and no collocation either
    // (遠慮する for 遠慮: the word with a suffix, WP-24 spot check).
    if phrases.iter().any(|p| own.contains(p)) {
        return true;
    }
    if phrases.iter().any(|p| {
        own.iter().any(|w| {
            p.strip_prefix(w.as_str()).is_some_and(|rest| {
                ["する", "します", "な", "だ", "です", "に", "の", "的"].contains(&rest)
            })
        })
    }) {
        return true;
    }
    if field != "collocations" {
        // Another spelling of the same word is no contrast (朝ご飯 and
        // 朝御飯): a form of a dictionary entry with the note's reading.
        let reading = hiragana(&spoken(&vocab.reading));
        let spellings: Vec<String> = vocab
            .dictionary
            .iter()
            .filter(|entry| {
                !reading.is_empty()
                    && entry
                        .readings
                        .iter()
                        .any(|r| hiragana(&spoken(r)) == reading)
            })
            .flat_map(|entry| entry.forms.iter().map(|f| spoken(f)))
            .collect();
        return phrases
            .iter()
            .any(|p| spellings.contains(p) || hiragana(p) == reading);
    }
    // A compound that holds the word's kanji is another word (体温 is not
    // a phrase of 体 read からだ): the written form must stand on its own.
    let expression = spoken(&vocab.expression);
    if !expression.is_empty()
        && !affix(vocab)
        && phrases
            .iter()
            .any(|p| p.contains(&expression) && !stands_alone(p, &expression))
    {
        return true;
    }
    // The same phrase twice.
    let unique: std::collections::BTreeSet<&String> = phrases.iter().collect();
    if unique.len() != phrases.len() {
        return true;
    }
    // A collocation of the word contains the word: its written form, its
    // reading, or (inflecting words) their stem without the last kana.
    let stems: Vec<String> = own
        .iter()
        .flat_map(|w| {
            let mut forms = vec![w.to_lowercase()];
            let chars: Vec<char> = w.chars().collect();
            if chars.len() > 1
                && chars
                    .last()
                    .is_some_and(|c| matches!(c, '\u{3040}'..='\u{309f}'))
            {
                forms.push(chars[..chars.len() - 1].iter().collect());
            }
            if w.is_ascii() && chars.len() > 4 {
                forms.push(w.to_lowercase().chars().take(4).collect());
            }
            forms
        })
        .filter(|f| !f.is_empty())
        .collect();
    phrases.iter().any(|p| {
        !stems
            .iter()
            .any(|stem| p.to_lowercase().contains(stem.as_str()))
    })
}

/// Decide one template entry; `Err` is the reason it stays open.
fn decide(
    policy: &Policy,
    plan: &PlanRevision,
    document: &LearningDocument,
    entry: &Value,
) -> Result<Value, String> {
    let issue = &entry["options"]["issue"];
    let code = issue["code"].as_str().unwrap_or_default();
    let templates: Vec<&Value> = entry["options"]["templates"]
        .as_array()
        .map(|t| t.iter().collect())
        .unwrap_or_default();
    let choice = |t: &Value| t["choice"].clone();
    if policy.agent_codes.iter().any(|c| c == code) {
        return Err("AGENT_CODE".into());
    }
    match code {
        "DICTIONARY_SENSE_REVIEW" => {
            if templates.is_empty() {
                return Err("NO_SENSE_CANDIDATE".into());
            }
            let LearningContent::Vocabulary(vocab) = &document.content else {
                return Err("NOT_VOCABULARY".into());
            };
            // A word written in kana alone is its own reading (WP-24: こらえる,
            // やがて and レントゲン came with no reading or pronunciation).
            let kana = |c: char| matches!(c, '\u{3041}'..='\u{309f}' | '\u{30a0}'..='\u{30ff}');
            let expression = vocab.expression.trim();
            let raw = if !vocab.reading.is_empty() {
                &vocab.reading
            } else if !vocab.pronunciation.trim().is_empty() {
                &vocab.pronunciation
            } else if !expression.is_empty() && expression.chars().all(kana) {
                &vocab.expression
            } else {
                &vocab.pronunciation
            };
            // The sense whose reading is the note's own, in dictionary order
            // (common senses first); without a reading match only a single
            // candidate is taken.
            // Equal, never a prefix: だいどこ is not 台所's だいどころ.
            // The whole field first (`から だ` is からだ), then separated
            // readings, then space-separated words.
            let mut own_readings = readings(raw).to_vec();
            // A する verb form looked up by its stem (メモします as メモ) is
            // read without its ending too.
            let word = linguist_core::review::dictionary_form(vocab);
            if let Some(ending) = vocab.expression.trim().strip_prefix(word.as_str())
                && !ending.is_empty()
            {
                let stems: Vec<Vec<String>> = own_readings
                    .iter()
                    .map(|tier| {
                        tier.iter()
                            .filter_map(|r| r.strip_suffix(ending).map(str::to_owned))
                            .filter(|r| !r.is_empty())
                            .collect()
                    })
                    .collect();
                own_readings.extend(stems);
            }
            let equal = |wanted: &[String]| -> Vec<&Value> {
                templates
                    .iter()
                    .copied()
                    .filter(|t| {
                        t["reading"].as_str().is_some_and(|r| {
                            let r = hiragana(&spoken(r));
                            !r.is_empty() && wanted.contains(&r)
                        })
                    })
                    .collect()
            };
            if let Some(matching) = own_readings
                .iter()
                .map(|tier| equal(tier))
                .find(|matching| !matching.is_empty())
            {
                // Several senses share the reading: the one the learner's own
                // dictionary screenshot names (its OCR text) wins outright;
                // otherwise dictionary order (WP-24: 得意 took "pride" when
                // the screenshot said "one's strong point").
                return Ok(choice(
                    screenshot_sense(vocab, document, &matching).unwrap_or(matching[0]),
                ));
            }
            if templates.iter().all(|t| t["reading"].is_null()) {
                return Ok(choice(templates[0]));
            }
            // Every candidate under one reading (一人っ子: ひとりっこ, with a
            // pronunciation field in kanji): no reading is left to decide, so
            // dictionary order chooses, as among equal readings.
            let reading = |t: &Value| t["reading"].as_str().map(|r| hiragana(&spoken(r)));
            // The field is there but holds no kana reading (whole, or between
            // separators; a lone syllable split off by a space does not count).
            let unusable = !raw.trim().is_empty()
                && !own_readings.iter().take(2).flatten().any(|r| {
                    !r.is_empty()
                        && r.chars()
                            .all(|c| matches!(c, '\u{3041}'..='\u{309f}' | 'ー'))
                });
            if templates.len() == 1
                || unusable
                    && templates
                        .iter()
                        .all(|t| reading(t).is_some() && reading(t) == reading(templates[0]))
            {
                return Ok(choice(
                    screenshot_sense(vocab, document, &templates).unwrap_or(templates[0]),
                ));
            }
            Err("SENSE_READING_UNMATCHED: no candidate reading equals the note's reading".into())
        }
        "SOURCE_MEDIA_CONTENT_REVIEW" | "SOURCE_MEDIA_FORMAT_REVIEW" => {
            let field = issue["field"].as_str().unwrap_or_default();
            let asset = document
                .media
                .iter()
                .find(|m| m.original_filename.as_deref() == Some(field) || m.filename == field)
                .ok_or("SOURCE_ASSET_NOT_FOUND")?;
            let evidence = document
                .evidence
                .iter()
                .find(|e| {
                    e.field == "media_format"
                        && matches!(&e.target, Some(linguist_core::records::EvidenceTarget::MediaAsset { digest }) if *digest == asset.digest)
                })
                .ok_or("SOURCE_MEDIA_EVIDENCE_MISSING")?;
            let role = if code == "SOURCE_MEDIA_FORMAT_REVIEW" {
                "archive"
            } else if asset.mime.starts_with("audio/") {
                if policy.keep_own_audio {
                    "audio"
                } else {
                    "archive"
                }
            } else if asset.mime.starts_with("image/") {
                picture_role(policy, document, asset)?
            } else {
                "archive"
            };
            let source_id = issue["source_refs"][0].clone();
            Ok(json!({"decision": "source_media_role", "value": {
                "source_id": source_id, "asset_digest": asset.digest,
                "original_filename": field, "evidence_id": evidence.id, "role": role,
                "attribution": "User's own collection media (previous note).", "license": null}}))
        }
        "IMAGE_CANDIDATE_REVIEW" => {
            let refs = issue["source_refs"].as_array().cloned().unwrap_or_default();
            let value = if keeps_own_picture(policy, document) {
                String::new()
            } else {
                refs.first()
                    .and_then(Value::as_str)
                    .ok_or("NO_IMAGE_CANDIDATE")?
                    .to_owned()
            };
            Ok(json!({"decision": "media", "value": value}))
        }
        "AUDIO_CANDIDATE_REVIEW" => {
            let refs = issue["source_refs"].as_array().cloned().unwrap_or_default();
            let decline = policy.keep_own_audio
                && matches!(document.content, LearningContent::Vocabulary(_))
                && has_own_audio(document);
            let value = if decline {
                String::new()
            } else {
                refs.first()
                    .and_then(Value::as_str)
                    .ok_or("NO_AUDIO_CANDIDATE")?
                    .to_owned()
            };
            Ok(json!({"decision": "media", "value": value}))
        }
        "GENERATED_FACT_REVIEW" => {
            if !policy.accept_generated_facts {
                return Err("GENERATED_FACTS_NEED_REVIEW".into());
            }
            // A collocation or contrast that is the word itself is a known
            // local-model failure (WP-20): reject it mechanically.
            if let Some(rejected) = templates
                .iter()
                .find(|t| t["choice"]["decision"] == "content_rejected")
                && let Some(id) = rejected["choice"]["value"]["evidence_id"].as_str()
                && let Some(evidence) = document.evidence.iter().find(|e| e.id.to_string() == id)
                && repeats_the_word(document, &evidence.field, &evidence.claim)
            {
                return Ok(choice(rejected));
            }
            templates
                .iter()
                .find(|t| t["choice"]["decision"] == "content_verified")
                .map(|t| choice(t))
                .ok_or("NO_VERIFY_TEMPLATE".into())
        }
        // OCR text of a vocabulary picture is evidence only (the card's
        // content comes from the dictionary); grammar cards are made of it,
        // so the agent compares it with the screenshot.
        "OCR_TEXT_REVIEW" | "OCR_FAILED_REVIEW" => match document.content {
            LearningContent::Vocabulary(_) => templates
                .first()
                .map(|t| choice(t))
                .ok_or("NO_TEMPLATE".into()),
            LearningContent::Grammar(_) => Err("GRAMMAR_OCR_NEEDS_AGENT".into()),
        },
        "GRAMMAR_SPLIT_NATIVE_REVIEW" => plan
            .grammar_groups
            .iter()
            .find(|g| g.units.contains(&document.id))
            .map(|g| json!({"decision": "anchor", "value": g.anchor_document}))
            .ok_or("NO_SPLIT_GROUP".into()),
        _ if policy.accept_first_template.iter().any(|c| c == code) => templates
            .first()
            .map(|t| choice(t))
            .ok_or("NO_TEMPLATE".into()),
        _ => Err("NO_RULE".into()),
    }
}

/// Fill `template` (from `batch::template`) for `plan`.
pub fn fill(policy: &Policy, plan: &PlanRevision, template: &Value) -> Result<Filled, String> {
    policy.validate()?;
    let mut decided = Vec::new();
    let mut undecided = Vec::new();
    for entry in template["decisions"].as_array().into_iter().flatten() {
        let document_id: uuid::Uuid = serde_json::from_value(entry["document_id"].clone())
            .map_err(|_| "REVIEW_TEMPLATE_INVALID")?;
        let issue_id = entry["issue_id"].as_str().unwrap_or_default().to_owned();
        let code = entry["options"]["issue"]["code"]
            .as_str()
            .unwrap_or_else(|| issue_id.split(':').next().unwrap_or_default())
            .to_owned();
        let mut out = json!({
            "document_id": entry["document_id"],
            "issue_id": entry["issue_id"],
            "input_digest": entry["input_digest"],
        });
        if entry.get("history_map").is_some() {
            // The map names vocabulary tasks; a grammar note's single card
            // is its Recognition card (Grammar v4 has no other task).
            let grammar = plan
                .documents
                .iter()
                .find(|d| d.id == document_id)
                .is_some_and(|d| matches!(d.content, LearningContent::Grammar(_)));
            out["history_map"] = if grammar {
                json!(["0=recognition"])
            } else {
                json!(policy.history_map)
            };
            decided.push(out);
            continue;
        }
        if entry["expect_resolved"] == true {
            out["expect_resolved"] = true.into();
            decided.push(out);
            continue;
        }
        let document = plan
            .documents
            .iter()
            .find(|d| d.id == document_id)
            .ok_or("REVIEW_DOCUMENT_NOT_FOUND")?;
        match decide(policy, plan, document, entry) {
            Ok(choice) => {
                out["choice"] = choice;
                decided.push(out);
            }
            Err(reason) => undecided.push(Undecided {
                document_id,
                issue_id,
                code,
                reason,
            }),
        }
    }
    // An assertion whose document has an undecided issue cannot be checked
    // yet; it waits with the rest of that document.
    let blocked: std::collections::BTreeSet<uuid::Uuid> =
        undecided.iter().map(|u| u.document_id).collect();
    decided.retain(|d| {
        !(d["expect_resolved"] == true
            && serde_json::from_value::<uuid::Uuid>(d["document_id"].clone())
                .is_ok_and(|id| blocked.contains(&id)))
    });
    let count = decided.len();
    let only_assertions = decided.iter().all(|d| d["expect_resolved"] == true);
    let batch = (!decided.is_empty() && !only_assertions).then(|| {
        json!({
            "schema_version": 2,
            "base_revision": template["base_revision"],
            "base_digest": template["base_digest"],
            "actor": policy.actor,
            "decisions": decided,
        })
    });
    Ok(Filled {
        batch,
        decided: if only_assertions { 0 } else { count },
        undecided,
    })
}

#[cfg(test)]
mod compound_tests {
    use super::stands_alone;

    #[test]
    fn a_word_inside_a_compound_does_not_stand_alone() {
        // Kanji words (WP-24: 体温 for 体, 球体 and 球場 for 球).
        assert!(!stands_alone("体温を測る", "体"));
        assert!(!stands_alone("この球体は滑らかだ。", "球"));
        assert!(stands_alone("野球の球を投げた。", "球"));
        // A noun with trailing kana glued to a noun (物干し台, 物干しロープ).
        assert!(!stands_alone("物干し台", "物干し"));
        assert!(!stands_alone("物干しロープ", "物干し"));
        assert!(stands_alone("物干しに干す", "物干し"));
        // A verb or adjective before a noun is a phrase.
        assert!(stands_alone("見る人", "見る"));
        assert!(stands_alone("高い山に登る", "高い"));
    }

    #[test]
    fn an_example_with_the_word_only_inside_compounds_is_rejected() {
        let mut document = linguist_core::LearningDocument::from_json(include_bytes!(
            "../../../../contracts/v2/fixtures/vocabulary.json"
        ))
        .unwrap();
        let linguist_core::LearningContent::Vocabulary(v) = &mut document.content else {
            panic!()
        };
        v.expression = "球".into();
        v.reading = "たま".into();
        let example = |s: &str| format!(r#"{{"sentence":"{s}","translation":"x"}}"#);
        let rejected = |s: &str| super::repeats_the_word(&document, "examples", &example(s));
        assert!(rejected("球場は観客で埋め尽くされた。"));
        assert!(!rejected("彼は野球の球を投げた。"));
        // No form of the word at all (直ちに: a sentence with たちまち).
        assert!(rejected("大雨で、たちまち水浸しになった。"));
        // Written in kana only: nothing to judge.
        assert!(!rejected("たまを投げた。"));
        // A prefix is only ever written inside compounds (副社長).
        let linguist_core::LearningContent::Vocabulary(v) = &mut document.content else {
            panic!()
        };
        v.expression = "副".into();
        v.reading = "ふく".into();
        v.dictionary = vec![
            serde_json::from_value(serde_json::json!({
                "provider": "jisho", "source_url": "https://jisho.org/search/副",
                "language": "ja", "forms": ["副"], "readings": ["ふく"],
                "senses": [{"key": "1", "definitions": ["vice-"], "labels": ["Noun", "Prefix"]}]
            }))
            .unwrap(),
        ];
        assert!(!super::repeats_the_word(
            &document,
            "examples",
            &example("彼は副社長になった。")
        ));
    }
}

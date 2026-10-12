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

/// True when a generated collocation or nuance list names the expression
/// (or its reading) itself as a phrase or contrast.
fn repeats_the_word(document: &LearningDocument, field: &str, claim: &str) -> bool {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return false;
    };
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
    // The word itself, as a phrase or a contrast.
    if phrases.iter().any(|p| own.contains(p)) {
        return true;
    }
    if field != "collocations" {
        return false;
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
            let own = spoken(if vocab.reading.is_empty() {
                &vocab.pronunciation
            } else {
                &vocab.reading
            });
            // The sense whose reading is the note's own, in dictionary order
            // (common senses first); without a reading match only a single
            // candidate is taken.
            if let Some(t) = templates.iter().find(|t| {
                t["reading"]
                    .as_str()
                    .is_some_and(|r| !own.is_empty() && own.contains(&spoken(r)))
            }) {
                return Ok(choice(t));
            }
            if templates.iter().all(|t| t["reading"].is_null()) {
                return Ok(choice(templates[0]));
            }
            if templates.len() == 1 {
                return Ok(choice(templates[0]));
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
            out["history_map"] = json!(policy.history_map);
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

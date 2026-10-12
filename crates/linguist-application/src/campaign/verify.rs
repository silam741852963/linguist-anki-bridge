//! Post-apply verification from companion note evidence (`inspect_note`).
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardFacts {
    pub id: String,
    pub ordinal: u16,
    pub repetitions: u32,
    /// Review log row IDs, oldest first.
    pub reviews: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteFacts {
    pub note_id: String,
    pub model: String,
    pub cards: Vec<CardFacts>,
    /// Referenced media files Anki does not have.
    pub missing_media: Vec<String>,
}

fn id_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Read the facts verification compares from `inspect_note` evidence.
pub fn facts(evidence: &Value) -> Result<NoteFacts, String> {
    let bad = || "CAMPAIGN_EVIDENCE_INVALID".to_owned();
    let mut cards = Vec::new();
    for card in evidence["cards"].as_array().ok_or_else(bad)? {
        cards.push(CardFacts {
            id: id_text(&card["id"]).ok_or_else(bad)?,
            ordinal: card["ordinal"].as_u64().ok_or_else(bad)? as u16,
            repetitions: card["repetitions"].as_u64().ok_or_else(bad)? as u32,
            reviews: card["reviews"]
                .as_array()
                .ok_or_else(bad)?
                .iter()
                .map(|r| id_text(&r["id"]).ok_or_else(bad))
                .collect::<Result<_, _>>()?,
        });
    }
    Ok(NoteFacts {
        note_id: id_text(&evidence["note_id"]).ok_or_else(bad)?,
        model: evidence["model"]["name"]
            .as_str()
            .ok_or_else(bad)?
            .to_owned(),
        cards,
        missing_media: evidence["media"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["missing"] == true)
            .filter_map(|m| m["name"].as_str().map(str::to_owned))
            .collect(),
    })
}

/// Problems with one revamped note; empty when it kept every card and review.
///
/// Reviews done while the batch ran are expected: a card may gain
/// repetitions and review rows, but never lose one.
pub fn compare(before: &NoteFacts, after: Option<&NoteFacts>, targets: &[String]) -> Vec<String> {
    let note = &before.note_id;
    let Some(after) = after else {
        return vec![format!("{note}: note is gone")];
    };
    let mut problems = Vec::new();
    if !targets.contains(&after.model) {
        problems.push(format!(
            "{note}: model is {} (expected one of {targets:?})",
            after.model
        ));
    }
    for card in &before.cards {
        let Some(now) = after.cards.iter().find(|c| c.id == card.id) else {
            problems.push(format!("{note}: card {} is gone", card.id));
            continue;
        };
        if now.repetitions < card.repetitions {
            problems.push(format!(
                "{note}: card {} repetitions {} -> {}",
                card.id, card.repetitions, now.repetitions
            ));
        }
        let kept: std::collections::BTreeSet<&String> = now.reviews.iter().collect();
        let lost = card.reviews.iter().filter(|r| !kept.contains(r)).count();
        if lost > 0 {
            problems.push(format!("{note}: card {} lost {lost} review rows", card.id));
        }
    }
    for missing in &after.missing_media {
        problems.push(format!("{note}: media {missing} is missing"));
    }
    problems
}

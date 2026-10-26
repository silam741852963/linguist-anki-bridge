//! WP-28: a grammar-cohort item whose pages are a rephrasing drill (a plain
//! sentence and its polite versions) is rewritten as a drill in place. The
//! reviewer writes the drill from the pages; this module cuts the first
//! answer into tap-build chunks and makes the decoys. No provider is called.
use crate::grammar::{SplitKind, SplitUnit, publish_split};
use linguist_core::{records::PlanRevision, *};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrillRequest {
    pub schema_version: u16,
    pub base_revision: u32,
    pub base_digest: String,
    pub document_id: uuid::Uuid,
    pub input_digest: String,
    pub actor: String,
    /// The drill as written; empty `chunks` and `decoys` are made here.
    pub drill: Drill,
}

/// Most decoys on a board: more would turn the drill into a search.
pub const MAX_DECOYS: usize = 3;

/// Tap-build data for `drill`: the first answer cut into chunks, a change's
/// polite part kept whole inside one chunk, and one decoy per change made by
/// putting the plain part back into that chunk (ちょうだいした against
/// もらった). A decoy that is also text of an accepted answer, or a chunk,
/// is dropped.
pub fn build(drill: &Drill) -> Result<(Vec<String>, Vec<String>), String> {
    let first = drill
        .answers
        .first()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .ok_or("DRILL_ANSWER_REQUIRED")?;
    let japanese = first
        .chars()
        .any(|c| linguist_text::is_kana(c) || linguist_text::is_kanji(c));
    let mut chunks = if japanese {
        linguist_text::ja::chunks(first)?
    } else {
        first
            .split_inclusive(' ')
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    // A polite part that crosses chunks is joined into one chunk, so its
    // decoy replaces exactly one tile.
    for change in &drill.changes {
        let polite = change.polite.trim();
        let Some(start) = first.find(polite).filter(|_| !polite.is_empty()) else {
            continue;
        };
        let end = start + polite.len();
        let mut offset = 0;
        let mut run: Option<(usize, usize)> = None;
        for (i, chunk) in chunks.iter().enumerate() {
            let (from, to) = (offset, offset + chunk.len());
            if to > start && from < end {
                run = Some(run.map_or((i, i), |(a, _)| (a, i)));
            }
            offset = to;
        }
        if let Some((a, b)) = run.filter(|(a, b)| a < b) {
            let joined: String = chunks.drain(a..=b).collect();
            chunks.insert(a, joined);
        }
    }
    if chunks.len() < 2 {
        return Err("DRILL_CHUNKS_TOO_FEW: the first answer is too short to build".into());
    }
    let mut decoys: Vec<String> = Vec::new();
    for change in &drill.changes {
        let (polite, plain) = (change.polite.trim(), change.plain.trim());
        if polite.is_empty() || plain.is_empty() {
            continue;
        }
        let Some(chunk) = chunks.iter().find(|chunk| chunk.contains(polite)) else {
            continue;
        };
        let decoy = chunk.replacen(polite, plain, 1);
        if decoys.len() < MAX_DECOYS
            && !decoys.contains(&decoy)
            && !chunks.contains(&decoy)
            && !drill.answers.iter().any(|a| a.contains(decoy.as_str()))
        {
            decoys.push(decoy);
        }
    }
    Ok((chunks, decoys))
}

/// Rewrite one grammar-cohort item as the drill of `request`, keeping its
/// note, card and archived pages (one unit, no split group).
pub fn convert(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    request: &DrillRequest,
    raw: &[u8],
) -> Result<PlanRevision, String> {
    let parsed: DrillRequest = canonical::parse(raw).map_err(|e| e.to_string())?;
    if canonical::bytes(&parsed).map_err(|e| e.to_string())?
        != canonical::bytes(request).map_err(|e| e.to_string())?
        || request.schema_version != 1
        || request.base_revision != base.revision
        || request.base_digest != base.approval_digest().map_err(|e| e.to_string())?
        || store.latest_revision(base.id)? != base.revision
        || store.revision(base.id, base.revision)? != *base
    {
        return Err("DRILL_CONVERT_BASE_CONFLICT".into());
    }
    let original = base
        .documents
        .iter()
        .find(|document| document.id == request.document_id)
        .ok_or("DRILL_CONVERT_DOCUMENT_MISSING")?;
    if !matches!(
        original.content,
        LearningContent::Grammar(_) | LearningContent::Drill(_)
    ) || request.input_digest != original.semantic_digest().map_err(|e| e.to_string())?
        || request.actor.trim().is_empty()
        || base
            .grammar_groups
            .iter()
            .any(|group| group.units.contains(&original.id))
    {
        return Err("DRILL_CONVERT_INPUT_CONFLICT".into());
    }
    let mut drill = request.drill.clone();
    drill.prompt = drill.prompt.trim().to_owned();
    for answer in &mut drill.answers {
        *answer = answer.trim().to_owned();
    }
    if drill.chunks.is_empty() {
        let (chunks, decoys) = build(&drill)?;
        drill.chunks = chunks;
        if drill.decoys.is_empty() {
            drill.decoys = decoys;
        }
    }
    let problems = validation::drill_problems(&drill);
    if !problems.is_empty() {
        return Err(format!(
            "DRILL_CONVERT_INVALID: {}",
            problems
                .iter()
                .map(|(code, _, _)| *code)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    publish_split(
        store,
        base,
        original,
        raw,
        &request.actor,
        0,
        SplitKind::Drill,
        vec![SplitUnit {
            content: LearningContent::Drill(drill),
            sibling_tasks: vec![Task::Production],
            keep_media: None,
        }],
    )
}

/// An editable request for one item: the identity filled in, the drill
/// empty for the reviewer to write from the pages.
pub fn template(plan: &PlanRevision, document_id: uuid::Uuid) -> Result<DrillRequest, String> {
    let document = plan
        .documents
        .iter()
        .find(|document| document.id == document_id)
        .ok_or("PLAN_ITEM_NOT_FOUND")?;
    Ok(DrillRequest {
        schema_version: 1,
        base_revision: plan.revision,
        base_digest: plan.approval_digest().map_err(|e| e.to_string())?,
        document_id,
        input_digest: document.semantic_digest().map_err(|e| e.to_string())?,
        actor: String::new(),
        drill: Drill {
            instruction: "Business keigo".into(),
            ..Default::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keigo() -> Drill {
        Drill {
            prompt: "さっきもらった見積書について、3つ確認したいことがあるんですが。".into(),
            instruction: "Business keigo".into(),
            answers: vec![
                "先程ちょうだいした見積書につきまして、3点ほど確認させていただきたいことがあるのですが。"
                    .into(),
                "先程いただいた見積書につきまして、3点ほど確認させていただきたいことがあるのですが。"
                    .into(),
            ],
            changes: vec![
                DrillChange {
                    plain: "さっき".into(),
                    polite: "先程".into(),
                    kind: "formal".into(),
                    note: String::new(),
                },
                DrillChange {
                    plain: "もらった".into(),
                    polite: "ちょうだいした".into(),
                    kind: "humble".into(),
                    note: String::new(),
                },
                DrillChange {
                    plain: "について".into(),
                    polite: "につきまして".into(),
                    kind: "polite".into(),
                    note: String::new(),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn the_first_answer_is_cut_and_each_change_gives_one_decoy() {
        let mut drill = keigo();
        let (chunks, decoys) = build(&drill).unwrap();
        assert_eq!(chunks.concat(), drill.answers[0]);
        assert!(chunks.contains(&"ちょうだいした".to_owned()), "{chunks:?}");
        assert_eq!(decoys, ["さっき", "もらった", "見積書について、"]);
        drill.chunks = chunks;
        drill.decoys = decoys;
        assert!(validation::drill_problems(&drill).is_empty());
    }

    #[test]
    fn a_decoy_that_another_answer_accepts_is_dropped() {
        let mut drill = keigo();
        // "いただいた" is accepted, so it cannot be the decoy of ちょうだいした.
        drill.changes[1].plain = "いただいた".into();
        let (_, decoys) = build(&drill).unwrap();
        assert!(!decoys.contains(&"いただいた".to_owned()));
    }

    #[test]
    fn a_polite_part_across_chunks_becomes_one_chunk() {
        let mut drill = keigo();
        drill.changes.push(DrillChange {
            plain: "確認したい".into(),
            polite: "確認させていただきたいことがある".into(),
            kind: "humble".into(),
            note: String::new(),
        });
        let (chunks, _) = build(&drill).unwrap();
        assert!(
            chunks
                .iter()
                .any(|c| c.contains("確認させていただきたいことがある")),
            "{chunks:?}"
        );
        assert_eq!(chunks.concat(), drill.answers[0]);
    }
}

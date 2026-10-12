//! Cohort rules and batch building (pure; the caller reads Anki).
use super::{BatchData, Spec};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Cohort classes, in precedence order.
pub const CLASSES: [&str; 5] = [
    "duplicate",
    "split_candidate",
    "kana_only",
    "missing_media",
    "plain",
];

/// What the cohort rules read of one note (from `notesInfo`/`cardsInfo`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteInfo {
    pub id: String,
    pub model: String,
    pub fields: BTreeMap<String, String>,
    /// Sum of the note's card repetitions (survivor choice for merges).
    pub reps: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SelectionReport {
    /// Matched notes per source.
    pub matched: BTreeMap<String, usize>,
    /// Notes per cohort.
    pub cohorts: BTreeMap<String, usize>,
    /// Notes in no ordered cohort (left out).
    pub unordered: BTreeMap<String, usize>,
    /// Notes matched by two sources (kept in the first).
    pub overlaps: usize,
    pub merge_groups: Vec<Vec<String>>,
    pub batches: usize,
}

/// Visible text of a field: tags, `&nbsp;` and sound markers removed,
/// whitespace collapsed.
pub fn text(value: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    let mut rest = value.replace("&nbsp;", " ");
    while let Some(start) = rest.find("[sound:") {
        match rest[start..].find(']') {
            Some(end) => rest.replace_range(start..start + end + 1, " "),
            None => break,
        }
    }
    for c in rest.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => {
                tag = false;
                out.push(' ');
            }
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30ff}' | '〜' | '～')
}

/// Several words in one note (a group, a variant list or a frame).
pub fn split_candidate(language: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    if language == "ja" {
        // Spaces between single characters are formatting (`朝  ご  飯`).
        let long_tokens = word.split(' ').filter(|t| t.chars().count() > 1).count();
        long_tokens >= 2
            || word.contains(['、', ',', '，', '/', '／', ';', '；'])
            || (word.contains(['(', '（']) && word.contains([')', '）']))
    } else {
        word.contains([';', '\n']) || word.contains(',') || word.contains('/')
    }
}

fn has(value: Option<&String>, marker: &str) -> bool {
    value.is_some_and(|v| v.contains(marker))
}

/// The note's class within its source.
pub fn classify(source: &super::SourceSpec, note: &NoteInfo, duplicate: bool) -> &'static str {
    if duplicate {
        return "duplicate";
    }
    let field = |name: &Option<String>| name.as_ref().and_then(|n| note.fields.get(n));
    let word = field(&source.word_field)
        .map(|w| text(w))
        .unwrap_or_default();
    if source.word_field.is_some() && split_candidate(&source.language, &word) {
        return "split_candidate";
    }
    if source.language == "ja"
        && !word.is_empty()
        && word.chars().filter(|c| !c.is_whitespace()).all(is_kana)
    {
        return "kana_only";
    }
    if (source.audio_field.is_some() && !has(field(&source.audio_field), "[sound:"))
        || (source.picture_field.is_some() && !has(field(&source.picture_field), "<img"))
    {
        return "missing_media";
    }
    "plain"
}

/// Same word and reading inside one source: merge groups, survivor first
/// (most repetitions, then the oldest note).
pub fn duplicate_groups(source: &super::SourceSpec, notes: &[NoteInfo]) -> Vec<Vec<String>> {
    let (Some(word), true) = (&source.word_field, source.merge_duplicates) else {
        return vec![];
    };
    let mut groups: BTreeMap<(String, String), Vec<&NoteInfo>> = BTreeMap::new();
    for note in notes {
        let w: String = text(note.fields.get(word).map(String::as_str).unwrap_or(""))
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if w.is_empty() {
            continue;
        }
        let r: String = source
            .reading_field
            .as_ref()
            .and_then(|f| note.fields.get(f))
            .map(|v| {
                text(v)
                    .chars()
                    .filter(|c| !c.is_whitespace() && *c != '/')
                    .collect()
            })
            .unwrap_or_default();
        groups.entry((w, r)).or_default().push(note);
    }
    groups
        .into_values()
        .filter(|g| g.len() > 1)
        .map(survivor_first)
        .collect()
}

fn survivor_first(mut group: Vec<&NoteInfo>) -> Vec<String> {
    group.sort_by(|a, b| {
        b.reps.cmp(&a.reps).then_with(|| {
            a.id.parse::<u64>()
                .unwrap_or(u64::MAX)
                .cmp(&b.id.parse::<u64>().unwrap_or(u64::MAX))
        })
    });
    group.into_iter().map(|n| n.id.clone()).collect()
}

/// Build the frozen batches from each source's matched notes.
pub fn build(
    spec: &Spec,
    matched: &BTreeMap<String, Vec<NoteInfo>>,
) -> Result<(Vec<(String, BatchData)>, SelectionReport), String> {
    let mut report = SelectionReport::default();
    let mut seen = BTreeSet::new();
    let mut cohorts: BTreeMap<String, Vec<NoteInfo>> = BTreeMap::new();
    let mut merges: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    for source in &spec.sources {
        let notes: Vec<NoteInfo> = matched
            .get(&source.id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|n| {
                let fresh = seen.insert(n.id.clone());
                if !fresh {
                    report.overlaps += 1;
                }
                fresh
            })
            .collect();
        report.matched.insert(source.id.clone(), notes.len());
        let mut groups = duplicate_groups(source, &notes);
        // Explicit groups (other note types) join; a note is in one group.
        let by_id: BTreeMap<&str, &NoteInfo> = notes.iter().map(|n| (n.id.as_str(), n)).collect();
        for extra in spec.merge_extra.iter().filter(|m| m.source == source.id) {
            let members: Vec<&NoteInfo> = extra
                .notes
                .iter()
                .filter_map(|id| by_id.get(id.as_str()).copied())
                .collect();
            if members.len() != extra.notes.len() {
                return Err(format!(
                    "CAMPAIGN_MERGE_NOTE_NOT_SELECTED: every note of {:?} must match source {}",
                    extra.notes, source.id
                ));
            }
            groups.retain(|g| !g.iter().any(|id| extra.notes.contains(id)));
            let mut group = survivor_first(members);
            if let Some(survivor) = &extra.survivor {
                group.retain(|id| id != survivor);
                group.insert(0, survivor.clone());
            }
            groups.push(group);
        }
        let grouped: BTreeSet<&String> = groups.iter().flatten().collect();
        for note in &notes {
            let class = classify(source, note, grouped.contains(&note.id));
            cohorts
                .entry(format!("{}:{class}", source.id))
                .or_default()
                .push(note.clone());
        }
        report.merge_groups.extend(groups.iter().cloned());
        merges.insert(source.id.clone(), groups);
    }
    for (cohort, notes) in &cohorts {
        report.cohorts.insert(cohort.clone(), notes.len());
        if !spec.order.contains(cohort) {
            report.unordered.insert(cohort.clone(), notes.len());
        }
    }
    let mut batches = Vec::new();
    let mut pilot_taken: BTreeSet<String> = BTreeSet::new();
    if let Some(pilot) = &spec.pilot {
        let notes = cohorts.get(&pilot.cohort).cloned().unwrap_or_default();
        let (source, class) = pilot.cohort.split_once(':').unwrap();
        if class == "duplicate" {
            return Err("CAMPAIGN_SPEC_INVALID: the pilot cannot be a merge cohort".into());
        }
        let ids: Vec<String> = notes
            .iter()
            .take(pilot.size as usize)
            .map(|n| n.id.clone())
            .collect();
        if !ids.is_empty() {
            pilot_taken.extend(ids.iter().cloned());
            batches.push((
                pilot.cohort.clone(),
                BatchData {
                    source: source.into(),
                    split_candidates: if class == "split_candidate" {
                        ids.clone()
                    } else {
                        vec![]
                    },
                    notes: ids,
                    pilot: true,
                    ..Default::default()
                },
            ));
        }
    }
    for cohort in &spec.order {
        let (source, class) = cohort.split_once(':').unwrap();
        if class == "duplicate" {
            for group in merges.get(source).into_iter().flatten() {
                batches.push((
                    cohort.clone(),
                    BatchData {
                        source: source.into(),
                        notes: vec![group[0].clone()],
                        merge: group[1..].to_vec(),
                        ..Default::default()
                    },
                ));
            }
            continue;
        }
        let size = spec
            .batch_sizes
            .get(source)
            .copied()
            .unwrap_or(spec.batch_size) as usize;
        let ids: Vec<String> = cohorts
            .get(cohort)
            .into_iter()
            .flatten()
            .map(|n| n.id.clone())
            .filter(|id| !pilot_taken.contains(id))
            .collect();
        for chunk in ids.chunks(size) {
            batches.push((
                cohort.clone(),
                BatchData {
                    source: source.into(),
                    notes: chunk.to_vec(),
                    split_candidates: if class == "split_candidate" {
                        chunk.to_vec()
                    } else {
                        vec![]
                    },
                    ..Default::default()
                },
            ));
        }
    }
    report.batches = batches.len();
    Ok((batches, report))
}

//! Deterministic task-cue suggestions derived from current vocabulary facts.
//!
//! A suggestion is offered as a review template, never written silently: the
//! validator requires a reviewed cue. Every suggestion is leak-checked against
//! the answer with the same rule as validation.
use crate::{LearningContent, LearningDocument, Task, validation::answer_leaks};

fn has_kanji(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x323AF))
}

fn is_kana(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| {
            matches!(c as u32, 0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F) || c == 'ー'
        })
}

/// Suggest a cue for `task`, or `None` when facts are missing or every
/// candidate would expose the answer.
pub fn suggest(document: &LearningDocument, task: Task) -> Option<String> {
    let LearningContent::Vocabulary(vocab) = &document.content else {
        return None;
    };
    let meaning = vocab.meaning.trim();
    if vocab.expression.trim().is_empty() || meaning.is_empty() {
        return None;
    }
    let japanese = document.target_language.as_str().split('-').next() == Some("ja");
    let reading = vocab.reading.trim();
    let pronunciation = vocab.pronunciation.trim();
    let candidates: Vec<String> = match task {
        // Production recalls the expression from its meaning (and picture, when rendered).
        Task::Production => vec![meaning.to_owned()],
        // Spelling gives a reading/audio prompt: kana for kanji words, otherwise
        // the pronunciation, then meaning alone.
        Task::Spelling => {
            let mut options = Vec::new();
            if japanese && has_kanji(&vocab.expression) && is_kana(reading) {
                options.push(format!("{reading} — {meaning}"));
            }
            if !pronunciation.is_empty() {
                options.push(format!("{pronunciation} — {meaning}"));
            }
            options.push(meaning.to_owned());
            options
        }
        _ => return None,
    };
    candidates
        .into_iter()
        .find(|cue| !answer_leaks(cue, &vocab.expression, &document.target_language))
}

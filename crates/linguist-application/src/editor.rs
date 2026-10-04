//! OP-28 `plans edit --editor`: a private typed draft edited in an external
//! editor. The editor is started from an explicit argument vector; no shell
//! interprets it. Aborting (non-zero exit) leaves the parent untouched.
use linguist_core::{
    editing::{ItemPatch, PlanPatch},
    records::PlanRevision,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

/// Fields a typed patch may set, shown with their current effective values.
const EDITABLE: [&str; 6] = [
    "Meaning",
    "Reading",
    "Pronunciation",
    "Usage",
    "Kanji",
    "Formation",
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    /// Read-only reference; changes here are ignored.
    #[serde(default)]
    pub current_values: BTreeMap<String, BTreeMap<String, String>>,
    /// The typed patch that is applied. Use {"intent":"set","value":"..."},
    /// {"intent":"clear"} or {"intent":"keep"} per field.
    pub patch: PlanPatch,
}

/// Build the draft for every item: an empty (no-op) patch plus current values.
pub fn draft(plan: &PlanRevision) -> Result<Vec<u8>, String> {
    let mut current = BTreeMap::new();
    let mut items = Vec::new();
    for document in &plan.documents {
        let fields = linguist_core::model::for_document(document).fields;
        let values = EDITABLE
            .iter()
            .filter(|key| fields.contains(&(**key).to_owned()))
            .map(|key| ((*key).to_owned(), current_value(document, key)))
            .collect();
        current.insert(document.id.to_string(), values);
        items.push(ItemPatch {
            document_id: document.id,
            fields: BTreeMap::new(),
            personal_notes: Default::default(),
        });
    }
    let draft = Draft {
        current_values: current,
        patch: PlanPatch {
            schema_version: 2,
            base_digest: plan.approval_digest().map_err(|e| e.to_string())?,
            items,
        },
    };
    serde_json::to_vec_pretty(&draft).map_err(|e| e.to_string())
}

/// Typed content value of a managed field, after any existing edit intent.
fn current_value(document: &linguist_core::LearningDocument, key: &str) -> String {
    use linguist_core::{FieldIntent, LearningContent};
    match document.edits.get(key) {
        Some(FieldIntent::Set(value)) => return value.clone(),
        Some(FieldIntent::Clear) => return String::new(),
        _ => {}
    }
    match (&document.content, key) {
        (LearningContent::Vocabulary(v), "Meaning") => v.meaning.clone(),
        (LearningContent::Vocabulary(v), "Reading") => v.reading.clone(),
        (LearningContent::Vocabulary(v), "Pronunciation") => v.pronunciation.clone(),
        (LearningContent::Vocabulary(v), "Usage") => v.usage.clone(),
        (LearningContent::Vocabulary(v), "Kanji") => v.kanji.clone(),
        (LearningContent::Grammar(g), "Meaning") => g.meaning.clone(),
        (LearningContent::Grammar(g), "Formation") => g.formation.clone(),
        (LearningContent::Grammar(g), "Usage") => g.usage.clone(),
        _ => String::new(),
    }
}

/// Parse an edited draft strictly and return only its patch.
pub fn parse(bytes: &[u8]) -> Result<PlanPatch, String> {
    let draft: Draft =
        serde_json::from_slice(bytes).map_err(|e| format!("PLAN_EDIT_DRAFT_INVALID: {e}"))?;
    Ok(draft.patch)
}

/// Split an editor command without a shell: whitespace separates words,
/// single quotes are literal, double quotes allow `\"` and `\\`.
pub fn split_command(text: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => current.push(c),
                        None => return Err("EDITOR_COMMAND_UNTERMINATED_QUOTE".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => current.push(c),
                            Some(c) => {
                                current.push('\\');
                                current.push(c);
                            }
                            None => return Err("EDITOR_COMMAND_UNTERMINATED_QUOTE".into()),
                        },
                        Some(c) => current.push(c),
                        None => return Err("EDITOR_COMMAND_UNTERMINATED_QUOTE".into()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                current.push(chars.next().ok_or("EDITOR_COMMAND_TRAILING_ESCAPE")?);
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                current.push(c);
            }
        }
    }
    if in_word {
        words.push(current);
    }
    Ok(words)
}

/// Resolve the editor argv: `editing.editor_argv`, else VISUAL, else EDITOR.
pub fn editor_argv(
    configured: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    if !configured.is_empty() {
        return Ok(configured.to_vec());
    }
    for name in ["VISUAL", "EDITOR"] {
        if let Some(value) = environment.get(name).filter(|v| !v.trim().is_empty()) {
            let argv = split_command(value)?;
            if !argv.is_empty() {
                return Ok(argv);
            }
        }
    }
    Err("EDITOR_UNAVAILABLE: set editing.editor_argv, VISUAL or EDITOR".into())
}

/// Run the editor on `path` with inherited terminal I/O and wait for it.
pub fn launch(argv: &[String], path: &Path) -> Result<(), String> {
    let (program, args) = argv.split_first().ok_or("EDITOR_UNAVAILABLE")?;
    let mut child = std::process::Command::new(program)
        .args(args)
        .arg(path)
        .spawn()
        .map_err(|_| "EDITOR_SPAWN_FAILED")?;
    // The editor owns the terminal: a Ctrl-C meant for it must not end this
    // process and orphan the editor. Restore the previous handlers afterwards.
    #[cfg(unix)]
    // SAFETY: SIG_IGN is always a valid disposition; the previous handlers are
    // restored below before returning.
    let previous = unsafe {
        (
            libc::signal(libc::SIGINT, libc::SIG_IGN),
            libc::signal(libc::SIGQUIT, libc::SIG_IGN),
        )
    };
    let status = child.wait();
    #[cfg(unix)]
    // SAFETY: restores the dispositions returned by signal() above.
    unsafe {
        libc::signal(libc::SIGINT, previous.0);
        libc::signal(libc::SIGQUIT, previous.1);
    }
    let status = status.map_err(|_| "EDITOR_SPAWN_FAILED")?;
    if !status.success() {
        return Err(
            "PLAN_EDIT_ABORTED: the editor exited unsuccessfully; the parent revision is unchanged"
                .into(),
        );
    }
    Ok(())
}

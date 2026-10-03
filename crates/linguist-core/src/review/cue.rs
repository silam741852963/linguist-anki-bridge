//! Typed cue repairs change content, never waive validation or remove tasks.
use crate::{
    Issue, LearningContent, LearningDocument, Severity, Task, canonical::ContractError,
    records::ReviewChoice,
};

fn field(
    document: &LearningDocument,
    issue: &Issue,
    choice: &ReviewChoice,
) -> Option<&'static str> {
    match (choice, &document.content) {
        (
            ReviewChoice::Cue {
                task: Task::Production,
                ..
            },
            LearningContent::Vocabulary(_),
        ) if document.requested_tasks.contains(&Task::Production)
            && matches!(issue.code.as_str(), "MISSING_CUE" | "ANSWER_LEAK")
            && issue.field.as_deref() == Some("production_prompt") =>
        {
            Some("production_prompt")
        }
        (
            ReviewChoice::Cue {
                task: Task::Spelling,
                ..
            },
            LearningContent::Vocabulary(_),
        ) if document.requested_tasks.contains(&Task::Spelling)
            && matches!(issue.code.as_str(), "MISSING_CUE" | "ANSWER_LEAK")
            && issue.field.as_deref() == Some("spelling_prompt") =>
        {
            Some("spelling_prompt")
        }
        (
            ReviewChoice::Cue {
                task: Task::Recognition,
                ..
            },
            LearningContent::Grammar(_),
        ) if document.requested_tasks.contains(&Task::Recognition)
            && matches!(issue.code.as_str(), "REQUIRED_CONTENT" | "ANSWER_LEAK")
            && issue.field.as_deref() == Some("recognition_prompt") =>
        {
            Some("recognition_prompt")
        }
        (ReviewChoice::Exercise { .. }, LearningContent::Grammar(_))
            if document.requested_tasks.contains(&Task::Application)
                && matches!(issue.code.as_str(), "MISSING_EXERCISE" | "ANSWER_LEAK")
                && issue.field.as_deref() == Some("exercise_prompt") =>
        {
            Some("exercise_prompt")
        }
        _ => None,
    }
}
pub(super) fn applicable(
    document: &LearningDocument,
    issue: &Issue,
    choice: &ReviewChoice,
) -> bool {
    issue.stage == "validation" && field(document, issue, choice).is_some()
}
pub(super) fn repair(
    document: &mut LearningDocument,
    issue: &Issue,
    choice: &ReviewChoice,
) -> Result<(), ContractError> {
    if !applicable(document, issue, choice) {
        return Err(ContractError("CUE_ISSUE_CONFLICT".into()));
    }
    let field = field(document, issue, choice).unwrap();
    match (choice, &mut document.content) {
        (
            ReviewChoice::Cue {
                task: Task::Production,
                text,
            },
            LearningContent::Vocabulary(vocab),
        ) => {
            vocab.production_prompt = text.clone();
            document.edits.remove("ProductionPrompt");
        }
        (
            ReviewChoice::Cue {
                task: Task::Spelling,
                text,
            },
            LearningContent::Vocabulary(vocab),
        ) => {
            vocab.spelling_prompt = text.clone();
            document.edits.remove("SpellingPrompt");
        }
        (
            ReviewChoice::Cue {
                task: Task::Recognition,
                text,
            },
            LearningContent::Grammar(grammar),
        ) => {
            grammar.recognition_prompt = text.clone();
            document.edits.remove("RecognitionPrompt");
        }
        (ReviewChoice::Exercise { prompt, answer }, LearningContent::Grammar(grammar)) => {
            grammar.exercise_prompt = prompt.clone();
            grammar.exercise_answer = answer.clone();
            document.edits.remove("ExercisePrompt");
            document.edits.remove("ExerciseAnswer");
        }
        _ => return Err(ContractError("CUE_ISSUE_CONFLICT".into())),
    }
    if crate::validation::validate(document)
        .iter()
        .any(|issue| issue.severity == Severity::Error && issue.field.as_deref() == Some(field))
    {
        return Err(ContractError("CUE_CONTENT_INVALID".into()));
    }
    Ok(())
}

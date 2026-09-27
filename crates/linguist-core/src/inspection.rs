//! Compare immutable plan evidence without generation or collection access.
use crate::{canonical::ContractError, records::PlanRevision};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Serialize)]
pub struct Change {
    /// RFC 6901 JSON pointer into the versioned revision.
    pub path: String,
    /// None means absent; Some(Null) means an explicit JSON null.
    pub before: Option<Value>,
    pub after: Option<Value>,
}
#[derive(Debug, Serialize)]
pub struct RevisionDiff {
    pub plan_id: uuid::Uuid,
    pub from_revision: u32,
    pub to_revision: u32,
    pub from_digest: String,
    pub to_digest: String,
    pub changes: Vec<Change>,
    pub card_consequences: Vec<CardConsequences>,
    pub live_checked: bool,
}
#[derive(Debug, Serialize)]
pub struct CardConsequences {
    pub document_id: uuid::Uuid,
    pub captured_card_ids: Vec<crate::AnkiId>,
    pub added_tasks: Vec<crate::Task>,
    pub removed_tasks: Vec<crate::Task>,
    pub document_removed: bool,
    /// Revision differences describe intent, not a verified native mutation receipt.
    pub native_history_verified: bool,
}
fn pointer_component(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
fn compare(path: &str, before: Option<&Value>, after: Option<&Value>, out: &mut Vec<Change>) {
    if before == after {
        return;
    }
    match (before, after) {
        (Some(Value::Object(left)), Some(Value::Object(right))) => {
            let keys: BTreeSet<_> = left.keys().chain(right.keys()).collect();
            for key in keys {
                compare(
                    &format!("{path}/{}", pointer_component(key)),
                    left.get(key),
                    right.get(key),
                    out,
                );
            }
        }
        // Arrays are atomic: insertion must not appear to change unrelated identities.
        _ => out.push(Change {
            path: path.into(),
            before: before.cloned(),
            after: after.cloned(),
        }),
    }
}
pub fn revision_diff(
    before: &PlanRevision,
    after: &PlanRevision,
) -> Result<RevisionDiff, ContractError> {
    if before.id != after.id {
        return Err(ContractError("PLAN_DIFF_ID_CONFLICT".into()));
    }
    let left = serde_json::to_value(before)?;
    let right = serde_json::to_value(after)?;
    let mut changes = Vec::new();
    compare("", Some(&left), Some(&right), &mut changes);
    let ids: BTreeSet<_> = before
        .documents
        .iter()
        .chain(&after.documents)
        .map(|doc| doc.id)
        .collect();
    let mut card_consequences = Vec::new();
    for id in ids {
        let old = before.documents.iter().find(|doc| doc.id == id);
        let new = after.documents.iter().find(|doc| doc.id == id);
        let old_tasks: BTreeSet<_> = old
            .into_iter()
            .flat_map(|doc| &doc.requested_tasks)
            .copied()
            .collect();
        let new_tasks: BTreeSet<_> = new
            .into_iter()
            .flat_map(|doc| &doc.requested_tasks)
            .copied()
            .collect();
        card_consequences.push(CardConsequences {
            document_id: id,
            captured_card_ids: old
                .into_iter()
                .flat_map(|doc| &doc.sources)
                .flat_map(|source| &source.cards)
                .map(|card| card.id.clone())
                .collect(),
            added_tasks: new_tasks.difference(&old_tasks).copied().collect(),
            removed_tasks: old_tasks.difference(&new_tasks).copied().collect(),
            document_removed: new.is_none(),
            native_history_verified: false,
        });
    }
    Ok(RevisionDiff {
        plan_id: before.id,
        from_revision: before.revision,
        to_revision: after.revision,
        from_digest: before.approval_digest()?,
        to_digest: after.approval_digest()?,
        changes,
        card_consequences,
        live_checked: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointers_escape_and_absent_is_distinct_from_null() {
        let before = serde_json::json!({"a/b~c":null,"array":[1,2]});
        let after = serde_json::json!({"new":null,"array":[0,1,2]});
        let mut changes = Vec::new();
        compare("", Some(&before), Some(&after), &mut changes);
        assert_eq!(changes[0].path, "/a~1b~0c");
        assert_eq!(changes[0].before, Some(Value::Null));
        assert_eq!(changes[0].after, None);
        assert_eq!(changes[1].path, "/array");
        assert_eq!(changes[2].before, None);
        assert_eq!(changes[2].after, Some(Value::Null));
    }
}

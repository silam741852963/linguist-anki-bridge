use linguist_core::{LearningDocument, records::*, render};
use linguist_store::*;
use std::{collections::BTreeMap, path::PathBuf};
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: std::env::temp_dir().join(format!("lab-store-test-{}", uuid::Uuid::new_v4())),
        }
    }
    fn open(&self) -> Store {
        Store::open(&self.root).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn plan() -> PlanRevision {
    let doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary.json"
    ))
    .unwrap();
    let rendered = render::render(&doc, &BTreeMap::new()).unwrap();
    PlanRevision {
        grammar_groups: vec![],
        schema_version: 2,
        id: uuid::Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            version: 2,
            values: BTreeMap::new(),
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "fixture".into(),
        },
        binding: None,
        source_digest: "fixture".into(),
        selection: None,
        documents: vec![doc],
        rendered: vec![rendered],
        review_decisions: vec![],
    }
}
#[test]
fn assets_survive_reopen_and_corruption_is_detected() {
    let f = Fixture::new();
    let mut store = f.open();
    let digest = store.publish_asset(b"original source bytes", 1024).unwrap();
    drop(store);
    let store = f.open();
    assert_eq!(
        store.asset(&digest, 1024).unwrap(),
        b"original source bytes"
    );
    std::fs::write(f.root.join("assets").join(&digest), "changed").unwrap();
    assert!(store.asset(&digest, 1024).is_err());
}
#[test]
fn immutable_revisions_require_exact_parent_and_survive_reopen() {
    let f = Fixture::new();
    let mut store = f.open();
    let mut plan = plan();
    let parent = store.publish_revision(&plan).unwrap();
    assert!(store.publish_revision(&plan).is_err());
    plan.revision = 2;
    plan.documents[0].personal_notes = "new revision".into();
    assert!(store.publish_revision(&plan).is_err());
    plan.parent_digest = Some(parent);
    plan.rendered = vec![render::render(&plan.documents[0], &BTreeMap::new()).unwrap()];
    store.publish_revision(&plan).unwrap();
    drop(store);
    let store = f.open();
    assert_eq!(store.revision(plan.id, 2).unwrap(), plan);
    assert_eq!(store.list_revisions(10).unwrap().len(), 2);
}
#[test]
fn missing_asset_prevents_revision_publication() {
    let f = Fixture::new();
    let mut store = f.open();
    let mut plan = plan();
    plan.documents[0].media.push(MediaAsset {
        digest: "a".repeat(64),
        filename: "source.png".into(),
        original_filename: None,
        size_bytes: 1,
        mime: "image/png".into(),
        owner: MediaOwner::Source,
        role: MediaRole::Archive,
        source_id: None,
        attribution: "fixture".into(),
        license: None,
    });
    assert!(store.publish_revision(&plan).is_err());
    assert!(store.list_revisions(10).unwrap().is_empty());
}
#[test]
fn future_schema_is_rejected_without_downgrade() {
    let f = Fixture::new();
    drop(f.open());
    let connection = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    connection.pragma_update(None, "user_version", 999).unwrap();
    drop(connection);
    assert!(Store::open(&f.root).is_err());
    let connection = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        999
    );
}
#[test]
fn sql_durability_settings_and_file_permissions_are_enforced() {
    let f = Fixture::new();
    let _store = f.open();
    let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(f.root.join("state.sqlite3"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&f.root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
#[test]
fn corrupt_existing_asset_is_not_adopted_as_valid() {
    let f = Fixture::new();
    let mut store = f.open();
    let digest = linguist_core::canonical::asset_digest(b"expected");
    std::fs::write(f.root.join("assets").join(&digest), "wrong bytes").unwrap();
    assert!(store.publish_asset(b"expected", 1024).is_err());
    assert!(store.asset(&digest, 1024).is_err());
}
#[test]
fn read_only_store_does_not_initialize_missing_state() {
    let f = Fixture::new();
    assert!(Store::read_only(&f.root).is_err());
    assert!(!f.root.exists());
    let mut store = f.open();
    let plan = plan();
    store.publish_revision(&plan).unwrap();
    drop(store);
    let mut store = Store::read_only(&f.root).unwrap();
    assert_eq!(store.latest_revision(plan.id).unwrap(), 1);
    assert_eq!(store.revision(plan.id, 1).unwrap(), plan);
    assert!(store.publish_asset(b"cannot publish", 1024).is_err());
}

#[test]
fn archive_only_assets_are_required_and_checked_after_reopen() {
    let f = Fixture::new();
    let mut store = f.open();
    let mut plan = plan();
    let bytes = b"byte-exact original input";
    let digest = linguist_core::canonical::asset_digest(bytes);
    plan.documents[0].archives.push(SourceArchive {
        id: uuid::Uuid::new_v4(),
        source_id: uuid::Uuid::new_v4(),
        digest: digest.clone(),
        original_fields: BTreeMap::new(),
        asset_digests: vec![digest.clone()],
    });
    assert!(store.publish_revision(&plan).is_err());
    assert!(store.list_revisions(10).unwrap().is_empty());
    assert_eq!(store.publish_asset(bytes, 1024).unwrap(), digest);
    store.publish_revision(&plan).unwrap();
    drop(store);
    let store = Store::read_only(&f.root).unwrap();
    assert_eq!(store.revision(plan.id, 1).unwrap(), plan);
    std::fs::write(f.root.join("assets").join(&digest), b"corrupt original").unwrap();
    assert_eq!(store.revision(plan.id, 1).unwrap_err(), "ASSET_CORRUPT");
    std::fs::remove_file(f.root.join("assets").join(&digest)).unwrap();
    assert_eq!(store.revision(plan.id, 1).unwrap_err(), "ASSET_READ_IO");
}

#[test]
fn revision_reads_reject_media_manifest_size_mismatch() {
    let f = Fixture::new();
    let mut store = f.open();
    let mut plan = plan();
    let digest = store.publish_asset(b"archive bytes", 1024).unwrap();
    plan.documents[0].media.push(MediaAsset {
        digest,
        filename: "original.bin".into(),
        original_filename: None,
        size_bytes: 13,
        mime: "application/octet-stream".into(),
        owner: MediaOwner::Source,
        role: MediaRole::Archive,
        source_id: None,
        attribution: "fixture".into(),
        license: None,
    });
    store.publish_revision(&plan).unwrap();
    // Simulate a self-consistent metadata rewrite: byte hashing alone cannot catch this.
    plan.documents[0].media[0].size_bytes = 14;
    let body = linguist_core::canonical::bytes(&plan).unwrap();
    let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    db.execute(
        "UPDATE revisions SET body=?1,body_digest=?2,digest=?3 WHERE id=?4",
        rusqlite::params![
            body,
            linguist_core::canonical::asset_digest(&body),
            plan.approval_digest().unwrap(),
            plan.id.to_string()
        ],
    )
    .unwrap();
    assert_eq!(
        store.revision(plan.id, 1).unwrap_err(),
        "ASSET_MANIFEST_SIZE_MISMATCH"
    );
}

#[test]
fn revision_diff_reports_task_and_field_changes_without_mutation() {
    let before = plan();
    let mut after = before.clone();
    after.revision = 2;
    after.parent_digest = Some(before.approval_digest().unwrap());
    after.documents[0].personal_notes = "authored annotation".into();
    after.documents[0]
        .requested_tasks
        .push(linguist_core::Task::Production);
    let result = linguist_core::inspection::revision_diff(&before, &after).unwrap();
    assert_eq!(
        result.card_consequences[0].added_tasks,
        vec![linguist_core::Task::Production]
    );
    assert!(result.card_consequences[0].removed_tasks.is_empty());
    assert!(!result.live_checked);
    assert!(!result.card_consequences[0].native_history_verified);
    assert!(
        result
            .changes
            .iter()
            .any(|change| change.path == "/documents")
    );
    assert_eq!(before.revision, 1);
    after.documents.clear();
    let result = linguist_core::inspection::revision_diff(&before, &after).unwrap();
    assert!(result.card_consequences[0].document_removed);
    assert_eq!(
        result.card_consequences[0].removed_tasks,
        vec![linguist_core::Task::Comprehension]
    );
    after.id = uuid::Uuid::new_v4();
    assert!(linguist_core::inspection::revision_diff(&before, &after).is_err());
}

#[test]
fn typed_plan_edits_preserve_parent_and_invalidate_content_reviews() {
    use linguist_core::{FieldIntent, editing::*};
    let f = Fixture::new();
    let mut store = f.open();
    let mut base = plan();
    let review_id = uuid::Uuid::new_v4();
    let input_digest = base.documents[0].semantic_digest().unwrap();
    base.documents[0].reviews.push(ReviewDecision {
        id: review_id,
        issue_id: "fixture".into(),
        input_digest,
        actor: "fixture".into(),
        created_at: "fixture".into(),
        choice: ReviewChoice::Media("fixture".into()),
    });
    let digest = store.publish_revision(&base).unwrap();
    let patch = PlanPatch {
        schema_version: 2,
        base_digest: digest.clone(),
        items: vec![ItemPatch {
            document_id: base.documents[0].id,
            fields: BTreeMap::new(),
            personal_notes: FieldIntent::Set("my association".into()),
        }],
    };
    let edited = apply_patch(&base, &patch, false).unwrap();
    assert!(edited.changed && edited.ready);
    assert_eq!(edited.invalidated_review_ids, vec![review_id]);
    assert!(edited.revision.documents[0].reviews.is_empty());
    assert_eq!(edited.revision.revision, 2);
    assert_eq!(edited.revision.parent_digest, Some(digest));
    assert_eq!(
        edited.revision.documents[0].personal_notes,
        "my association"
    );
    store.publish_revision(&edited.revision).unwrap();
    assert_eq!(store.revision(base.id, 1).unwrap(), base);
    assert_eq!(store.revision(base.id, 2).unwrap(), edited.revision);
    let stale = apply_patch(&edited.revision, &patch, false).unwrap_err();
    assert!(stale.to_string().contains("CONFLICT"));
    let empty_patch = PlanPatch {
        schema_version: 2,
        base_digest: base.approval_digest().unwrap(),
        items: vec![],
    };
    let noop = apply_patch(&base, &empty_patch, false).unwrap();
    assert!(!noop.changed);
    assert!(noop.ready);
    assert_eq!(noop.revision, base);
}

#[test]
fn invalid_edits_require_explicit_draft_and_typed_controls_cannot_be_patched() {
    use linguist_core::{FieldIntent, editing::*};
    let base = plan();
    let mut patch = PlanPatch {
        schema_version: 2,
        base_digest: base.approval_digest().unwrap(),
        items: vec![ItemPatch {
            document_id: base.documents[0].id,
            fields: BTreeMap::from([("Meaning".into(), FieldIntent::Clear)]),
            personal_notes: FieldIntent::Keep,
        }],
    };
    assert!(apply_patch(&base, &patch, false).is_err());
    let draft = apply_patch(&base, &patch, true).unwrap();
    assert!(!draft.ready);
    assert!(draft.revision.rendered.is_empty());
    assert!(
        draft.revision.documents[0]
            .issues
            .iter()
            .any(|issue| issue.code == "EFFECTIVE_RENDER_BLOCKED")
    );
    let f = Fixture::new();
    let mut store = f.open();
    store.publish_revision(&base).unwrap();
    store.publish_revision(&draft.revision).unwrap();
    assert_eq!(store.revision(base.id, 2).unwrap(), draft.revision);
    let repair = PlanPatch {
        schema_version: 2,
        base_digest: draft.revision.approval_digest().unwrap(),
        items: vec![ItemPatch {
            document_id: base.documents[0].id,
            fields: BTreeMap::from([(
                "Meaning".into(),
                FieldIntent::Set("restored meaning".into()),
            )]),
            personal_notes: FieldIntent::Keep,
        }],
    };
    let repaired = apply_patch(&draft.revision, &repair, false).unwrap();
    assert!(repaired.ready);
    assert!(
        repaired.revision.documents[0]
            .issues
            .iter()
            .all(|issue| issue.code != "EFFECTIVE_RENDER_BLOCKED")
    );
    store.publish_revision(&repaired.revision).unwrap();
    patch.items[0].fields =
        BTreeMap::from([("EnableSpelling".into(), FieldIntent::Set("1".into()))]);
    assert!(
        apply_patch(&base, &patch, true)
            .unwrap_err()
            .to_string()
            .contains("TYPED_FIELD_REQUIRED")
    );
    patch.items[0].fields.clear();
    patch.items.push(ItemPatch {
        document_id: base.documents[0].id,
        fields: BTreeMap::new(),
        personal_notes: FieldIntent::Keep,
    });
    assert!(apply_patch(&base, &patch, true).is_err());
}

#[test]
fn validation_evidence_is_durable_append_only_and_revision_bound() {
    let f = Fixture::new();
    let mut store = f.open();
    let original = plan();
    let digest = store.publish_revision(&original).unwrap();
    let receipt = store.validate_revision(original.id, 1).unwrap();
    assert!(receipt.evidence.content_ready);
    assert!(!receipt.evidence.apply_eligible && !receipt.evidence.live_checked);
    assert_eq!(receipt.evidence.plan_digest, digest);
    assert_eq!(store.revision(original.id, 1).unwrap(), original);
    let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE validations SET plan_digest='changed'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM validations", []).is_err());
    drop(db);
    drop(store);
    let mut store = Store::read_only(&f.root).unwrap();
    let restored = store.validation_evidence(receipt.evidence.id).unwrap();
    assert_eq!(restored.evidence, receipt.evidence);
    assert_eq!(restored.evidence_digest, receipt.evidence_digest);
    assert!(store.validate_revision(original.id, 1).is_err());
    assert!(store.validation_evidence(uuid::Uuid::new_v4()).is_err());
}

#[test]
fn validation_reports_missing_staged_render_without_rewriting_plan() {
    let f = Fixture::new();
    let mut store = f.open();
    let mut original = plan();
    original.rendered.clear();
    store.publish_revision(&original).unwrap();
    let receipt = store.validate_revision(original.id, 1).unwrap();
    assert!(!receipt.evidence.content_ready);
    assert!(
        receipt.evidence.items[0]
            .issues
            .iter()
            .any(|issue| issue.code == "STAGED_RENDER_MISMATCH")
    );
    assert_eq!(store.revision(original.id, 1).unwrap(), original);
}

#[test]
fn schema_three_upgrade_preserves_revisions_and_leases_and_backs_up_first() {
    let f = Fixture::new();
    let mut store = f.open();
    let original = plan();
    store.publish_revision(&original).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE preparation_controls;DROP TABLE preparation_event_assets;DROP TABLE preparation_events;DROP TABLE preparation_jobs;DROP TABLE approvals;DROP TABLE validations; PRAGMA user_version=3;")
        .unwrap();
    drop(db);
    let mut store = f.open();
    assert_eq!(store.revision(original.id, 1).unwrap(), original);
    assert!(
        store
            .validate_revision(original.id, 1)
            .unwrap()
            .evidence
            .content_ready
    );
    let backup = std::fs::read_dir(&f.root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".schema-v3-")
        })
        .unwrap();
    let db = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.pragma_query_value(None, "integrity_check", |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM revisions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(db.prepare("SELECT * FROM leases").is_ok());
}

#[test]
fn content_approval_is_immutable_scoped_and_does_not_authorize_apply() {
    use linguist_core::approval::ApprovalRequest;
    let f = Fixture::new();
    let mut store = f.open();
    let mut original = plan();
    let digest = store.publish_revision(&original).unwrap();
    let mut request = ApprovalRequest {
        plan_id: original.id,
        revision: 1,
        digest: digest.clone(),
        item_ids: None,
        actor: "reviewer".into(),
        accepted_warnings: vec![],
    };
    let receipt = store.approve_revision(&request).unwrap().unwrap();
    assert!(!receipt.apply_authorized);
    assert_eq!(receipt.approval.item_ids, vec![original.documents[0].id]);
    assert!(receipt.approval.approved_at.starts_with("unix-seconds:"));
    assert_eq!(
        store.approval(receipt.id).unwrap().approval,
        receipt.approval
    );
    let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE approvals SET plan_digest='forged'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM approvals", []).is_err());
    drop(db);
    request.digest = "wrong".into();
    assert!(
        store
            .approve_revision(&request)
            .unwrap_err()
            .contains("CONFLICT")
    );
    request.digest = digest.clone();
    request.item_ids = Some(vec![uuid::Uuid::new_v4()]);
    assert!(store.approve_revision(&request).is_err());
    request.item_ids = Some(vec![]);
    assert!(store.approve_revision(&request).unwrap().is_none());
    request.item_ids = None;
    original.revision = 2;
    original.parent_digest = Some(digest);
    store.publish_revision(&original).unwrap();
    assert!(
        store
            .approve_revision(&request)
            .unwrap_err()
            .contains("CONFLICT")
    );
    drop(store);
    let store = Store::read_only(&f.root).unwrap();
    // Historical approval stays inspectable, while the latest revision has changed.
    assert_eq!(
        store.approval(receipt.id).unwrap().approval,
        receipt.approval
    );
}

#[test]
fn approval_requires_exact_warning_acceptance_and_cannot_waive_errors() {
    use linguist_core::{Issue, Severity, approval::ApprovalRequest};
    let f = Fixture::new();
    let mut store = f.open();
    let mut original = plan();
    let mut issue = Issue::new(
        "ATTRIBUTION_REVIEWED",
        Severity::Warning,
        None,
        "Confirm attribution.",
    );
    issue.stage = "capture".into();
    original.documents[0].issues.push(issue);
    let digest = store.publish_revision(&original).unwrap();
    let mut request = ApprovalRequest {
        plan_id: original.id,
        revision: 1,
        digest,
        item_ids: None,
        actor: "reviewer".into(),
        accepted_warnings: vec![],
    };
    assert!(
        store
            .approve_revision(&request)
            .unwrap_err()
            .contains("DOCUMENT_NOT_READY")
    );
    request.accepted_warnings = vec!["unknown".into()];
    assert!(
        store
            .approve_revision(&request)
            .unwrap_err()
            .contains("UNKNOWN_WARNING")
    );
    request.accepted_warnings = vec!["ATTRIBUTION_REVIEWED".into()];
    assert!(store.approve_revision(&request).unwrap().is_some());
    original.revision = 2;
    original.parent_digest = Some(request.digest.clone());
    original.rendered.clear();
    original.documents[0].issues[0].severity = Severity::Error;
    request.revision = 2;
    request.digest = store.publish_revision(&original).unwrap();
    assert!(
        store
            .approve_revision(&request)
            .unwrap_err()
            .contains("DOCUMENT_NOT_READY")
    );
}

#[test]
fn schema_four_upgrade_retains_validation_receipts() {
    let f = Fixture::new();
    let mut store = f.open();
    let original = plan();
    store.publish_revision(&original).unwrap();
    let evidence = store.validate_revision(original.id, 1).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(f.root.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE preparation_controls;DROP TABLE preparation_event_assets;DROP TABLE preparation_events;DROP TABLE preparation_jobs;DROP TABLE approvals; PRAGMA user_version=4;")
        .unwrap();
    drop(db);
    let store = f.open();
    assert_eq!(
        store
            .validation_evidence(evidence.evidence.id)
            .unwrap()
            .evidence,
        evidence.evidence
    );
    assert_eq!(store.revision(original.id, 1).unwrap(), original);
}

#[test]
fn resolving_generated_claim_creates_ready_child_and_preserves_parent() {
    use linguist_core::{Provenance, review::*};
    let f = Fixture::new();
    let mut store = f.open();
    let mut base = plan();
    let evidence_id = uuid::Uuid::new_v4();
    let language = base.documents[0].explanation_language.clone();
    base.documents[0].evidence.push(Evidence {
        id: evidence_id,
        field: "meaning".into(),
        provenance: Provenance::Generated,
        source_id: None,
        region_id: None,
        language,
        claim: "eat means consume food".into(),
        source_url: None,
        ambiguous: false,
    });
    base.documents[0].issues = linguist_core::validation::validate(&base.documents[0]);
    base.rendered.clear();
    let parent = store.publish_revision(&base).unwrap();
    let mut request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: parent,
        document_id: base.documents[0].id,
        issue_id: format!("GENERATED_FACT_REVIEW:{evidence_id}"),
        input_digest: base.documents[0].semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::ContentVerified {
            evidence_ids: vec![evidence_id],
        },
    };
    request.choice = ReviewChoice::ContentVerified {
        evidence_ids: vec![uuid::Uuid::new_v4()],
    };
    assert!(
        resolve(&base, &request, "unix-seconds:1".into())
            .unwrap_err()
            .to_string()
            .contains("EVIDENCE_MISMATCH")
    );
    request.choice = ReviewChoice::ContentVerified {
        evidence_ids: vec![evidence_id, evidence_id],
    };
    assert!(resolve(&base, &request, "unix-seconds:1".into()).is_err());
    request.choice = ReviewChoice::ContentVerified {
        evidence_ids: vec![evidence_id],
    };
    let result = resolve(&base, &request, "unix-seconds:1".into()).unwrap();
    assert!(result.ready);
    assert_eq!(
        result.revision.documents[0].semantic_digest().unwrap(),
        request.input_digest
    );
    assert_eq!(
        result.revision.documents[0].reviews[0].id,
        result.decision_id
    );
    assert_eq!(result.revision.review_decisions[0].id, result.decision_id);
    assert_eq!(result.revision.rendered.len(), 1);
    assert!(result.revision.documents[0].issues.is_empty());
    store.publish_revision(&result.revision).unwrap();
    assert_eq!(store.revision(base.id, 1).unwrap(), base);
    assert!(
        store
            .validate_revision(base.id, 2)
            .unwrap()
            .evidence
            .content_ready
    );
    assert!(
        resolve(&result.revision, &request, "unix-seconds:2".into())
            .unwrap_err()
            .to_string()
            .contains("CONFLICT")
    );
}

#[test]
fn review_rejects_stale_content_wrong_evidence_and_error_waivers() {
    use linguist_core::{Severity, review::*};
    let mut base = plan();
    let mut issue = linguist_core::Issue::new(
        "SOURCE_FIELDS_MISSING",
        Severity::Error,
        None,
        "Capture the original fields.",
    );
    issue.stage = "capture".into();
    base.documents[0].issues.push(issue.clone());
    base.rendered.clear();
    let mut request = ResolutionRequest {
        schema_version: 2,
        base_revision: 1,
        base_digest: base.approval_digest().unwrap(),
        document_id: base.documents[0].id,
        issue_id: issue.id,
        input_digest: base.documents[0].semantic_digest().unwrap(),
        actor: "reviewer".into(),
        choice: ReviewChoice::ContentVerified {
            evidence_ids: vec![],
        },
    };
    assert!(
        resolve(&base, &request, "unix-seconds:1".into())
            .unwrap_err()
            .to_string()
            .contains("CANNOT_BE_WAIVED")
    );
    request.input_digest = "stale".into();
    assert!(
        resolve(&base, &request, "unix-seconds:1".into())
            .unwrap_err()
            .to_string()
            .contains("INPUT_CONFLICT")
    );
    request.input_digest = base.documents[0].semantic_digest().unwrap();
    request.actor = "\n".into();
    assert!(
        resolve(&base, &request, "unix-seconds:1".into())
            .unwrap_err()
            .to_string()
            .contains("ACTOR")
    );
}

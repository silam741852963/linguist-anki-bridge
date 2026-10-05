//! `--apply` paths over the verified `lab-native-v1` companion (WP-03).
//!
//! Every path fails closed before any lease, journal or Anki request unless
//! `Client::native_verified` accepts the companion: a pinned verified build,
//! all seven actions and variants, an API key on both sides, a loopback
//! endpoint and a live collection session. Each write then holds the local
//! collection-writer lease, creates and verifies a full-collection checkpoint
//! through native `export_checkpoint`, and runs the existing journaled
//! orchestration (apply, split, restore, rollback, reconcile, model install).
use linguist_application::{
    apply::{self, ApplyRequest, ReconcileRequest},
    backup::{self, CheckpointRequest, ScopePreference},
    checkpoint::CoverageRequirement,
    model_install,
    native_port::{NativeDeadlines, NativePort},
    restore::{self, RestoreDecision, RestoreRequest},
    split::{self, SplitApplyRequest},
};
use linguist_core::records::OperationState;
use linguist_store::{
    Store,
    lease::{LeaseToken, Resource},
};
use serde_json::json;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

use crate::{emit, now_ms, package_limits, state_root};

/// Writer lease for one invocation; released on every exit path by the caller.
const LEASE_SECONDS: u64 = 1800;

fn setting_u64(settings: &linguist_config::Effective, key: &str) -> u64 {
    settings.values[key].as_u64().unwrap_or(0)
}

fn deadlines(settings: &linguist_config::Effective) -> NativeDeadlines {
    let request = setting_u64(settings, "anki.request_timeout_seconds").max(1);
    let verify = setting_u64(settings, "backup.verify_timeout_seconds").max(request);
    let poll = settings.values["anki.commit_interval_seconds"]
        .as_f64()
        .unwrap_or(0.25)
        .clamp(0.05, 5.0);
    NativeDeadlines {
        operation: Duration::from_secs(request),
        export: Duration::from_secs(verify),
        poll: Duration::from_secs_f64(poll),
    }
}

fn max_asset_bytes(settings: &linguist_config::Effective) -> u64 {
    setting_u64(settings, "media.max_asset_mb").max(1) * 1024 * 1024
}

/// Client plus verified port; fails closed with `CAPABILITY_UNAVAILABLE`.
pub(crate) fn connect<'a>(
    client: &'a linguist_anki::Client,
    settings: &linguist_config::Effective,
) -> Result<NativePort<'a>, String> {
    apply::require_native_adapter(&settings.values)?;
    NativePort::connect(
        client,
        &state_root(settings)?,
        deadlines(settings),
        max_asset_bytes(settings),
    )
}

struct Writer<'a> {
    store: Store,
    lease: LeaseToken,
    port: NativePort<'a>,
}

impl<'a> Writer<'a> {
    fn open(port: NativePort<'a>, settings: &linguist_config::Effective) -> Result<Self, String> {
        let mut store = Store::open(&state_root(settings)?)?;
        let lineage = port.binding().lineage_id;
        let lease = store.acquire_lease(&Resource::CollectionWriter(lineage), LEASE_SECONDS)?;
        Ok(Self { store, lease, port })
    }

    /// Full-collection checkpoint through native export, verified by size,
    /// hash, package structure, every scope entry and a decode-restore test.
    fn checkpoint(
        &mut self,
        settings: &linguist_config::Effective,
        note_ids: &[i64],
        model_ids: &[i64],
        group: Option<Uuid>,
    ) -> Result<(Uuid, String), String> {
        self.checkpoint_to(settings, note_ids, model_ids, group, None)
    }

    fn checkpoint_to(
        &mut self,
        settings: &linguist_config::Effective,
        note_ids: &[i64],
        model_ids: &[i64],
        group: Option<Uuid>,
        output: Option<PathBuf>,
    ) -> Result<(Uuid, String), String> {
        let binding = self.port.refresh()?;
        let scope = self.port.scope(
            note_ids,
            model_ids,
            CoverageRequirement {
                scheduling: true,
                media: true,
                schema: true,
            },
        )?;
        let protected = scope.digest()?;
        let limits = package_limits(settings)?;
        let stamp = now_ms()?;
        let output = match output {
            Some(output) => output,
            None => {
                let suffix = Uuid::new_v4().simple().to_string();
                backup_dir(settings)?.join(format!("checkpoint-{stamp}-{}.colpkg", &suffix[..12]))
            }
        };
        let restore_target = private_scratch(&limits.scratch_dir)?;
        let outcome = backup::create_checkpoint(
            &mut self.store,
            &self.lease,
            &mut self.port,
            CheckpointRequest {
                binding,
                scope,
                preference: ScopePreference::Collection,
                output,
                group_id: group,
                protected_manifest_digest: protected.clone(),
                restore_target: restore_target.clone(),
                limits,
                now_ms: stamp,
            },
        );
        let _ = std::fs::remove_dir_all(&restore_target);
        let outcome = outcome.map_err(|failure| {
            format!(
                "{}{}",
                failure.code,
                failure
                    .journal_id
                    .map(|id| format!(" (checkpoint journal {id})"))
                    .unwrap_or_default()
            )
        })?;
        Ok((outcome.record.receipt.id, protected))
    }

    fn finish(mut self) {
        let _ = self.store.release_lease(&self.lease);
    }
}

fn backup_dir(settings: &linguist_config::Effective) -> Result<PathBuf, String> {
    let directory = linguist_config::expand_path(
        settings.values["storage.backup_dir"]
            .as_str()
            .ok_or("INVALID_BACKUP_DIR")?,
        &std::env::vars().collect(),
    )?;
    if !directory.is_absolute() {
        return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
    }
    private_dir(&directory)?;
    Ok(directory)
}

fn private_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!(
            "CHECKPOINT_OUTPUT_PARENT_INVALID: {}",
            path.display()
        )),
        Err(_) => std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|_| format!("CHECKPOINT_OUTPUT_PARENT_UNAVAILABLE: {}", path.display())),
    }
}

fn private_scratch(parent: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let path = parent.join(format!("lab-restore-test-{}", Uuid::new_v4().simple()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(|_| "CHECKPOINT_SCRATCH_UNAVAILABLE".to_owned())?;
    Ok(path)
}

fn source_note_ids(
    store: &Store,
    plan: Uuid,
    revision: u32,
    items: &[Uuid],
) -> Result<Vec<i64>, String> {
    let revision = store.revision(plan, revision)?;
    let mut notes = BTreeSet::new();
    for document in revision.documents.iter().filter(|d| items.contains(&d.id)) {
        if let Some((_, note)) = apply::source_note(document)? {
            notes.insert(note);
        }
    }
    Ok(notes.into_iter().collect())
}

fn approval_for(
    store: &Store,
    plan: Uuid,
    revision: u32,
    digest: &str,
    items: &[Uuid],
) -> Result<Uuid, String> {
    store
        .approvals_for(plan, revision)?
        .into_iter()
        .rev()
        .find(|receipt| {
            receipt.approval.digest == digest
                && items
                    .iter()
                    .all(|item| receipt.approval.item_ids.contains(item))
        })
        .map(|receipt| receipt.id)
        .ok_or_else(|| {
            "APPLY_APPROVAL_MISSING: approve this exact revision for every selected item first"
                .into()
        })
}

fn reuse_seconds(settings: &linguist_config::Effective) -> u64 {
    setting_u64(settings, "backup.reuse_max_age_seconds")
}

fn max_package(settings: &linguist_config::Effective) -> u64 {
    setting_u64(settings, "backup.max_package_gb") * 1024 * 1024 * 1024
}

/// OP-34 `apply PLAN --apply`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_plan(
    settings: &linguist_config::Effective,
    plan: Uuid,
    revision: u32,
    digest: &str,
    item_ids: &[Uuid],
    split_group: Option<Uuid>,
    accept_schema_change: bool,
) -> Result<u8, String> {
    let client = crate::anki_client(settings)?;
    let port = connect(&client, settings)?;
    let mut writer = Writer::open(port, settings)?;
    let result = (|| {
        let stored = writer.store.revision(plan, revision)?;
        if let Some(group) = split_group {
            let units = stored
                .grammar_groups
                .iter()
                .find(|g| g.id == group)
                .ok_or("SPLIT_GROUP_NOT_FOUND")?
                .units
                .clone();
            let approval = approval_for(&writer.store, plan, revision, digest, &units)?;
            let execution = match writer.store.split_execution_for(plan, revision, group)? {
                Some(record) => record.execution_id,
                None => Uuid::new_v4(),
            };
            let notes = source_note_ids(&writer.store, plan, revision, &units)?;
            let (checkpoint, protected) =
                writer.checkpoint(settings, &notes, &[], Some(execution))?;
            let outcome = split::apply_group(
                &mut writer.store,
                &writer.lease,
                &mut writer.port,
                &SplitApplyRequest {
                    apply: true,
                    plan_id: plan,
                    revision,
                    digest,
                    grammar_group: group,
                    approval_id: approval,
                    checkpoint_id: checkpoint,
                    protected_manifest_digest: &protected,
                    reuse_max_age_seconds: reuse_seconds(settings),
                    max_package_bytes: max_package(settings),
                    max_media_bytes: max_asset_bytes(settings),
                    accept_schema_change,
                    now_ms: now_ms()?,
                    new_execution_id: Some(execution),
                },
            )?;
            let complete = outcome.state == "complete";
            emit(&json!({
                "schema_version": 2, "mode": "apply", "plan_id": plan, "revision": revision,
                "digest": digest, "checkpoint_id": checkpoint, "split": outcome,
                "collection_writes_enabled": true,
            }))?;
            return Ok(if complete { 0 } else { 4 });
        }
        let grouped: BTreeSet<Uuid> = stored
            .grammar_groups
            .iter()
            .flat_map(|g| g.units.iter().copied())
            .collect();
        let items: Vec<Uuid> = if item_ids.is_empty() {
            stored
                .documents
                .iter()
                .map(|d| d.id)
                .filter(|id| !grouped.contains(id))
                .collect()
        } else {
            item_ids.to_vec()
        };
        if items.is_empty() {
            return Err(
                "APPLY_NOTHING_SELECTED: use --split-group for reviewed grammar split units".into(),
            );
        }
        let approval = approval_for(&writer.store, plan, revision, digest, &items)?;
        let group = Uuid::new_v4();
        let notes = source_note_ids(&writer.store, plan, revision, &items)?;
        let (checkpoint, protected) = writer.checkpoint(settings, &notes, &[], Some(group))?;
        let mut outcomes = Vec::new();
        let mut committed = true;
        for item in &items {
            let outcome = apply::apply_item(
                &mut writer.store,
                &writer.lease,
                &mut writer.port,
                &ApplyRequest {
                    apply: true,
                    plan_id: plan,
                    revision,
                    digest,
                    item_id: *item,
                    approval_id: approval,
                    checkpoint_id: checkpoint,
                    group_id: Some(group),
                    protected_manifest_digest: &protected,
                    reuse_max_age_seconds: reuse_seconds(settings),
                    max_package_bytes: max_package(settings),
                    max_media_bytes: max_asset_bytes(settings),
                    accept_schema_change,
                    now_ms: now_ms()?,
                },
            )?;
            committed &= outcome.state == OperationState::Committed;
            let stop = outcome.state != OperationState::Committed;
            outcomes.push(outcome);
            if stop {
                break;
            }
        }
        emit(&json!({
            "schema_version": 2, "mode": "apply", "plan_id": plan, "revision": revision,
            "digest": digest, "group_id": group, "checkpoint_id": checkpoint,
            "items": outcomes, "rollback_command": format!("linguist-anki-bridge jobs rollback {group}"),
            "collection_writes_enabled": true,
        }))?;
        Ok(if committed { 0 } else { 4 })
    })();
    writer.finish();
    result
}

/// Live ALG-RESTORE preview through the companion; writes nothing.
pub(crate) fn restore_preview(
    settings: &linguist_config::Effective,
    snapshot: Uuid,
) -> Result<restore::RestorePlan, String> {
    let client = crate::anki_client(settings)?;
    let mut port = connect(&client, settings)?;
    let store = Store::read_only(&state_root(settings)?)?;
    restore::plan_restore(&store, &mut port, snapshot, None)
}

fn restore_scope(plan: &restore::RestorePlan) -> (Vec<i64>, Vec<i64>) {
    let notes = plan.note_id.into_iter().collect();
    let models = plan
        .model
        .as_ref()
        .map(|m| {
            let mut ids = vec![m.original_model_id, m.current_model_id];
            ids.sort();
            ids.dedup();
            ids
        })
        .unwrap_or_default();
    (notes, models)
}

/// OP-43 `snapshots restore SNAPSHOT --decision FILE --apply`.
pub(crate) fn restore_snapshot(
    settings: &linguist_config::Effective,
    snapshot: Uuid,
    decision: Option<RestoreDecision>,
) -> Result<u8, String> {
    let client = crate::anki_client(settings)?;
    let port = connect(&client, settings)?;
    let mut writer = Writer::open(port, settings)?;
    let result = (|| {
        let preview = restore::plan_restore(&writer.store, &mut writer.port, snapshot, None)?;
        if preview.resume_operation.is_none() && decision.is_none() {
            return Err("RESTORE_DECISION_REQUIRED: preview with `snapshots restore SNAPSHOT`, then pass --decision bound to the observed state".into());
        }
        let (notes, models) = restore_scope(&preview);
        let group = Uuid::new_v4();
        let (checkpoint, protected) = writer.checkpoint(settings, &notes, &models, Some(group))?;
        let outcome = restore::restore(
            &mut writer.store,
            &writer.lease,
            &mut writer.port,
            &RestoreRequest {
                apply: true,
                snapshot_id: snapshot,
                decision: decision.clone(),
                checkpoint_id: checkpoint,
                group_id: Some(group),
                protected_manifest_digest: &protected,
                reuse_max_age_seconds: reuse_seconds(settings),
                max_package_bytes: max_package(settings),
                max_media_bytes: max_asset_bytes(settings),
                now_ms: now_ms()?,
            },
        )?;
        let restored = outcome.state == Some(OperationState::Committed);
        emit(&json!({
            "schema_version": 2, "mode": "restore", "checkpoint_id": checkpoint,
            "restore": outcome, "collection_writes_enabled": true,
        }))?;
        Ok(if restored { 0 } else { 4 })
    })();
    writer.finish();
    result
}

/// OP-44 `jobs rollback GROUP --apply`: restore every apply operation of the
/// group with an observed-state-bound decision built from its live preview.
/// Any conflict stops before the first write and names the snapshot to
/// restore explicitly with a decision file.
pub(crate) fn rollback(
    settings: &linguist_config::Effective,
    group: Uuid,
    item_ids: &[Uuid],
    accept_schema_change: bool,
) -> Result<u8, String> {
    let client = crate::anki_client(settings)?;
    let port = connect(&client, settings)?;
    let mut writer = Writer::open(port, settings)?;
    let result = (|| {
        let order = restore::plan_group_rollback(&writer.store, &mut writer.port, group)?;
        let mut requests = Vec::new();
        let (mut notes, mut models) = (BTreeSet::new(), BTreeSet::new());
        let mut decisions = Vec::new();
        for item in &order {
            let record = writer.store.apply_operation(item.operation_id)?;
            if !item_ids.is_empty() && !item_ids.contains(&record.item_id) {
                continue;
            }
            let plan = match (&item.plan, &item.error) {
                (Some(plan), None) => plan,
                (_, error) => {
                    return Err(format!(
                        "ROLLBACK_PREVIEW_FAILED: snapshot {}: {}",
                        item.snapshot_id,
                        error.clone().unwrap_or_default()
                    ));
                }
            };
            if !plan.conflicts.is_empty() || !plan.blockers.is_empty() {
                return Err(format!(
                    "ROLLBACK_DECISION_REQUIRED: snapshot {} has {:?} {:?}; restore it with `snapshots restore {} --decision FILE --apply`",
                    item.snapshot_id, plan.conflicts, plan.blockers, item.snapshot_id
                ));
            }
            let (n, m) = restore_scope(plan);
            notes.extend(n);
            models.extend(m);
            decisions.push(RestoreDecision {
                schema_version: 1,
                snapshot_id: item.snapshot_id,
                observed_state_digest: plan.observed_state_digest.clone(),
                actor: "jobs rollback".into(),
                fields: Default::default(),
                decks: Default::default(),
                remove_unstudied_cards: vec![],
                delete_created_notes: vec![],
                accept_missing_media: vec![],
                accept_schema_change: accept_schema_change && plan.model.is_some(),
            });
        }
        if decisions.is_empty() {
            return Err("ROLLBACK_GROUP_EMPTY: no matching apply operation in this group".into());
        }
        let notes: Vec<i64> = notes.into_iter().collect();
        let models: Vec<i64> = models.into_iter().collect();
        let checkpoint_group = Uuid::new_v4();
        let (checkpoint, protected) =
            writer.checkpoint(settings, &notes, &models, Some(checkpoint_group))?;
        let now = now_ms()?;
        for decision in &decisions {
            requests.push(RestoreRequest {
                apply: true,
                snapshot_id: decision.snapshot_id,
                decision: Some(decision.clone()),
                checkpoint_id: checkpoint,
                group_id: Some(checkpoint_group),
                protected_manifest_digest: &protected,
                reuse_max_age_seconds: reuse_seconds(settings),
                max_package_bytes: max_package(settings),
                max_media_bytes: max_asset_bytes(settings),
                now_ms: now,
            });
        }
        let outcomes = restore::rollback_group(
            &mut writer.store,
            &writer.lease,
            &mut writer.port,
            group,
            &requests,
        )?;
        let mut all = true;
        let items: Vec<_> = outcomes
            .into_iter()
            .map(|outcome| match outcome {
                Ok(outcome) => {
                    all &= outcome.state == Some(OperationState::Committed);
                    json!({"restore": outcome})
                }
                Err((snapshot, error)) => {
                    all = false;
                    json!({"snapshot_id": snapshot, "error": error})
                }
            })
            .collect();
        emit(&json!({
            "schema_version": 2, "mode": "rollback", "group_id": group,
            "checkpoint_id": checkpoint, "items": items, "collection_writes_enabled": true,
        }))?;
        Ok(if all { 0 } else { 4 })
    })();
    writer.finish();
    result
}

/// OP-60 `recover reconcile OPERATION --apply [--rebind]`.
pub(crate) fn reconcile(
    settings: &linguist_config::Effective,
    operation: Uuid,
    rebind: bool,
) -> Result<u8, String> {
    let client = crate::anki_client(settings)?;
    let port = connect(&client, settings)?;
    let mut writer = Writer::open(port, settings)?;
    let result = (|| {
        if writer.store.model_operation(operation).is_ok() {
            let outcome = model_install::reconcile(
                &mut writer.store,
                &writer.lease,
                &mut writer.port,
                operation,
            )?;
            let verified = outcome.verified;
            emit(&json!({"schema_version": 2, "mode": "reconcile", "model_install": outcome}))?;
            return Ok(if verified { 0 } else { 4 });
        }
        if let Ok(record) = writer.store.restore_operation(operation) {
            return Err(format!(
                "RESTORE_RESUME_REQUIRED: resume with `{}`",
                restore::restore_command(record.target_snapshot)
            ));
        }
        let decision = if rebind {
            Some(apply::rebind_decision(
                &writer.store,
                &mut writer.port,
                operation,
                "recover reconcile --rebind",
                now_ms()?.to_string(),
            )?)
        } else {
            None
        };
        let outcome = apply::reconcile(
            &mut writer.store,
            &writer.lease,
            &mut writer.port,
            &ReconcileRequest {
                operation_id: operation,
                apply: true,
                rebind: decision,
            },
        )?;
        let resolved = matches!(
            outcome.state,
            OperationState::Committed
                | OperationState::FailedBeforeWrite
                | OperationState::Restored
        );
        emit(&json!({"schema_version": 2, "mode": "reconcile", "reconcile": outcome}))?;
        Ok(if resolved { 0 } else { 4 })
    })();
    writer.finish();
    result
}

/// OP-41 `backup create --apply`: a verified full-collection checkpoint.
pub(crate) fn backup_create(
    settings: &linguist_config::Effective,
    preference: ScopePreference,
    scope_manifest_given: bool,
    output: Option<PathBuf>,
) -> Result<u8, String> {
    let client = crate::anki_client(settings)?;
    let port = connect(&client, settings)?;
    if preference != ScopePreference::Collection || scope_manifest_given {
        return Err("CHECKPOINT_SCOPE_PREFERENCE_UNSUPPORTED: native export writes whole-collection packages with a natively observed scope; use --scope collection without --scope-manifest".into());
    }
    let mut writer = Writer::open(port, settings)?;
    let result = (|| {
        let (checkpoint, _) = writer.checkpoint_to(settings, &[], &[], None, output.clone())?;
        let record = writer.store.checkpoint(checkpoint)?;
        emit(&json!({
            "schema_version": 2, "mode": "apply", "checkpoint": backup::summarize(&writer.store, &record)?,
            "collection_writes_enabled": true,
        }))?;
        Ok(0)
    })();
    writer.finish();
    result
}

/// OP-25 `models install PURPOSE --apply`.
pub(crate) fn models_install(
    settings: &linguist_config::Effective,
    target: linguist_core::model::ManagedModel,
) -> Result<u8, String> {
    let client = crate::anki_client(settings)?;
    let port = connect(&client, settings)?;
    let mut writer = Writer::open(port, settings)?;
    let result = (|| {
        let group = Uuid::new_v4();
        let (checkpoint, protected) = writer.checkpoint(settings, &[], &[], Some(group))?;
        let binding = writer.port.binding();
        let outcome = model_install::install(
            &mut writer.store,
            &writer.lease,
            &mut writer.port,
            model_install::InstallRequest {
                binding,
                target,
                checkpoint_id: checkpoint,
                group_id: Some(group),
                protected_manifest_digest: &protected,
                reuse_max_age_seconds: reuse_seconds(settings),
                max_package_bytes: max_package(settings),
                now_ms: now_ms()?,
            },
        )?;
        let verified = outcome.verified;
        emit(&json!({
            "schema_version": 2, "mode": "apply", "model_install": outcome,
            "checkpoint_id": checkpoint, "collection_writes_enabled": true,
        }))?;
        Ok(if verified { 0 } else { 4 })
    })();
    writer.finish();
    result
}

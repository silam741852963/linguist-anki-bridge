//! ALG-BACKUP orchestration over an injected native export port. A checkpoint
//! counts only after the local artifact, its scope and a disposable restoration
//! test are verified; an API success alone is never evidence.
use crate::checkpoint::{
    CoverageRequirement, PackageLimits, ScopeManifest, inspect_colpkg_scope, restore_test,
};
use linguist_core::{
    canonical,
    records::{
        BackupReceipt, CollectionBinding, JournalStep, OperationJournal, OperationState, StepState,
    },
    validation::{Issue, Severity},
};
use linguist_store::{
    Store, checkpoint::CheckpointRecord, journal::JournalVersion, lease::LeaseToken,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, String>;

/// Outcome of one external request. `Rejected` is used only when the adapter
/// proves the request had no effect; anything uncertain is `Unknown`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortFailure {
    Rejected(String),
    Unknown(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct ExportRequest {
    pub operation_id: Uuid,
    pub binding: CollectionBinding,
    /// Create-new temporary path beside the final artifact.
    pub destination: PathBuf,
    pub include_media: bool,
    pub include_scheduling: bool,
}

/// What the adapter claims it wrote. Every value is checked against the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportClaim {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
}

pub trait CheckpointExporter {
    fn export_checkpoint(
        &mut self,
        request: &ExportRequest,
    ) -> std::result::Result<ExportClaim, PortFailure>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopePreference {
    Affected,
    Collection,
}

impl std::str::FromStr for ScopePreference {
    type Err = String;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "affected" => Ok(Self::Affected),
            "collection" => Ok(Self::Collection),
            _ => Err("CHECKPOINT_SCOPE_PREFERENCE_INVALID".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoveragePlan {
    pub preference: ScopePreference,
    pub package_scope: &'static str,
    pub escalated: bool,
    pub escalation_reason: Option<&'static str>,
    pub requirement: CoverageRequirement,
}

/// Schema actions always need the collection package. Affected-scope deck
/// packages are not implemented, so content-only scopes escalate too.
pub fn plan_coverage(preference: ScopePreference, scope: &ScopeManifest) -> CoveragePlan {
    let reason = match preference {
        ScopePreference::Collection => None,
        ScopePreference::Affected if scope.requirement.schema => {
            Some("schema_action_requires_collection_package")
        }
        ScopePreference::Affected => Some("affected_scope_package_unavailable"),
    };
    let requirement = if scope.requirement.schema {
        CoverageRequirement {
            scheduling: true,
            media: true,
            schema: true,
        }
    } else {
        scope.requirement
    };
    CoveragePlan {
        preference,
        package_scope: "collection",
        escalated: reason.is_some(),
        escalation_reason: reason,
        requirement,
    }
}

pub struct CheckpointRequest {
    pub binding: CollectionBinding,
    pub scope: ScopeManifest,
    pub preference: ScopePreference,
    /// Final create-new `.colpkg` path; its parent must already exist.
    pub output: PathBuf,
    pub group_id: Option<Uuid>,
    /// Caller digest of the protected models/notes this checkpoint covers.
    pub protected_manifest_digest: String,
    /// Existing disposable directory for the mandatory restoration test.
    pub restore_target: PathBuf,
    pub limits: PackageLimits,
    pub now_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckpointOutcome {
    pub record: CheckpointRecord,
    pub journal_id: Uuid,
    pub coverage: CoveragePlan,
}

/// Failure that leaves durable journal evidence. Dependent writes must stop.
#[derive(Clone, Debug, Serialize)]
pub struct CheckpointFailure {
    pub code: String,
    pub journal_id: Option<Uuid>,
    pub journal_state: Option<OperationState>,
}

impl From<CheckpointFailure> for String {
    fn from(failure: CheckpointFailure) -> Self {
        failure.code
    }
}

fn digest<T: Serialize + ?Sized>(domain: &str, value: &T) -> Result<String> {
    canonical::digest(domain, value).map_err(|e| e.to_string())
}

pub fn binding_digest(binding: &CollectionBinding) -> Result<String> {
    digest("lab-collection-binding-v1", binding)
}

fn sha256_file(path: &Path, limit: u64) -> Result<(u64, String)> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "CHECKPOINT_ARTIFACT_MISSING")?;
    if !metadata.file_type().is_file() {
        return Err("CHECKPOINT_ARTIFACT_INVALID".into());
    }
    let mut file = File::open(path).map_err(|_| "CHECKPOINT_ARTIFACT_MISSING")?;
    let mut hash = <sha2::Sha256 as sha2::Digest>::new();
    let mut total = 0u64;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut chunk)
            .map_err(|_| "CHECKPOINT_ARTIFACT_READ_FAILED")?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > limit {
            return Err("CHECKPOINT_ARTIFACT_INVALID".into());
        }
        sha2::Digest::update(&mut hash, &chunk[..count]);
    }
    Ok((total, format!("{:x}", sha2::Digest::finalize(hash))))
}

fn sync_parent(path: &Path) -> Result<()> {
    let parent = path.parent().ok_or("CHECKPOINT_OUTPUT_INVALID")?;
    File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|_| "CHECKPOINT_OUTPUT_SYNC_FAILED".into())
}

/// Validates a create-new final path and returns the canonical path plus its
/// private temporary sibling for `operation`.
pub fn checkpoint_paths(output: &Path, operation: Uuid) -> Result<(PathBuf, PathBuf)> {
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| name.ends_with(".colpkg") && name.len() > 7 && !name.starts_with('.'))
        .ok_or("CHECKPOINT_OUTPUT_INVALID")?;
    if !output.is_absolute() {
        return Err("CHECKPOINT_OUTPUT_MUST_BE_ABSOLUTE".into());
    }
    let parent = output.parent().ok_or("CHECKPOINT_OUTPUT_INVALID")?;
    let parent = fs::canonicalize(parent).map_err(|_| "CHECKPOINT_OUTPUT_PARENT_UNAVAILABLE")?;
    if !fs::metadata(&parent)
        .map_err(|_| "CHECKPOINT_OUTPUT_PARENT_UNAVAILABLE")?
        .is_dir()
    {
        return Err("CHECKPOINT_OUTPUT_PARENT_UNAVAILABLE".into());
    }
    let output = parent.join(name);
    if fs::symlink_metadata(&output).is_ok() {
        return Err("CHECKPOINT_OUTPUT_EXISTS".into());
    }
    let temporary = parent.join(format!(".{name}.lab-tmp-{}", operation.simple()));
    Ok((output, temporary))
}

fn issue(code: &str, message: impl Into<String>) -> Issue {
    let mut issue = Issue::new(code, Severity::Error, None, message);
    issue.stage = "checkpoint".into();
    issue
}

struct Journal<'a> {
    store: &'a mut Store,
    version: JournalVersion,
}

impl Journal<'_> {
    fn advance(&mut self, change: impl FnOnce(&mut OperationJournal)) -> Result<()> {
        let mut next = self.version.journal.clone();
        change(&mut next);
        self.version = self.store.append_journal(&next, Some(&self.version))?;
        Ok(())
    }
}

/// Runs ALG-BACKUP for one scope-closed group under an already-held writer lease.
pub fn create_checkpoint(
    store: &mut Store,
    lease: &LeaseToken,
    exporter: &mut dyn CheckpointExporter,
    request: CheckpointRequest,
) -> std::result::Result<CheckpointOutcome, CheckpointFailure> {
    let early = |code: String| CheckpointFailure {
        code,
        journal_id: None,
        journal_state: None,
    };
    request.scope.validate().map_err(early)?;
    if request.protected_manifest_digest.trim().is_empty() {
        return Err(early("CHECKPOINT_PROTECTED_MANIFEST_REQUIRED".into()));
    }
    store.validate_lease(lease).map_err(early)?;
    let coverage = plan_coverage(request.preference, &request.scope);
    let operation = Uuid::new_v4();
    let receipt_id = Uuid::new_v4();
    let (output, temporary) = checkpoint_paths(&request.output, operation).map_err(early)?;
    let scope_digest = request.scope.digest().map_err(early)?;
    let export = ExportRequest {
        operation_id: operation,
        binding: request.binding.clone(),
        destination: temporary.clone(),
        include_media: coverage.requirement.media || coverage.package_scope == "collection",
        include_scheduling: true,
    };
    let step = JournalStep {
        id: Uuid::new_v4(),
        action: "export_checkpoint".into(),
        payload_digest: digest("lab-checkpoint-export-request-v1", &export).map_err(early)?,
        precondition_digest: binding_digest(&request.binding).map_err(early)?,
        expected_post_digest: scope_digest.clone(),
        state: StepState::IntentRecorded,
        observed_digest: None,
    };
    let initial = OperationJournal {
        id: operation,
        group_id: request.group_id,
        approval_digest: format!("lab-checkpoint-v1:{scope_digest}"),
        binding: request.binding.clone(),
        snapshot_id: receipt_id,
        backup_id: receipt_id,
        state: OperationState::Prepared,
        steps: vec![step],
        issues: vec![],
    };
    let version = store.append_journal(&initial, None).map_err(early)?;
    let mut journal = Journal { store, version };
    let failed = |journal: &Journal, code: String| CheckpointFailure {
        code,
        journal_id: Some(operation),
        journal_state: Some(journal.version.journal.state),
    };
    for state in [OperationState::Preflight, OperationState::Checkpointed] {
        if let Err(code) = journal.advance(|j| j.state = state) {
            return Err(failed(&journal, code));
        }
    }
    // Durable request_started precedes dispatch; a local durability failure stops here.
    if let Err(code) = journal.advance(|j| {
        j.state = OperationState::Mutating;
        j.steps[0].state = StepState::RequestStarted;
    }) {
        return Err(failed(&journal, code));
    }
    let result = exporter.export_checkpoint(&export);
    let reject = |journal: &mut Journal, code: String| -> CheckpointFailure {
        // The temporary path is ours and create-new; no collection state changed.
        let _ = fs::remove_file(&temporary);
        let observed = digest("lab-checkpoint-failure-v1", &code).ok();
        let recorded = journal.advance(|j| {
            j.steps[0].state = StepState::ObservedFailure;
            j.steps[0].observed_digest = observed;
            j.state = OperationState::FailedBeforeWrite;
            j.issues.push(issue(&code, "checkpoint was not verified"));
        });
        CheckpointFailure {
            code: recorded.err().unwrap_or(code),
            journal_id: Some(operation),
            journal_state: Some(journal.version.journal.state),
        }
    };
    let claim = match result {
        Ok(claim) => claim,
        Err(PortFailure::Rejected(reason)) => {
            return Err(reject(
                &mut journal,
                format!("CHECKPOINT_EXPORT_REJECTED: {reason}"),
            ));
        }
        Err(PortFailure::Unknown(reason)) => {
            let code = format!("CHECKPOINT_EXPORT_UNKNOWN: {reason}");
            let recorded = journal.advance(|j| {
                j.steps[0].state = StepState::Unknown;
                j.state = OperationState::NeedsRecovery;
                j.issues.push(issue(
                    "CHECKPOINT_EXPORT_UNKNOWN",
                    format!(
                        "export outcome unknown; temporary path {}",
                        temporary.display()
                    ),
                ));
            });
            return Err(failed(&journal, recorded.err().unwrap_or(code)));
        }
    };
    // Never equate the adapter's claim with a verified file.
    let verified = (|| -> Result<_> {
        if claim.path != temporary {
            return Err("CHECKPOINT_EXPORT_PATH_MISMATCH".into());
        }
        let (size, sha256) = sha256_file(&temporary, request.limits.max_package_bytes)?;
        if size == 0 || size != claim.size_bytes || sha256 != claim.sha256 {
            return Err("CHECKPOINT_EXPORT_CLAIM_MISMATCH".into());
        }
        let (inspection, report) =
            inspect_colpkg_scope(&temporary, request.limits.clone(), &request.scope)?;
        if inspection.package_sha256 != sha256 {
            return Err("CHECKPOINT_ARTIFACT_CHANGED".into());
        }
        let restoration = restore_test(
            &temporary,
            request.limits.clone(),
            &request.scope,
            &request.restore_target,
        )?;
        if restoration.package_sha256 != sha256 || restoration.scope != report {
            return Err("CHECKPOINT_RESTORATION_SCOPE_MISMATCH".into());
        }
        Ok((size, sha256, inspection, report, restoration))
    })();
    let (size, sha256, inspection, report, restoration) = match verified {
        Ok(value) => value,
        Err(code) => return Err(reject(&mut journal, code)),
    };
    // Finalize the artifact create-new before any receipt exists.
    if let Err(error) = fs::hard_link(&temporary, &output) {
        let code = if error.kind() == std::io::ErrorKind::AlreadyExists {
            "CHECKPOINT_OUTPUT_EXISTS"
        } else {
            "CHECKPOINT_OUTPUT_LINK_FAILED"
        };
        return Err(reject(&mut journal, code.into()));
    }
    let _ = fs::remove_file(&temporary);
    let finalized = sync_parent(&output).and_then(|_| {
        let (final_size, final_sha) = sha256_file(&output, request.limits.max_package_bytes)?;
        if final_size != size || final_sha != sha256 {
            return Err("CHECKPOINT_ARTIFACT_CHANGED".to_owned());
        }
        Ok(())
    });
    if let Err(code) = finalized {
        let _ = fs::remove_file(&output);
        return Err(reject(&mut journal, code));
    }
    let restoration_value = serde_json::to_value(&restoration).map_err(|e| early(e.to_string()))?;
    let evidence = serde_json::json!({
        "schema_version": 1,
        "scope": request.scope,
        "coverage": coverage,
        "group_id": request.group_id,
        "protected_manifest_digest": request.protected_manifest_digest,
        "inspection": inspection,
        "scope_report": report,
        "restoration": restoration_value,
        "adapter": {"port": "export_checkpoint", "include_media": export.include_media},
    });
    let verification_digest = digest(
        "lab-checkpoint-verification-v1",
        &serde_json::json!({"inspection": evidence["inspection"], "scope_report": evidence["scope_report"]}),
    )
    .map_err(early)?;
    let receipt = BackupReceipt {
        id: receipt_id,
        binding: request.binding.clone(),
        path: output.to_string_lossy().into_owned(),
        checksum: sha256,
        scope_digest: scope_digest.clone(),
        includes_scheduling: report.scheduling_included,
        includes_media: export.include_media && report.media_verified == request.scope.media.len(),
        includes_schema: report.schema_included,
        verification_digest,
        restoration_evidence: Some(
            digest("lab-checkpoint-restoration-v1", &restoration_value).map_err(early)?,
        ),
    };
    let record = CheckpointRecord {
        receipt,
        operation_id: operation,
        created_ms: request.now_ms,
        scope: coverage.package_scope.into(),
        size_bytes: size,
        evidence,
    };
    if let Err(code) = journal.advance(|j| {
        j.steps[0].state = StepState::ObservedSuccess;
        j.steps[0].observed_digest = Some(scope_digest.clone());
        j.state = OperationState::Verifying;
    }) {
        return Err(failed(&journal, code));
    }
    if let Err(code) = journal.store.publish_checkpoint(&record) {
        let recorded = journal.advance(|j| {
            j.state = OperationState::NeedsRecovery;
            j.issues.push(issue(
                "CHECKPOINT_RECEIPT_UNSAVED",
                format!(
                    "verified artifact {} has no saved receipt",
                    output.display()
                ),
            ));
        });
        return Err(failed(&journal, recorded.err().unwrap_or(code)));
    }
    if let Err(code) = journal.advance(|j| {
        j.steps[0].state = StepState::Verified;
        j.state = OperationState::Committed;
    }) {
        return Err(failed(&journal, code));
    }
    Ok(CheckpointOutcome {
        record,
        journal_id: operation,
        coverage,
    })
}

/// The dependent write's exact needs. Every listed entry must be inside the
/// checkpoint's verified scope.
pub struct DependentScope<'a> {
    pub binding: &'a CollectionBinding,
    pub requirement: CoverageRequirement,
    pub note_ids: &'a [i64],
    pub card_ids: &'a [i64],
    pub model_ids: &'a [i64],
    pub media_names: &'a [String],
    pub group_id: Option<Uuid>,
    pub protected_manifest_digest: &'a str,
    pub now_ms: u64,
    pub reuse_max_age_seconds: u64,
    pub max_package_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckpointAuthorization {
    pub receipt_id: Uuid,
    pub scope_digest: String,
    pub reuse_reason: Option<&'static str>,
}

pub(crate) fn same_execution(a: &CollectionBinding, b: &CollectionBinding) -> bool {
    a.endpoint == b.endpoint
        && a.profile_fingerprint == b.profile_fingerprint
        && a.path_fingerprint == b.path_fingerprint
        && a.bridge_id == b.bridge_id
        && a.lineage_id == b.lineage_id
        && a.session_epoch == b.session_epoch
}

/// Fail-closed gate before any dependent collection write. It re-hashes the
/// artifact, so a deleted or changed file blocks the write.
pub fn require_checkpoint(
    store: &Store,
    receipt_id: Uuid,
    dependent: &DependentScope,
) -> Result<CheckpointAuthorization> {
    let record = store.checkpoint(receipt_id)?;
    let receipt = &record.receipt;
    if !same_execution(&receipt.binding, dependent.binding) {
        return Err("CHECKPOINT_BINDING_MISMATCH".into());
    }
    let restoration = &record.evidence["restoration"];
    if restoration["passed"] != true
        || restoration["scope_digest"] != receipt.scope_digest.as_str()
        || restoration["package_sha256"] != receipt.checksum.as_str()
        || receipt.restoration_evidence.as_deref()
            != Some(digest("lab-checkpoint-restoration-v1", restoration)?.as_str())
    {
        return Err("CHECKPOINT_RESTORATION_UNVERIFIED".into());
    }
    let need = dependent.requirement;
    if (need.scheduling && !receipt.includes_scheduling)
        || (need.media && !receipt.includes_media)
        || (need.schema && (!receipt.includes_schema || record.scope != "collection"))
    {
        return Err("CHECKPOINT_COVERAGE_INSUFFICIENT".into());
    }
    let scope: ScopeManifest = serde_json::from_value(record.evidence["scope"].clone())
        .map_err(|_| "CHECKPOINT_RECORD_CORRUPT")?;
    if scope.digest()? != receipt.scope_digest {
        return Err("CHECKPOINT_RECORD_CORRUPT".into());
    }
    let cards: Vec<i64> = scope.cards.iter().map(|card| card.card_id).collect();
    if dependent
        .note_ids
        .iter()
        .any(|id| scope.note_ids.binary_search(id).is_err())
        || dependent
            .card_ids
            .iter()
            .any(|id| cards.binary_search(id).is_err())
        || dependent
            .model_ids
            .iter()
            .any(|id| scope.model_ids.binary_search(id).is_err())
        || dependent
            .media_names
            .iter()
            .any(|name| !scope.media.iter().any(|media| &media.name == name))
    {
        return Err("CHECKPOINT_SCOPE_INSUFFICIENT".into());
    }
    let recorded_group = record.evidence["group_id"]
        .as_str()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| "CHECKPOINT_RECORD_CORRUPT")?;
    let reuse_reason = if recorded_group.is_some() && recorded_group == dependent.group_id {
        None
    } else {
        let age_ms = dependent.now_ms.saturating_sub(record.created_ms);
        if dependent.reuse_max_age_seconds == 0
            || dependent.now_ms < record.created_ms
            || age_ms > dependent.reuse_max_age_seconds.saturating_mul(1000)
            || record.evidence["protected_manifest_digest"] != dependent.protected_manifest_digest
        {
            return Err("CHECKPOINT_REUSE_REJECTED".into());
        }
        Some("cross_group_reuse_unchanged_protected_manifest_within_age")
    };
    let (size, sha256) = sha256_file(Path::new(&receipt.path), dependent.max_package_bytes)?;
    if size != record.size_bytes || sha256 != receipt.checksum {
        return Err("CHECKPOINT_ARTIFACT_CHANGED".into());
    }
    Ok(CheckpointAuthorization {
        receipt_id,
        scope_digest: receipt.scope_digest.clone(),
        reuse_reason,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckpointSummary {
    pub id: Uuid,
    pub created_ms: u64,
    pub path: String,
    pub checksum: String,
    pub size_bytes: u64,
    pub scope: String,
    pub scope_digest: String,
    pub includes_scheduling: bool,
    pub includes_media: bool,
    pub includes_schema: bool,
    pub restoration_tested: bool,
    pub later_verifications: usize,
    pub checkpoint_eligible: bool,
}

/// Local read only; eligibility is the stored verified evidence, not a fresh check.
pub fn summarize(store: &Store, record: &CheckpointRecord) -> Result<CheckpointSummary> {
    let restoration = &record.evidence["restoration"];
    let restoration_tested = restoration["passed"] == true
        && restoration["scope_digest"] == record.receipt.scope_digest.as_str();
    Ok(CheckpointSummary {
        id: record.receipt.id,
        created_ms: record.created_ms,
        path: record.receipt.path.clone(),
        checksum: record.receipt.checksum.clone(),
        size_bytes: record.size_bytes,
        scope: record.scope.clone(),
        scope_digest: record.receipt.scope_digest.clone(),
        includes_scheduling: record.receipt.includes_scheduling,
        includes_media: record.receipt.includes_media,
        includes_schema: record.receipt.includes_schema,
        restoration_tested,
        later_verifications: store.checkpoint_verifications(record.receipt.id)?.len(),
        checkpoint_eligible: restoration_tested,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct RegisteredVerification {
    pub receipt_id: Uuid,
    pub artifact_present: bool,
    pub checksum_matches: bool,
    pub scope_verified: bool,
    pub restoration_tested: bool,
    pub verified: bool,
    pub evidence: serde_json::Value,
}

/// Re-verifies a registered receipt's artifact and scope; with a disposable
/// target it also repeats the restoration test and appends the evidence.
pub fn verify_registered(
    store: &mut Store,
    receipt_id: Uuid,
    limits: PackageLimits,
    restore_target: Option<&Path>,
    now_ms: u64,
) -> Result<RegisteredVerification> {
    let record = store.checkpoint(receipt_id)?;
    let path = Path::new(&record.receipt.path);
    let (size, sha256) = sha256_file(path, limits.max_package_bytes)?;
    if size != record.size_bytes || sha256 != record.receipt.checksum {
        return Err("CHECKPOINT_ARTIFACT_CHANGED".into());
    }
    let scope: ScopeManifest = serde_json::from_value(record.evidence["scope"].clone())
        .map_err(|_| "CHECKPOINT_RECORD_CORRUPT")?;
    let (inspection, report) = inspect_colpkg_scope(path, limits.clone(), &scope)?;
    if inspection.package_sha256 != record.receipt.checksum {
        return Err("CHECKPOINT_ARTIFACT_CHANGED".into());
    }
    let restoration = restore_target
        .map(|target| restore_test(path, limits, &scope, target))
        .transpose()?;
    let evidence = serde_json::json!({
        "schema_version": 1,
        "inspection": inspection,
        "scope_report": report,
        "restoration": restoration,
    });
    if restoration.is_some() {
        store.append_checkpoint_verification(receipt_id, now_ms, &evidence)?;
    }
    Ok(RegisteredVerification {
        receipt_id,
        artifact_present: true,
        checksum_matches: true,
        scope_verified: true,
        restoration_tested: restoration.is_some(),
        verified: true,
        evidence,
    })
}

/// Close an interrupted checkpoint journal (process exit during export or
/// verification). A native export never changes collection state, so the
/// journal ends `failed_before_write` with `CHECKPOINT_ABANDONED`; only this
/// operation's create-new temporary package in `output_dir` is removed. A
/// journal whose receipt was saved is never abandoned.
pub fn abandon_checkpoint(
    store: &mut Store,
    lease: &LeaseToken,
    operation: Uuid,
    output_dir: &Path,
) -> Result<OperationJournal> {
    store.validate_lease(lease)?;
    let version = store.journal(operation)?;
    let journal = &version.journal;
    if journal.steps.len() != 1 || journal.steps[0].action != "export_checkpoint" {
        return Err("CHECKPOINT_JOURNAL_REQUIRED".into());
    }
    if store.checkpoint(journal.backup_id).is_ok() {
        return Err("CHECKPOINT_RECEIPT_EXISTS: the checkpoint was verified and saved".into());
    }
    if matches!(
        journal.state,
        OperationState::Committed | OperationState::FailedBeforeWrite
    ) {
        return Ok(journal.clone());
    }
    let suffix = format!(".lab-tmp-{}", operation.simple());
    if let Ok(entries) = fs::read_dir(output_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                && name.ends_with(&suffix)
                && entry.file_type().is_ok_and(|t| t.is_file())
            {
                fs::remove_file(entry.path()).map_err(|_| "CHECKPOINT_TEMPORARY_REMOVE_FAILED")?;
            }
        }
    }
    let mut journal = Journal { store, version };
    journal.advance(|j| {
        if j.steps[0].state != StepState::ObservedFailure {
            j.steps[0].state = StepState::ObservedFailure;
            j.steps[0].observed_digest =
                digest("lab-checkpoint-failure-v1", &"CHECKPOINT_ABANDONED").ok();
        }
        j.state = OperationState::FailedBeforeWrite;
        j.issues.push(issue(
            "CHECKPOINT_ABANDONED",
            "interrupted before a verified receipt; no collection state changed",
        ));
    })?;
    Ok(journal.version.journal)
}

/// Seconds a collection-writer lease is held or renewed for.
pub const WRITER_LEASE_SECONDS: u64 = 600;

/// Renew the collection-writer lease before the next unit of a long group
/// (a split apply, a group rollback): one checkpoint plus many units can
/// outlast one lease period (WP-23, `LEASE_STALE_OR_EXPIRED` mid-group).
/// Renewal fails if another writer took the lease; the group then stops.
pub fn keep_writer_lease(store: &mut Store, lease: &LeaseToken) -> Result<()> {
    store.renew_lease(lease, WRITER_LEASE_SECONDS)
}

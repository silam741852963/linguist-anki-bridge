//! The `lab-native-v1` transport behind every managed write (WP-03).
//!
//! [`NativePort`] implements [`ApplyPort`], [`CheckpointExporter`] and
//! [`ModelPort`] over the companion actions that
//! `linguist_anki::native` speaks (`labBegin`, `labInspect`, `labMutate`,
//! `labOperationStatus`, `labEnd`). It is constructed only from a
//! [`linguist_anki::native::VerifiedNative`] declaration, so an unverified
//! build, a missing API key, a remote endpoint or a missing session keeps
//! every write unavailable. A mutation is accepted by the companion, then
//! polled until it is terminal or the deadline passes; a deadline or an
//! ambiguous transport error is reported as unknown, never as success, and
//! the orchestration still checks an actual read-back.
use crate::{
    apply::{
        ApplyPort, Effect, MutationRequest, NativeStatus, ObservedDeck, ObservedMedia,
        ObservedNote, OwnerToken,
    },
    backup::{CheckpointExporter, ExportClaim, ExportRequest, PortFailure, Result},
    checkpoint::{CoverageRequirement, ScopeManifest},
    model_install::{ModelInstallRequest, ModelPort, ObservedModel, OperationEvidence},
};
use base64::Engine;
use linguist_anki::{
    Client,
    native::{NativeObservation, NativeOperationState, NativeOwner, VerifiedNative},
};
use linguist_core::{canonical, records::CollectionBinding};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Deadlines for one native operation, from settings.
#[derive(Clone, Copy, Debug)]
pub struct NativeDeadlines {
    /// Accept-to-terminal deadline for one note/media/model operation.
    pub operation: Duration,
    /// Accept-to-terminal deadline for a checkpoint export.
    pub export: Duration,
    pub poll: Duration,
}

pub struct NativePort<'a> {
    client: &'a Client,
    state_root: PathBuf,
    deadlines: NativeDeadlines,
    max_asset_bytes: u64,
    verified: VerifiedNative,
    owner: Option<NativeOwner>,
}

fn rejected(error: String) -> PortFailure {
    // A companion `BRIDGE_*` refusal of the request itself queued nothing.
    if error.starts_with("ANKI_ACTION_REJECTED: labMutate: BRIDGE_") {
        PortFailure::Rejected(error)
    } else {
        PortFailure::Unknown(error)
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value, code: &str) -> Result<T> {
    serde_json::from_value(value).map_err(|_| code.to_owned())
}

/// Approval identity of a checkpoint export, distinct from plan approvals.
pub fn checkpoint_approval(operation: Uuid, binding: &CollectionBinding) -> Result<String> {
    canonical::digest(
        "checkpoint",
        &json!({"operation_id": operation, "lineage_id": binding.lineage_id,
                "session_epoch": binding.session_epoch}),
    )
    .map_err(|e| e.to_string())
}

/// Approval identity of a managed model install.
pub fn model_install_approval(operation: Uuid, manifest_digest: &str) -> Result<String> {
    canonical::digest(
        "model-install",
        &json!({"operation_id": operation, "manifest_digest": manifest_digest}),
    )
    .map_err(|e| e.to_string())
}

impl<'a> NativePort<'a> {
    /// Fails closed unless the companion passes `Client::native_verified`.
    pub fn connect(
        client: &'a Client,
        state_root: &Path,
        deadlines: NativeDeadlines,
        max_asset_bytes: u64,
    ) -> Result<Self> {
        let verified = client.native_verified()?;
        Ok(Self {
            client,
            state_root: state_root.to_owned(),
            deadlines,
            max_asset_bytes,
            verified,
            owner: None,
        })
    }

    /// Binding of the session verified at the last capability read.
    pub fn binding(&self) -> CollectionBinding {
        CollectionBinding {
            endpoint: self.client.endpoint(),
            profile_fingerprint: self.verified.profile_fingerprint.clone(),
            path_fingerprint: self.verified.path_fingerprint.clone(),
            bridge_id: self.verified.bridge_id,
            lineage_id: self.verified.lineage_id,
            session_epoch: self.verified.session_epoch,
            capability_digest: self.verified.manifest_digest.clone(),
        }
    }

    pub fn refresh(&mut self) -> Result<CollectionBinding> {
        self.verified = self.client.native_verified()?;
        Ok(self.binding())
    }

    fn inspect(&self, kind: &str, params: Value) -> Result<Value> {
        self.client
            .native_inspect(self.verified.session_epoch, kind, params)
    }

    /// Checkpoint scope manifest for these notes and extra models, observed natively.
    pub fn scope(
        &self,
        note_ids: &[i64],
        model_ids: &[i64],
        requirement: CoverageRequirement,
    ) -> Result<ScopeManifest> {
        let value = self.inspect(
            "scope",
            json!({"note_ids": note_ids, "model_ids": model_ids, "requirement": requirement}),
        )?;
        let scope: ScopeManifest = decode(value, "ANKI_NATIVE_SCOPE_INVALID")?;
        scope.validate()?;
        Ok(scope)
    }

    /// Complete bounded note evidence (`inspection.py`): fields, model,
    /// every card with scheduling, FSRS state and review rows, and media.
    pub fn note_evidence(&self, note_id: i64) -> Result<Value> {
        self.inspect("note_evidence", json!({"note_id": note_id}))
    }

    fn owner_for(&mut self, binding: &CollectionBinding, approval: &str) -> Result<NativeOwner> {
        let binding = serde_json::to_value(binding).map_err(|e| e.to_string())?;
        let owner = self.client.native_begin(binding, approval)?;
        self.owner = Some(owner.clone());
        Ok(owner)
    }

    /// Hand one approved local asset to the companion's private staging
    /// directory (create-new, verified by hash); the companion reads it by
    /// digest and never receives a path.
    fn stage(&self, owner: &NativeOwner, staged_asset: &str, sha256: &str) -> Result<()> {
        if staged_asset != sha256
            || sha256.len() != 64
            || !sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("APPLY_MEDIA_STAGE_INVALID".into());
        }
        let bytes = linguist_store::Store::read_only(&self.state_root)?
            .asset(staged_asset, self.max_asset_bytes)?;
        if canonical::asset_digest(&bytes) != sha256 {
            return Err("APPLY_MEDIA_STAGE_INVALID".into());
        }
        let directory = PathBuf::from(&owner.staging_dir);
        let metadata =
            fs::symlink_metadata(&directory).map_err(|_| "ANKI_NATIVE_STAGING_INVALID")?;
        if !directory.is_absolute() || !metadata.is_dir() {
            return Err("ANKI_NATIVE_STAGING_INVALID".into());
        }
        let target = directory.join(sha256);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&target)
        {
            Ok(mut file) => {
                file.write_all(&bytes)
                    .map_err(|_| "ANKI_NATIVE_STAGING_WRITE_FAILED")?;
                file.sync_all()
                    .map_err(|_| "ANKI_NATIVE_STAGING_WRITE_FAILED")?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Content addressed: an existing file must hold exactly these bytes.
                let existing = fs::read(&target).map_err(|_| "ANKI_NATIVE_STAGING_INVALID")?;
                if canonical::asset_digest(&existing) == sha256 {
                    Ok(())
                } else {
                    Err("ANKI_NATIVE_STAGING_CONFLICT".into())
                }
            }
            Err(_) => Err("ANKI_NATIVE_STAGING_WRITE_FAILED".into()),
        }
    }

    /// Submit one envelope and poll until terminal or the deadline passes.
    #[allow(clippy::too_many_arguments)]
    fn submit(
        &self,
        lineage_id: Uuid,
        operation_id: Uuid,
        session_epoch: Uuid,
        owner: &NativeOwner,
        approval: &str,
        variant: &str,
        payload: &[u8],
        deadline: Duration,
    ) -> std::result::Result<linguist_anki::native::NativeOperationStatus, PortFailure> {
        let payload = std::str::from_utf8(payload)
            .map_err(|_| PortFailure::Rejected("APPLY_PAYLOAD_INVALID".into()))?;
        let started = Instant::now();
        let mut status = self
            .client
            .native_mutate(
                lineage_id,
                operation_id,
                session_epoch,
                owner,
                approval,
                variant,
                payload,
            )
            .map_err(rejected)?;
        while matches!(
            status.state,
            NativeOperationState::Queued | NativeOperationState::Running
        ) {
            if started.elapsed() >= deadline {
                return Err(PortFailure::Unknown(format!(
                    "ANKI_NATIVE_TIMEOUT: operation {operation_id} still {:?} after {}s",
                    status.state,
                    deadline.as_secs()
                )));
            }
            std::thread::sleep(self.deadlines.poll);
            status = match self.client.native_observe(lineage_id, operation_id) {
                Ok(NativeObservation::Present(status)) => status,
                Ok(NativeObservation::Absent) => {
                    return Err(PortFailure::Unknown(
                        "ANKI_NATIVE_LEDGER_ROW_MISSING after acceptance".into(),
                    ));
                }
                Err(error) => return Err(PortFailure::Unknown(error)),
            };
        }
        Ok(status)
    }
}

fn native_status(status: &linguist_anki::native::NativeOperationStatus) -> NativeStatus {
    match status.state {
        NativeOperationState::Queued => NativeStatus::Queued,
        NativeOperationState::Running => NativeStatus::Running,
        NativeOperationState::Verified => NativeStatus::Verified,
        NativeOperationState::Unknown => NativeStatus::Unknown {
            reason: status.reason.clone().unwrap_or_default(),
        },
        NativeOperationState::FailedBeforeWrite => {
            let code = status
                .receipt
                .as_ref()
                .and_then(|r| r["code"].as_str())
                .unwrap_or_default();
            NativeStatus::FailedBeforeWrite {
                reason: format!(
                    "{}{}{code}",
                    status.reason.clone().unwrap_or_default(),
                    if code.is_empty() { "" } else { ": " }
                ),
            }
        }
    }
}

impl ApplyPort for NativePort<'_> {
    fn execution_binding(&mut self) -> Result<CollectionBinding> {
        self.refresh()
    }
    fn mutation_variants(&mut self) -> Result<Vec<String>> {
        Ok([
            "install_model",
            "export_checkpoint",
            "store_media",
            "create_note",
            "update_note",
            "restore_note",
            "delete_unstudied_created_note",
        ]
        .map(str::to_owned)
        .to_vec())
    }
    fn begin(&mut self, binding: &CollectionBinding, approval_digest: &str) -> Result<OwnerToken> {
        if binding != &self.binding() {
            return Err("ANKI_NATIVE_BINDING_STALE: refresh the execution binding first".into());
        }
        let owner = self.owner_for(binding, approval_digest)?;
        Ok(OwnerToken {
            token: owner.owner_token,
            fence: owner.fence,
        })
    }
    fn end(&mut self, owner: &OwnerToken) -> Result<()> {
        self.client.native_end(owner.token, owner.fence)?;
        if self
            .owner
            .as_ref()
            .is_some_and(|o| o.owner_token == owner.token)
        {
            self.owner = None;
        }
        Ok(())
    }
    fn note(&mut self, note_id: i64) -> Result<Option<ObservedNote>> {
        decode(
            self.inspect("note", json!({"note_id": note_id}))?,
            "ANKI_NATIVE_NOTE_INVALID",
        )
    }
    fn notes_tagged(&mut self, tag: &str) -> Result<Vec<ObservedNote>> {
        decode(
            self.inspect("notes_tagged", json!({"tag": tag}))?,
            "ANKI_NATIVE_NOTE_INVALID",
        )
    }
    fn models_named(&mut self, name: &str) -> Result<Vec<ObservedModel>> {
        decode(
            self.inspect("models_named", json!({"name": name}))?,
            "ANKI_NATIVE_MODEL_INVALID",
        )
    }
    fn deck(&mut self, name: &str) -> Result<Option<ObservedDeck>> {
        decode(
            self.inspect("deck", json!({"name": name}))?,
            "ANKI_NATIVE_DECK_INVALID",
        )
    }
    fn media(&mut self, filename: &str) -> Result<Option<ObservedMedia>> {
        decode(
            self.inspect("media", json!({"filename": filename}))?,
            "ANKI_NATIVE_MEDIA_INVALID",
        )
    }
    fn media_bytes(&mut self, filename: &str, max_bytes: u64) -> Result<Option<Vec<u8>>> {
        let value = self.inspect(
            "media_bytes",
            json!({"filename": filename, "max_bytes": max_bytes.min(64 << 20)}),
        )?;
        let Some(text) = value.as_str() else {
            return if value.is_null() {
                Ok(None)
            } else {
                Err("ANKI_NATIVE_MEDIA_INVALID".into())
            };
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(|_| "ANKI_NATIVE_MEDIA_INVALID")?;
        if bytes.len() as u64 > max_bytes {
            return Err("ANKI_MEDIA_TOO_LARGE".into());
        }
        Ok(Some(bytes))
    }
    fn mutate(
        &mut self,
        request: &MutationRequest,
    ) -> std::result::Result<NativeStatus, PortFailure> {
        let owner = match &self.owner {
            Some(owner)
                if owner.owner_token == request.owner.token
                    && owner.fence == request.owner.fence =>
            {
                owner.clone()
            }
            _ => return Err(PortFailure::Rejected("ANKI_NATIVE_OWNER_STALE".into())),
        };
        if let Effect::StoreMedia {
            staged_asset,
            sha256,
            ..
        } = &request.effect
        {
            self.stage(&owner, staged_asset, sha256)
                .map_err(PortFailure::Rejected)?;
        }
        let payload = request.effect.wire_bytes().map_err(PortFailure::Rejected)?;
        if canonical::asset_digest(&payload) != request.payload_digest {
            return Err(PortFailure::Rejected(
                "APPLY_PAYLOAD_DIGEST_MISMATCH".into(),
            ));
        }
        let status = self.submit(
            request.binding.lineage_id,
            request.operation_id,
            request.binding.session_epoch,
            &owner,
            &request.approval_digest,
            request.effect.variant(),
            &payload,
            self.deadlines.operation,
        )?;
        if status.payload_digest != request.payload_digest {
            return Err(PortFailure::Unknown(
                "ANKI_NATIVE_PAYLOAD_CONFLICT: ledger row holds another payload".into(),
            ));
        }
        Ok(native_status(&status))
    }
    fn status(&mut self, operation_id: Uuid) -> Result<NativeStatus> {
        match self
            .client
            .native_observe(self.verified.lineage_id, operation_id)?
        {
            NativeObservation::Absent => Ok(NativeStatus::Absent),
            NativeObservation::Present(status) => Ok(native_status(&status)),
        }
    }
}

fn copy_verified(source: &Path, destination: &Path, size: u64, sha256: &str) -> Result<()> {
    let mut input = File::open(source).map_err(|_| "CHECKPOINT_EXPORT_UNREADABLE")?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(destination)
        .map_err(|_| "CHECKPOINT_EXPORT_DESTINATION_INVALID")?;
    let mut hash = <sha2::Sha256 as sha2::Digest>::new();
    let mut total = 0u64;
    let mut chunk = vec![0u8; 1 << 20];
    loop {
        let count = input
            .read(&mut chunk)
            .map_err(|_| "CHECKPOINT_EXPORT_UNREADABLE")?;
        if count == 0 {
            break;
        }
        total += count as u64;
        sha2::Digest::update(&mut hash, &chunk[..count]);
        output
            .write_all(&chunk[..count])
            .map_err(|_| "CHECKPOINT_EXPORT_WRITE_FAILED")?;
    }
    output
        .sync_all()
        .map_err(|_| "CHECKPOINT_EXPORT_WRITE_FAILED")?;
    if total != size || format!("{:x}", sha2::Digest::finalize(hash)) != sha256 {
        return Err("CHECKPOINT_EXPORT_CLAIM_MISMATCH".into());
    }
    Ok(())
}

impl CheckpointExporter for NativePort<'_> {
    /// Native `export_checkpoint` into the companion's private exports
    /// directory, then a verified create-new copy to the requested path.
    fn export_checkpoint(
        &mut self,
        request: &ExportRequest,
    ) -> std::result::Result<ExportClaim, PortFailure> {
        let binding = self.refresh().map_err(PortFailure::Rejected)?;
        if !crate::apply::same_collection(&binding, &request.binding)
            || binding.session_epoch != request.binding.session_epoch
        {
            return Err(PortFailure::Rejected("CHECKPOINT_BINDING_MISMATCH".into()));
        }
        let approval =
            checkpoint_approval(request.operation_id, &binding).map_err(PortFailure::Rejected)?;
        let owner = self
            .owner_for(&binding, &approval)
            .map_err(PortFailure::Rejected)?;
        let payload = canonical::bytes(&json!({
            "schema_version": 1, "variant": "export_checkpoint",
            "body": {"include_media": request.include_media, "include_scheduling": true},
        }))
        .map_err(|e| PortFailure::Rejected(e.to_string()))?;
        let result = self.submit(
            binding.lineage_id,
            request.operation_id,
            binding.session_epoch,
            &owner,
            &approval,
            "export_checkpoint",
            &payload,
            self.deadlines.export,
        );
        let _ = self.client.native_end(owner.owner_token, owner.fence);
        self.owner = None;
        let status = result?;
        if status.state != NativeOperationState::Verified {
            return Err(PortFailure::Rejected(format!(
                "CHECKPOINT_EXPORT_NOT_VERIFIED: {}",
                native_status(&status).reason()
            )));
        }
        let receipt = status.receipt.unwrap_or(Value::Null);
        let (Some(path), Some(size), Some(sha256)) = (
            receipt["path"].as_str(),
            receipt["size_bytes"].as_u64(),
            receipt["sha256"].as_str(),
        ) else {
            return Err(PortFailure::Unknown(
                "CHECKPOINT_EXPORT_RECEIPT_INVALID".into(),
            ));
        };
        if let Err(code) = copy_verified(Path::new(path), &request.destination, size, sha256) {
            // Never leave an unverified copy at the requested path.
            if code != "CHECKPOINT_EXPORT_DESTINATION_INVALID" {
                let _ = fs::remove_file(&request.destination);
            }
            return Err(PortFailure::Rejected(code));
        }
        // The companion copy is no longer needed once the verified copy exists.
        let _ = fs::remove_file(path);
        Ok(ExportClaim {
            path: request.destination.clone(),
            size_bytes: size,
            sha256: sha256.to_owned(),
        })
    }
}

impl NativeStatus {
    fn reason(&self) -> String {
        match self {
            Self::FailedBeforeWrite { reason } | Self::Unknown { reason } => reason.clone(),
            other => format!("{other:?}"),
        }
    }
}

impl ModelPort for NativePort<'_> {
    fn models_named(&mut self, name: &str) -> Result<Vec<ObservedModel>> {
        ApplyPort::models_named(self, name)
    }
    fn install_model(
        &mut self,
        request: &ModelInstallRequest,
    ) -> std::result::Result<i64, PortFailure> {
        let binding = self.refresh().map_err(PortFailure::Rejected)?;
        if !crate::apply::same_collection(&binding, &request.binding) {
            return Err(PortFailure::Rejected(
                "MODEL_INSTALL_BINDING_MISMATCH".into(),
            ));
        }
        let approval = model_install_approval(request.operation_id, &request.manifest_digest)
            .map_err(PortFailure::Rejected)?;
        let owner = self
            .owner_for(&binding, &approval)
            .map_err(PortFailure::Rejected)?;
        let payload = canonical::bytes(&json!({
            "schema_version": 1, "variant": "install_model",
            "body": {"manifest": request.manifest, "manifest_digest": request.manifest_digest,
                     "expected_absent": request.expected_absent},
        }))
        .map_err(|e| PortFailure::Rejected(e.to_string()))?;
        let result = self.submit(
            binding.lineage_id,
            request.operation_id,
            binding.session_epoch,
            &owner,
            &approval,
            "install_model",
            &payload,
            self.deadlines.operation,
        );
        let _ = self.client.native_end(owner.owner_token, owner.fence);
        self.owner = None;
        let status = result?;
        match status.state {
            NativeOperationState::Verified => status
                .receipt
                .as_ref()
                .filter(|r| r["manifest_digest"] == request.manifest_digest.as_str())
                .and_then(|r| r["model_id"].as_i64())
                .ok_or_else(|| PortFailure::Unknown("MODEL_INSTALL_RECEIPT_INVALID".into())),
            NativeOperationState::FailedBeforeWrite => {
                Err(PortFailure::Rejected(native_status(&status).reason()))
            }
            _ => Err(PortFailure::Unknown(native_status(&status).reason())),
        }
    }
    fn operation_evidence(&mut self, operation_id: Uuid) -> Result<OperationEvidence> {
        match self
            .client
            .native_observe(self.verified.lineage_id, operation_id)?
        {
            NativeObservation::Absent => Ok(OperationEvidence::Absent),
            NativeObservation::Present(status) => match status.state {
                NativeOperationState::FailedBeforeWrite => Ok(OperationEvidence::FailedBeforeWrite),
                NativeOperationState::Verified => {
                    let receipt = status.receipt.unwrap_or(Value::Null);
                    match (
                        receipt["model_id"].as_i64(),
                        receipt["manifest_digest"].as_str(),
                    ) {
                        (Some(model_id), Some(digest)) => Ok(OperationEvidence::Created {
                            model_id,
                            manifest_digest: digest.to_owned(),
                        }),
                        _ => Err("MODEL_INSTALL_RECEIPT_INVALID".into()),
                    }
                }
                state => Err(format!(
                    "MODEL_INSTALL_EVIDENCE_PENDING: companion reports {state:?}"
                )),
            },
        }
    }
}

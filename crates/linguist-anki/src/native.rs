//! Native companion declarations are evidence to inspect, never client-side write authorization.
use super::*;
use uuid::Uuid;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationVariant {
    InstallModel,
    ExportCheckpoint,
    StoreMedia,
    CreateNote,
    UpdateNote,
    RestoreNote,
    DeleteUnstudiedCreatedNote,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeIntegration {
    pub anki_version: String,
    pub anki_connect_source_digest: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSession {
    pub lineage_id: Uuid,
    pub session_epoch: Uuid,
    pub profile_fingerprint: String,
    pub path_fingerprint: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeManifest {
    pub protocol: String,
    pub companion_version: String,
    pub bridge_id: Uuid,
    pub integration: NativeIntegration,
    pub collection_session: Option<NativeSession>,
    pub actions: Vec<String>,
    pub mutation_variants: Vec<MutationVariant>,
    pub api_key_configured: bool,
}
#[derive(Debug, Serialize)]
pub struct NativeInspection {
    pub declaration: NativeManifest,
    pub manifest_digest: String,
    pub compatibility_verified: bool,
    pub collection_identity_verified: bool,
    pub collection_writes_enabled: bool,
}
/// Untrusted companion ledger evidence. This is not a native-effect receipt.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeOperationState {
    Queued,
    Running,
    Unknown,
    FailedBeforeWrite,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeOperationStatus {
    pub lineage_id: Uuid,
    pub operation_id: Uuid,
    pub payload_digest: String,
    pub approved_digest: String,
    pub session_epoch: Uuid,
    pub variant: MutationVariant,
    pub state: NativeOperationState,
    pub reason: Option<String>,
    pub event_digest: String,
    pub needs_recovery: bool,
    pub dispatch_newly_authorized: bool,
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn plan_digest(value: &str) -> bool {
    value.strip_prefix("lab-jcs-v1:plan:").is_some_and(digest)
}
fn label(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
pub fn inspect_native_manifest(value: Value) -> Result<NativeInspection> {
    let declaration: NativeManifest =
        serde_json::from_value(value).map_err(|_| "ANKI_NATIVE_MANIFEST_INVALID")?;
    if declaration.protocol != "lab-native-v1" {
        return Err("CAPABILITY_UNAVAILABLE: unsupported native companion protocol".into());
    }
    if declaration.bridge_id.is_nil()
        || !label(&declaration.companion_version, 64)
        || !label(&declaration.integration.anki_version, 128)
        || !digest(&declaration.integration.anki_connect_source_digest)
    {
        return Err("ANKI_NATIVE_MANIFEST_INVALID".into());
    }
    let known = [
        "labCapabilities",
        "labBegin",
        "labInspect",
        "labMutate",
        "labOperationStatus",
        "labRebind",
        "labEnd",
    ];
    let unique: std::collections::BTreeSet<_> = declaration.actions.iter().collect();
    if unique.len() != declaration.actions.len()
        || !declaration.actions.iter().any(|a| a == "labCapabilities")
        || declaration
            .actions
            .iter()
            .any(|a| !known.contains(&a.as_str()))
    {
        return Err("ANKI_NATIVE_ACTIONS_INVALID".into());
    }
    if declaration.mutation_variants.len() > 7
        || declaration
            .mutation_variants
            .iter()
            .enumerate()
            .any(|(index, variant)| declaration.mutation_variants[..index].contains(variant))
    {
        return Err("ANKI_NATIVE_VARIANTS_INVALID".into());
    }
    if !declaration.mutation_variants.is_empty()
        && (!known
            .iter()
            .all(|a| declaration.actions.iter().any(|v| v == a))
            || !declaration.api_key_configured
            || declaration.collection_session.is_none())
    {
        return Err("ANKI_NATIVE_MUTATION_DECLARATION_INVALID".into());
    }
    if declaration
        .collection_session
        .as_ref()
        .is_some_and(|session| {
            session.lineage_id.is_nil()
                || session.session_epoch.is_nil()
                || !digest(&session.profile_fingerprint)
                || !digest(&session.path_fingerprint)
        })
    {
        return Err("ANKI_NATIVE_SESSION_INVALID".into());
    }
    let manifest_digest = canonical::digest("lab-native-capabilities-v1", &declaration)
        .map_err(|_| "ANKI_NATIVE_MANIFEST_INVALID")?;
    Ok(NativeInspection {
        declaration,
        manifest_digest,
        compatibility_verified: false,
        collection_identity_verified: false,
        collection_writes_enabled: false,
    })
}
pub fn inspect_native_operation_status(
    value: Value,
    lineage_id: Uuid,
    operation_id: Uuid,
) -> Result<NativeOperationStatus> {
    let status: NativeOperationStatus =
        serde_json::from_value(value).map_err(|_| "ANKI_NATIVE_STATUS_INVALID")?;
    let valid_reason = match status.state {
        NativeOperationState::Queued | NativeOperationState::Running => status.reason.is_none(),
        NativeOperationState::Unknown => matches!(
            status.reason.as_deref(),
            Some("transport_ambiguous" | "worker_crash" | "native_observation_incomplete")
        ),
        NativeOperationState::FailedBeforeWrite => matches!(
            status.reason.as_deref(),
            Some(
                "preflight_rejected"
                    | "operator_cancelled"
                    | "session_changed"
                    | "checkpoint_failed"
            )
        ),
    };
    if lineage_id.is_nil()
        || operation_id.is_nil()
        || status.lineage_id != lineage_id
        || status.operation_id != operation_id
        || status.session_epoch.is_nil()
        || !digest(&status.payload_digest)
        || !plan_digest(&status.approved_digest)
        || !digest(&status.event_digest)
        || !valid_reason
        || status.needs_recovery
            != matches!(
                status.state,
                NativeOperationState::Running | NativeOperationState::Unknown
            )
        || status.dispatch_newly_authorized
    {
        return Err("ANKI_NATIVE_STATUS_INVALID".into());
    }
    Ok(status)
}
impl Client {
    /// Profile-pinned read only. A declaration cannot bypass the compatibility/disposable-test gate.
    pub fn native_capabilities(&self) -> Result<NativeInspection> {
        self.check_profile()?;
        let value = self.call(Action::NativeCapabilities, json!({}))?;
        self.check_profile()?;
        inspect_native_manifest(value)
    }

    /// Profile-pinned observation only; callers must reconcile it with the CLI journal.
    pub fn native_operation_status(
        &self,
        lineage_id: Uuid,
        operation_id: Uuid,
    ) -> Result<NativeOperationStatus> {
        if lineage_id.is_nil() || operation_id.is_nil() {
            return Err("ANKI_NATIVE_STATUS_ID_INVALID".into());
        }
        self.check_profile()?;
        let value = self.call(
            Action::NativeOperationStatus,
            json!({"lineage_id":lineage_id,"operation_id":operation_id}),
        )?;
        self.check_profile()?;
        inspect_native_operation_status(value, lineage_id, operation_id)
    }
}

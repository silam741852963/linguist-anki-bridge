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
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
impl Client {
    /// Profile-pinned read only. A declaration cannot bypass the compatibility/disposable-test gate.
    pub fn native_capabilities(&self) -> Result<NativeInspection> {
        self.check_profile()?;
        let value = self.call(Action::NativeCapabilities, json!({}))?;
        self.check_profile()?;
        inspect_native_manifest(value)
    }
}

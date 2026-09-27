//! Read-only AnkiConnect port. No public arbitrary-action or mutation interface.
use linguist_config::Effective;
use linguist_core::canonical;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
};
pub type Result<T> = std::result::Result<T, String>;
mod capture;
pub use capture::ReadCapture;
#[derive(Clone, Copy, Debug)]
enum Action {
    Version,
    Reflect,
    Profile,
    Decks,
    Models,
    Fields,
    Templates,
    Styling,
    FindNotes,
    FindCards,
    NotesInfo,
    CardsInfo,
    RetrieveMedia,
}
impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Reflect => "apiReflect",
            Self::Profile => "getActiveProfile",
            Self::Decks => "deckNamesAndIds",
            Self::Models => "modelNamesAndIds",
            Self::Fields => "modelFieldNames",
            Self::Templates => "modelTemplates",
            Self::Styling => "modelStyling",
            Self::FindNotes => "findNotes",
            Self::FindCards => "findCards",
            Self::NotesInfo => "notesInfo",
            Self::CardsInfo => "cardsInfo",
            Self::RetrieveMedia => "retrieveMediaFile",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    result: Value,
    error: Value,
}
pub struct Client {
    http: reqwest::blocking::Client,
    endpoint: url::Url,
    key: Option<String>,
    limit: u64,
    batch: usize,
    media_limit: u64,
    expected_profile: Option<String>,
    observed_profile: std::sync::Mutex<Option<String>>,
}
#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub api_version: u32,
    pub profile: String,
    pub available_actions: Vec<String>,
    pub read_ready: bool,
    pub native_advertised: bool,
    pub native_verified: bool,
    pub collection_writes_enabled: bool,
    pub identity_confidence: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NamedId {
    pub id: String,
    pub name: String,
}
#[derive(Debug, Serialize)]
pub struct ModelInspection {
    pub model: NamedId,
    pub fields: Vec<String>,
    pub templates: BTreeMap<String, Value>,
    pub css: String,
    pub template_order_verified: bool,
    pub managed_verified: bool,
    pub content_matches_managed: bool,
    pub compatibility: Vec<linguist_core::model::ModelComparison>,
}
/// Captured original bytes; callers must still validate decoded media type/content.
#[derive(Debug)]
pub struct MediaFile {
    pub filename: String,
    pub bytes: Vec<u8>,
    pub digest: String,
}
#[derive(Debug, Serialize)]
pub struct Counts {
    pub note_count: usize,
    pub card_count: usize,
}
fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_broadcast()
                && !v.is_unspecified()
                && !v.is_multicast()
                && o[0] != 0
                && o[0] < 240
                && !(o[0] == 100 && (64..=127).contains(&o[1]))
                && !(o[0] == 198 && (18..=19).contains(&o[1]))
                && !(o[0] == 192 && o[1] == 0)
                && !(o[0] == 198 && o[1] == 51 && o[2] == 100)
                && !(o[0] == 203 && o[1] == 0 && o[2] == 113)
        }
        IpAddr::V6(v) => v
            .to_ipv4_mapped()
            .map(|v| public_address(IpAddr::V4(v)))
            .unwrap_or_else(|| {
                let s = v.segments();
                s[0] & 0xe000 == 0x2000 && !(s[0] == 0x2001 && s[1] == 0xdb8)
            }),
    }
}
impl Client {
    pub fn from_settings(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let value = |key: &str| {
            settings
                .values
                .get(key)
                .ok_or_else(|| format!("MISSING_SETTING: {key}"))
        };
        let registry = linguist_config::Registry::builtin();
        for key in [
            "anki.endpoint",
            "anki.api_key_env",
            "anki.expected_profile",
            "anki.request_timeout_seconds",
            "anki.read_batch_size",
            "media.max_asset_mb",
            "network.offline",
            "network.proxy_env",
            "network.connect_timeout_seconds",
            "network.max_response_mb",
            "network.allowed_remote_service_hosts",
        ] {
            registry.validate_value(key, value(key)?)?;
        }
        let endpoint = url::Url::parse(
            value("anki.endpoint")?
                .as_str()
                .ok_or("INVALID_ANKI_ENDPOINT")?,
        )
        .map_err(|_| "INVALID_ANKI_ENDPOINT")?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
        {
            return Err("INVALID_ANKI_ENDPOINT".into());
        }
        if !value("network.proxy_env")?.is_null() {
            return Err(
                "CAPABILITY_UNAVAILABLE: policy-checked proxy transport is not implemented".into(),
            );
        }
        let host = endpoint.host_str().ok_or("INVALID_ANKI_ENDPOINT")?;
        let bare = host.trim_matches(['[', ']']);
        let port = endpoint
            .port_or_known_default()
            .ok_or("INVALID_ANKI_ENDPOINT")?;
        let loopback =
            bare == "localhost" || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
        if !loopback
            && (value("network.offline")? == &json!(true)
                || !value("network.allowed_remote_service_hosts")?
                    .as_array()
                    .ok_or("INVALID_HOST_POLICY")?
                    .iter()
                    .any(|h| h.as_str().is_some_and(|s| s.eq_ignore_ascii_case(host))))
        {
            return Err("REMOTE_ENDPOINT_NOT_ALLOWED".into());
        }
        let addresses: Vec<SocketAddr> = if bare == "localhost" {
            vec![
                SocketAddr::from(([127, 0, 0, 1], port)),
                SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
            ]
        } else if let Ok(ip) = bare.parse::<IpAddr>() {
            vec![SocketAddr::new(ip, port)]
        } else {
            let host = bare.to_owned();
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = (host.as_str(), port)
                    .to_socket_addrs()
                    .map(|a| a.take(65).collect::<Vec<_>>());
                let _ = sender.send(result);
            });
            receiver
                .recv_timeout(std::time::Duration::from_secs(
                    value("network.connect_timeout_seconds")?.as_u64().unwrap(),
                ))
                .map_err(|_| "ANKI_DNS_UNAVAILABLE")?
                .map_err(|_| "ANKI_DNS_UNAVAILABLE")?
        };
        if addresses.len() > 64
            || addresses.is_empty()
            || addresses.iter().any(|a| {
                if loopback {
                    !a.ip().is_loopback()
                } else {
                    !public_address(a.ip())
                }
            })
        {
            return Err("ENDPOINT_ADDRESS_POLICY_REJECTED".into());
        }
        let key = value("anki.api_key_env")?
            .as_str()
            .map(|name| {
                environment
                    .get(name)
                    .filter(|v| !v.trim().is_empty())
                    .cloned()
                    .ok_or("ANKI_CREDENTIAL_UNAVAILABLE")
            })
            .transpose()?;
        let http = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .resolve_to_addrs(bare, &addresses)
            .connect_timeout(std::time::Duration::from_secs(
                value("network.connect_timeout_seconds")?
                    .as_u64()
                    .ok_or("INVALID_CONNECT_TIMEOUT")?,
            ))
            .timeout(std::time::Duration::from_secs(
                value("anki.request_timeout_seconds")?
                    .as_u64()
                    .ok_or("INVALID_REQUEST_TIMEOUT")?,
            ))
            .build()
            .map_err(|_| "ANKI_TRANSPORT_UNAVAILABLE")?;
        Ok(Self {
            http,
            endpoint,
            key,
            limit: value("network.max_response_mb")?
                .as_u64()
                .ok_or("INVALID_RESPONSE_LIMIT")?
                * 1024
                * 1024,
            batch: value("anki.read_batch_size")?
                .as_u64()
                .ok_or("INVALID_BATCH_SIZE")? as usize,
            media_limit: value("media.max_asset_mb")?
                .as_u64()
                .ok_or("INVALID_MEDIA_LIMIT")?
                * 1024
                * 1024,
            expected_profile: value("anki.expected_profile")?.as_str().map(str::to_owned),
            observed_profile: std::sync::Mutex::new(None),
        })
    }
    fn call(&self, action: Action, params: Value) -> Result<Value> {
        let mut request = json!({"action":action.name(),"version":6,"params":params});
        if let Some(key) = &self.key {
            request["key"] = json!(key)
        }
        let response = self
            .http
            .post(self.endpoint.clone())
            .json(&request)
            .send()
            .map_err(|e| {
                if e.is_timeout() {
                    "ANKI_READ_TIMEOUT"
                } else {
                    "ANKI_DEPENDENCY_UNAVAILABLE"
                }
            })?;
        if !response.status().is_success() {
            return Err(format!("ANKI_HTTP_FAILURE: {}", response.status().as_u16()));
        }
        if response.content_length().is_some_and(|n| n > self.limit) {
            return Err("ANKI_RESPONSE_TOO_LARGE".into());
        }
        let mut bytes = Vec::new();
        response
            .take(self.limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "ANKI_RESPONSE_IO")?;
        if bytes.len() as u64 > self.limit {
            return Err("ANKI_RESPONSE_TOO_LARGE".into());
        }
        let envelope: Envelope = canonical::parse(&bytes)
            .map_err(|_| "ANKI_PROTOCOL_INVALID: malformed envelope or unsafe numbers")?;
        if !envelope.error.is_null() {
            return Err(format!("ANKI_ACTION_REJECTED: {}", action.name()));
        }
        Ok(envelope.result)
    }
    pub fn capabilities(&self) -> Result<Capabilities> {
        let version = self
            .call(Action::Version, json!({}))?
            .as_u64()
            .filter(|v| *v == 6)
            .ok_or("ANKI_PROTOCOL_UNSUPPORTED")? as u32;
        let profile = self
            .call(Action::Profile, json!({}))?
            .as_str()
            .ok_or("ANKI_PROFILE_INVALID")?
            .to_owned();
        if self
            .expected_profile
            .as_ref()
            .is_some_and(|p| p != &profile)
        {
            return Err("ANKI_PROFILE_CONFLICT".into());
        }
        {
            let mut pinned = self
                .observed_profile
                .lock()
                .map_err(|_| "ANKI_PROFILE_LOCK_FAILURE")?;
            if pinned.as_ref().is_some_and(|p| p != &profile) {
                return Err("ANKI_PROFILE_CONFLICT".into());
            }
            *pinned = Some(profile.clone());
        }
        let reflection = self.call(Action::Reflect, json!({"scopes":["actions"]}))?;
        let mut actions: Vec<String> = serde_json::from_value(
            reflection
                .get("actions")
                .cloned()
                .ok_or("ANKI_REFLECTION_INVALID")?,
        )
        .map_err(|_| "ANKI_REFLECTION_INVALID")?;
        actions.sort();
        actions.dedup();
        let read_ready = [
            "findNotes",
            "findCards",
            "notesInfo",
            "cardsInfo",
            "deckNamesAndIds",
            "modelFieldNames",
            "modelNamesAndIds",
            "modelTemplates",
            "modelStyling",
        ]
        .iter()
        .all(|a| actions.iter().any(|s| s == a));
        Ok(Capabilities {
            api_version: version,
            profile,
            read_ready,
            native_advertised: actions.iter().any(|a| a == "labCapabilities"),
            available_actions: actions,
            native_verified: false,
            collection_writes_enabled: false,
            identity_confidence: "weak".into(),
        })
    }
    pub fn check_profile(&self) -> Result<()> {
        let actual = self.call(Action::Profile, json!({}))?;
        let actual = actual.as_str().ok_or("ANKI_PROFILE_INVALID")?;
        if self.expected_profile.as_ref().is_some_and(|p| p != actual) {
            return Err("ANKI_PROFILE_CONFLICT".into());
        }
        let mut pinned = self
            .observed_profile
            .lock()
            .map_err(|_| "ANKI_PROFILE_LOCK_FAILURE")?;
        if pinned.as_ref().is_some_and(|p| p != actual) {
            return Err("ANKI_PROFILE_CONFLICT".into());
        }
        *pinned = Some(actual.to_owned());
        Ok(())
    }
    fn names(&self, action: Action) -> Result<Vec<NamedId>> {
        let map = self.call(action, json!({}))?;
        let entries = map.as_object().ok_or("ANKI_NAME_MAP_INVALID")?;
        let mut out = Vec::new();
        for (name, id) in entries {
            out.push(NamedId {
                id: wire_id(id)?,
                name: name.clone(),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(out)
    }
    pub fn decks(&self) -> Result<Vec<NamedId>> {
        self.check_profile()?;
        self.names(Action::Decks)
    }
    pub fn models(&self) -> Result<Vec<NamedId>> {
        self.check_profile()?;
        self.names(Action::Models)
    }
    pub fn inspect_model(&self, selector: &str) -> Result<ModelInspection> {
        let model = select_name(self.models()?, selector)?;
        let params = json!({"modelName":model.name});
        let fields: Vec<String> =
            serde_json::from_value(self.call(Action::Fields, params.clone())?)
                .map_err(|_| "ANKI_MODEL_FIELDS_INVALID")?;
        let templates: BTreeMap<String, Value> =
            serde_json::from_value(self.call(Action::Templates, params.clone())?)
                .map_err(|_| "ANKI_MODEL_TEMPLATES_INVALID")?;
        let styling = self.call(Action::Styling, params)?;
        let css = styling
            .get("css")
            .and_then(Value::as_str)
            .ok_or("ANKI_MODEL_CSS_INVALID")?
            .to_owned();
        let template_content = templates
            .iter()
            .map(|(name, value)| {
                let front = value["Front"]
                    .as_str()
                    .ok_or("ANKI_MODEL_TEMPLATES_INVALID")?;
                let back = value["Back"]
                    .as_str()
                    .ok_or("ANKI_MODEL_TEMPLATES_INVALID")?;
                Ok((name.clone(), (front.to_owned(), back.to_owned())))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        self.check_profile()?;
        let compatibility = [
            linguist_core::model::vocabulary(),
            linguist_core::model::grammar(),
        ]
        .iter()
        .map(|target| {
            linguist_core::model::compare(&model.name, &fields, &template_content, &css, target)
        })
        .collect::<Vec<_>>();
        let content_matches_managed = compatibility
            .iter()
            .any(|c| c.name_matches && c.exact_content_match);
        Ok(ModelInspection {
            model,
            fields,
            templates,
            css,
            template_order_verified: false,
            managed_verified: false,
            content_matches_managed,
            compatibility,
        })
    }
    pub fn find_notes(&self, query: &str) -> Result<Vec<String>> {
        self.check_profile()?;
        ids(self.call(Action::FindNotes, json!({"query":query}))?)
    }
    pub fn counts(&self, query: &str) -> Result<Counts> {
        let notes = self.find_notes(query)?;
        let cards = ids(self.call(Action::FindCards, json!({"query":query}))?)?;
        self.check_profile()?;
        Ok(Counts {
            note_count: notes.len(),
            card_count: cards.len(),
        })
    }
    /// Read only, with profile checks on both sides. False is the documented missing-file result.
    pub fn retrieve_media_file(&self, filename: &str) -> Result<Option<MediaFile>> {
        use base64::Engine;
        if !linguist_core::validation::safe_media_name(filename)
            || filename.trim().is_empty()
            || filename.ends_with([' ', '.'])
            || filename.contains(['*', '|'])
            || !unicode_normalization::is_nfc(filename)
        {
            return Err("ANKI_MEDIA_FILENAME_UNSAFE".into());
        }
        self.check_profile()?;
        let result = self.call(Action::RetrieveMedia, json!({"filename":filename}))?;
        self.check_profile()?;
        if result == Value::Bool(false) {
            return Ok(None);
        }
        let encoded = result.as_str().ok_or("ANKI_MEDIA_PROTOCOL_INVALID")?;
        if encoded.len() % 4 != 0 {
            return Err("ANKI_MEDIA_ENCODING_INVALID".into());
        }
        let padding = if encoded.ends_with("==") {
            2
        } else if encoded.ends_with('=') {
            1
        } else {
            0
        };
        let decoded_size = encoded
            .len()
            .checked_div(4)
            .and_then(|n| n.checked_mul(3))
            .and_then(|n| n.checked_sub(padding))
            .ok_or("ANKI_MEDIA_ENCODING_INVALID")?;
        if decoded_size as u64 > self.media_limit {
            return Err("ANKI_MEDIA_TOO_LARGE".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "ANKI_MEDIA_ENCODING_INVALID")?;
        if bytes.len() as u64 > self.media_limit {
            return Err("ANKI_MEDIA_TOO_LARGE".into());
        }
        Ok(Some(MediaFile {
            filename: filename.into(),
            digest: canonical::asset_digest(&bytes),
            bytes,
        }))
    }
    pub fn notes_info(&self, note_ids: &[String]) -> Result<Vec<Value>> {
        self.info(Action::NotesInfo, "notes", note_ids)
    }
    pub fn cards_info(&self, card_ids: &[String]) -> Result<Vec<Value>> {
        self.info(Action::CardsInfo, "cards", card_ids)
    }
    fn info(&self, action: Action, key: &str, ids: &[String]) -> Result<Vec<Value>> {
        for id in ids {
            numeric_id(id)?;
        }
        self.check_profile()?;
        if self.batch == 0 {
            return Err("INVALID_BATCH_SIZE".into());
        }
        let mut result = Vec::new();
        for chunk in ids.chunks(self.batch) {
            let numbers: Vec<_> = chunk
                .iter()
                .map(|id| numeric_id(id))
                .collect::<Result<_>>()?;
            let rows = self.call(action, json!({key:numbers}))?;
            let rows = rows.as_array().ok_or("ANKI_INFO_INVALID")?;
            if rows.len() != chunk.len() {
                return Err("ANKI_INFO_COUNT_CONFLICT".into());
            }
            let id_field = if matches!(action, Action::NotesInfo) {
                "noteId"
            } else {
                "cardId"
            };
            for (row, expected) in rows.iter().zip(chunk) {
                if !row.as_object().is_some_and(|v| v.is_empty())
                    && row.get(id_field).map(wire_id).transpose()?.as_ref() != Some(expected)
                {
                    return Err("ANKI_INFO_ID_CONFLICT".into());
                }
            }
            result.extend(rows.iter().cloned());
        }
        self.check_profile()?;
        Ok(result)
    }
}
pub fn wire_id(value: &Value) -> Result<String> {
    let id = match value {
        Value::String(s) => s.clone(),
        _ => value
            .as_u64()
            .filter(|n| *n > 0 && *n <= 9_007_199_254_740_991)
            .ok_or("ANKI_ID_INVALID")?
            .to_string(),
    };
    numeric_id(&id)?;
    Ok(id)
}
fn numeric_id(s: &str) -> Result<u64> {
    if s.starts_with('0') || s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err("ANKI_ID_INVALID".into());
    }
    s.parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && *n <= 9_007_199_254_740_991)
        .ok_or("ANKI_ID_UNREPRESENTABLE_ON_V6_WIRE".into())
}
fn ids(value: Value) -> Result<Vec<String>> {
    let mut ids: Vec<_> = value
        .as_array()
        .ok_or("ANKI_IDS_INVALID")?
        .iter()
        .map(wire_id)
        .collect::<Result<_>>()?;
    ids.sort_by_key(|id| id.parse::<u64>().unwrap());
    ids.dedup();
    Ok(ids)
}
pub fn select_name(entries: Vec<NamedId>, selector: &str) -> Result<NamedId> {
    let matches: Vec<_> = entries
        .into_iter()
        .filter(|e| e.name == selector || e.id == selector)
        .collect();
    if matches.len() != 1 {
        return Err("ANKI_NAME_MISSING_OR_AMBIGUOUS".into());
    }
    Ok(matches.into_iter().next().unwrap())
}
pub fn deck_query(name: &str) -> Result<String> {
    if name.chars().any(char::is_control) {
        return Err("INVALID_DECK_SELECTOR".into());
    }
    Ok(format!(
        "deck:\"{}\"",
        name.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('*', "\\*")
            .replace('_', "\\_")
    ))
}
/// Convert known wire ID positions before output or archival; preserve other values verbatim.
pub fn normalize_note_ids(mut value: Value) -> Result<Value> {
    for key in ["noteId", "modelId"] {
        if let Some(id) = value.get_mut(key) {
            *id = json!(wire_id(id)?);
        }
    }
    if let Some(cards) = value.get_mut("cards").and_then(Value::as_array_mut) {
        for id in cards {
            *id = json!(wire_id(id)?);
        }
    }
    Ok(value)
}

pub fn normalize_card_ids(mut value: Value) -> Result<Value> {
    for key in ["cardId", "note", "deckId", "originalDeckId"] {
        if let Some(id) = value.get_mut(key) {
            if id.as_u64() == Some(0) && key == "originalDeckId" {
                *id = json!("0");
            } else {
                *id = json!(wire_id(id)?);
            }
        }
    }
    Ok(value)
}

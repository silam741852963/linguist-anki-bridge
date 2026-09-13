//! Minimal asynchronous AnkiConnect transport.
//!
//! This crate deliberately starts with connectivity and permission only. Read
//! and write actions are added in later slices after their domain boundaries
//! have tests, so the desktop cannot mutate Anki by accident.

use std::{collections::BTreeSet, time::Duration};

use linguist_application::{
    CardTemplate, DeckName, ExactExpressionRequest, ExpressionResolution, MediaFile, ModelFields,
    ModelName, ModelTemplates, NoteInfo, resolve_exact_expression,
};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// AnkiConnect v6 envelope version used by the existing Python application.
pub const ANKI_CONNECT_VERSION: u8 = 6;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Granted,
    Denied,
}

/// Result returned by AnkiConnect's `requestPermission` action.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PermissionStatus {
    pub permission: Permission,
    #[serde(default, rename = "requireApiKey")]
    pub requires_api_key: Option<bool>,
    #[serde(default)]
    pub version: Option<u32>,
}

impl PermissionStatus {
    pub fn is_granted(&self) -> bool {
        self.permission == Permission::Granted
    }
}

#[derive(Clone, Debug)]
pub struct AnkiConnectTransport {
    client: Client,
    url: Url,
    timeout: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnkiConnectError {
    InvalidUrl(String),
    Transport { message: String, retryable: bool },
    Timeout { timeout: Duration },
    HttpStatus { status: u16, message: String },
    MalformedResponse { message: String },
    Remote { action: String, message: String },
}

impl std::fmt::Display for AnkiConnectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl(message) => write!(formatter, "invalid AnkiConnect URL: {message}"),
            Self::Transport { message, .. } => {
                write!(formatter, "AnkiConnect transport: {message}")
            }
            Self::Timeout { timeout } => {
                write!(formatter, "AnkiConnect timed out after {timeout:?}")
            }
            Self::HttpStatus { status, message } => {
                write!(formatter, "AnkiConnect returned HTTP {status}: {message}")
            }
            Self::MalformedResponse { message } => {
                write!(formatter, "malformed AnkiConnect response: {message}")
            }
            Self::Remote { action, message } => {
                write!(formatter, "AnkiConnect {action}: {message}")
            }
        }
    }
}

impl std::error::Error for AnkiConnectError {}

impl AnkiConnectTransport {
    pub fn new(url: &str) -> Result<Self, AnkiConnectError> {
        Self::with_timeout(url, DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(url: &str, timeout: Duration) -> Result<Self, AnkiConnectError> {
        let url =
            Url::parse(url).map_err(|error| AnkiConnectError::InvalidUrl(error.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(AnkiConnectError::InvalidUrl(
                "URL must have an http(s) scheme and host".into(),
            ));
        }
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| AnkiConnectError::Transport {
                message: error.to_string(),
                retryable: false,
            })?;
        Ok(Self {
            client,
            url,
            timeout,
        })
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Check the local AnkiConnect API version without changing collection data.
    pub async fn version(&self) -> Result<u32, AnkiConnectError> {
        self.send("version", Value::Null).await
    }

    /// Ask AnkiConnect to grant this local application permission.
    pub async fn request_permission(&self) -> Result<PermissionStatus, AnkiConnectError> {
        self.send("requestPermission", Value::Null).await
    }

    /// Return collection deck names without loading their notes.
    pub async fn deck_names(&self) -> Result<Vec<DeckName>, AnkiConnectError> {
        self.send::<Vec<String>>("deckNames", Value::Null)
            .await
            .map(|names| names.into_iter().map(DeckName).collect())
    }

    /// Return model names without loading model definitions.
    pub async fn model_names(&self) -> Result<Vec<ModelName>, AnkiConnectError> {
        self.send::<Vec<String>>("modelNames", Value::Null)
            .await
            .map(|names| names.into_iter().map(ModelName).collect())
    }

    pub async fn model_fields(
        &self,
        model_name: &ModelName,
    ) -> Result<ModelFields, AnkiConnectError> {
        let fields = self
            .send("modelFieldNames", json!({"modelName": model_name.0}))
            .await?;
        Ok(ModelFields {
            model_name: model_name.clone(),
            fields,
        })
    }

    pub async fn model_templates(
        &self,
        model_name: &ModelName,
    ) -> Result<ModelTemplates, AnkiConnectError> {
        let templates: serde_json::Map<String, Value> = self
            .send("modelTemplates", json!({"modelName": model_name.0}))
            .await?;
        let mut converted = Vec::with_capacity(templates.len());
        for (name, template) in templates {
            let object =
                template
                    .as_object()
                    .ok_or_else(|| AnkiConnectError::MalformedResponse {
                        message: format!("template {name:?} is not an object"),
                    })?;
            let front = required_string(object, "Front", "modelTemplates")?;
            let back = required_string(object, "Back", "modelTemplates")?;
            converted.push(CardTemplate { name, front, back });
        }
        Ok(ModelTemplates {
            model_name: model_name.clone(),
            templates: converted,
        })
    }

    /// Search Anki's indexed collection without embedding search policy here.
    pub async fn find_notes(&self, query: &str) -> Result<Vec<i64>, AnkiConnectError> {
        self.send("findNotes", json!({"query": query})).await
    }

    /// Ask Anki for indexed candidates, then defer exact comparison and mode
    /// selection to the application use case.
    pub async fn resolve_exact_expression(
        &self,
        deck_name: &str,
        request: &ExactExpressionRequest,
    ) -> Result<ExpressionResolution, AnkiConnectError> {
        let normalized = linguist_core::normalize_expression(&request.expression);
        if normalized.is_empty() {
            return Ok(resolve_exact_expression(request, []));
        }
        let query = format!(
            "deck:\"{}\" \"{}\"",
            escape_anki_query(deck_name),
            escape_anki_query(&normalized)
        );
        let note_ids = self.find_notes(&query).await?;
        let notes = self.notes_info(&note_ids).await?;
        Ok(resolve_exact_expression(request, notes))
    }

    /// Convert Anki's rich note-info response into the read-only application model.
    pub async fn notes_info(&self, note_ids: &[i64]) -> Result<Vec<NoteInfo>, AnkiConnectError> {
        let raw: Vec<RawNoteInfo> = self.send("notesInfo", json!({"notes": note_ids})).await?;
        Ok(raw.into_iter().map(RawNoteInfo::into_note).collect())
    }

    /// Retrieve a media payload. A missing Anki media file is represented as `None`.
    pub async fn retrieve_media_file(
        &self,
        filename: &str,
    ) -> Result<Option<MediaFile>, AnkiConnectError> {
        let data_base64: Option<String> = self
            .send("retrieveMediaFile", json!({"filename": filename}))
            .await?;
        Ok(data_base64.map(|data_base64| MediaFile {
            filename: filename.to_owned(),
            data_base64,
        }))
    }

    async fn send<T>(&self, action: &str, params: Value) -> Result<T, AnkiConnectError>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut request = json!({"action": action, "version": ANKI_CONNECT_VERSION});
        if !params.is_null() {
            request["params"] = params;
        }
        let response = self
            .client
            .post(self.url.clone())
            .json(&request)
            .send()
            .await
            .map_err(|error| self.transport_error(error))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| self.transport_error(error))?;
        if !status.is_success() {
            return Err(AnkiConnectError::HttpStatus {
                status: status.as_u16(),
                message: body,
            });
        }
        let envelope: Value =
            serde_json::from_str(&body).map_err(|error| AnkiConnectError::MalformedResponse {
                message: format!("response is not JSON: {error}"),
            })?;
        let object = envelope
            .as_object()
            .ok_or_else(|| AnkiConnectError::MalformedResponse {
                message: "response must be an object".into(),
            })?;
        let error = object
            .get("error")
            .ok_or_else(|| AnkiConnectError::MalformedResponse {
                message: "response is missing error".into(),
            })?;
        let result = object
            .get("result")
            .ok_or_else(|| AnkiConnectError::MalformedResponse {
                message: "response is missing result".into(),
            })?;
        if !error.is_null() {
            let message = error
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| error.to_string());
            return Err(AnkiConnectError::Remote {
                action: action.to_owned(),
                message,
            });
        }
        serde_json::from_value(result.clone()).map_err(|error| {
            AnkiConnectError::MalformedResponse {
                message: format!("invalid result for {action}: {error}"),
            }
        })
    }

    fn transport_error(&self, error: reqwest::Error) -> AnkiConnectError {
        if error.is_timeout() {
            return AnkiConnectError::Timeout {
                timeout: self.timeout,
            };
        }
        AnkiConnectError::Transport {
            message: error.to_string(),
            retryable: error.is_connect() || error.is_request(),
        }
    }
}

fn required_string(
    object: &serde_json::Map<String, Value>,
    field: &str,
    action: &str,
) -> Result<String, AnkiConnectError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| AnkiConnectError::MalformedResponse {
            message: format!("{action} result is missing string {field:?}"),
        })
}

fn escape_anki_query(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[derive(Deserialize)]
struct RawNoteInfo {
    #[serde(rename = "noteId")]
    note_id: i64,
    #[serde(rename = "modelName")]
    model_name: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    fields: std::collections::BTreeMap<String, RawField>,
    #[serde(default)]
    cards: Vec<RawCardInfo>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawField {
    Info { value: String },
    Value(String),
}

#[derive(Deserialize)]
struct RawCardInfo {
    #[serde(rename = "deckName")]
    deck_name: Option<String>,
}

impl RawNoteInfo {
    fn into_note(self) -> NoteInfo {
        let fields = self
            .fields
            .into_iter()
            .map(|(name, value)| {
                let value = match value {
                    RawField::Info { value } | RawField::Value(value) => value,
                };
                (name, value)
            })
            .collect();
        let deck_names = self
            .cards
            .into_iter()
            .filter_map(|card| card.deck_name)
            .map(DeckName)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        NoteInfo {
            note_id: self.note_id,
            model_name: ModelName(self.model_name),
            deck_names,
            fields,
            tags: self.tags,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    use super::*;

    async fn mock_server(
        status: u16,
        body: &'static str,
        delay: Duration,
    ) -> (String, oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let count = stream.read(&mut request).await.unwrap();
            request.truncate(count);
            let _ = request_tx.send(String::from_utf8_lossy(&request).into_owned());
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        (format!("http://{address}"), request_rx)
    }

    async fn mock_sequence(
        responses: Vec<&'static str>,
    ) -> (String, oneshot::Receiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();
        tokio::spawn(async move {
            let mut requests = Vec::with_capacity(responses.len());
            for body in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 4096];
                let count = stream.read(&mut request).await.unwrap();
                request.truncate(count);
                requests.push(String::from_utf8_lossy(&request).into_owned());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
            let _ = request_tx.send(requests);
        });
        (format!("http://{address}"), request_rx)
    }

    #[tokio::test]
    async fn sends_version_in_the_anki_connect_envelope() {
        let (url, request) =
            mock_server(200, r#"{"result": 6, "error": null}"#, Duration::ZERO).await;
        let transport = AnkiConnectTransport::new(&url).unwrap();
        assert_eq!(transport.version().await.unwrap(), 6);
        let request = request.await.unwrap();
        assert!(request.starts_with("POST / HTTP/1.1"));
        assert!(request.contains(r#"{"action":"version","version":6}"#));
    }

    #[tokio::test]
    async fn requests_permission_without_collection_writes() {
        let (url, request) = mock_server(
            200,
            r#"{"result":{"permission":"granted","requireApiKey":false,"version":6},"error":null}"#,
            Duration::ZERO,
        )
        .await;
        let transport = AnkiConnectTransport::new(&url).unwrap();
        assert_eq!(
            transport.request_permission().await.unwrap(),
            PermissionStatus {
                permission: Permission::Granted,
                requires_api_key: Some(false),
                version: Some(6),
            }
        );
        assert!(
            request
                .await
                .unwrap()
                .contains(r#"{"action":"requestPermission","version":6}"#)
        );
    }

    #[tokio::test]
    async fn exposes_anki_connect_errors_structurally() {
        let (url, _) = mock_server(
            200,
            r#"{"result": null, "error": "permission denied"}"#,
            Duration::ZERO,
        )
        .await;
        let error = AnkiConnectTransport::new(&url)
            .unwrap()
            .version()
            .await
            .unwrap_err();
        assert_eq!(
            error,
            AnkiConnectError::Remote {
                action: "version".into(),
                message: "permission denied".into(),
            }
        );
    }

    #[tokio::test]
    async fn times_out_without_requiring_anki() {
        let (url, _) = mock_server(
            200,
            r#"{"result": 6, "error": null}"#,
            Duration::from_millis(100),
        )
        .await;
        let transport = AnkiConnectTransport::with_timeout(&url, Duration::from_millis(5)).unwrap();
        assert_eq!(
            transport.version().await.unwrap_err(),
            AnkiConnectError::Timeout {
                timeout: Duration::from_millis(5),
            }
        );
    }

    #[tokio::test]
    async fn reports_unreachable_local_transport_structurally() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let error = AnkiConnectTransport::new(&format!("http://{address}"))
            .unwrap()
            .version()
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            AnkiConnectError::Transport {
                retryable: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn rejects_malformed_response_envelopes() {
        let (url, _) = mock_server(200, r#"{"result": 6}"#, Duration::ZERO).await;
        let error = AnkiConnectTransport::new(&url)
            .unwrap()
            .version()
            .await
            .unwrap_err();
        assert!(matches!(error, AnkiConnectError::MalformedResponse { .. }));
    }

    #[tokio::test]
    async fn converts_empty_decks_and_missing_notes_at_the_adapter_boundary() {
        let (deck_url, _) = mock_server(200, r#"{"result":[],"error":null}"#, Duration::ZERO).await;
        assert!(
            AnkiConnectTransport::new(&deck_url)
                .unwrap()
                .deck_names()
                .await
                .unwrap()
                .is_empty()
        );

        let (search_url, request) =
            mock_server(200, r#"{"result":[],"error":null}"#, Duration::ZERO).await;
        let transport = AnkiConnectTransport::new(&search_url).unwrap();
        assert!(
            transport
                .find_notes("deck:Japanese missing")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(request.await.unwrap().contains(r#"{"action":"findNotes"#));

        let (notes_url, _) =
            mock_server(200, r#"{"result":[],"error":null}"#, Duration::ZERO).await;
        assert!(
            AnkiConnectTransport::new(&notes_url)
                .unwrap()
                .notes_info(&[404])
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn preserves_html_fields_tags_and_note_decks() {
        let response = r#"{
          "result":[{
            "noteId":42,"modelName":"Japanese","tags":["source","needs review"],
            "fields":{"Expression":{"order":0,"value":"<b>俳優</b>"},"Meaning":{"order":1,"value":"actor<br>performer"}},
            "cards":[{"deckName":"Japanese::Media"},{"deckName":"Japanese::Media"}]
          }],"error":null
        }"#;
        let (url, _) = mock_server(200, response, Duration::ZERO).await;
        let note = AnkiConnectTransport::new(&url)
            .unwrap()
            .notes_info(&[42])
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(note.note_id, 42);
        assert_eq!(note.model_name, ModelName("Japanese".into()));
        assert_eq!(note.fields["Expression"], "<b>俳優</b>");
        assert_eq!(note.fields["Meaning"], "actor<br>performer");
        assert_eq!(note.tags, ["source", "needs review"]);
        assert_eq!(note.deck_names, [DeckName("Japanese::Media".into())]);
    }

    #[tokio::test]
    async fn converts_multiple_models_fields_templates_and_media() {
        let (models_url, _) = mock_server(
            200,
            r#"{"result":["Basic","Japanese"],"error":null}"#,
            Duration::ZERO,
        )
        .await;
        assert_eq!(
            AnkiConnectTransport::new(&models_url)
                .unwrap()
                .model_names()
                .await
                .unwrap(),
            [ModelName("Basic".into()), ModelName("Japanese".into())]
        );

        let (fields_url, _) = mock_server(
            200,
            r#"{"result":["Expression","Meaning"],"error":null}"#,
            Duration::ZERO,
        )
        .await;
        let model = ModelName("Japanese".into());
        assert_eq!(
            AnkiConnectTransport::new(&fields_url)
                .unwrap()
                .model_fields(&model)
                .await
                .unwrap()
                .fields,
            ["Expression", "Meaning"]
        );

        let (templates_url, _) = mock_server(
            200,
            r#"{"result":{"Recognition":{"Front":"{{Expression}}","Back":"{{FrontSide}}<hr>{{Meaning}}"}},"error":null}"#,
            Duration::ZERO,
        )
        .await;
        assert_eq!(
            AnkiConnectTransport::new(&templates_url)
                .unwrap()
                .model_templates(&model)
                .await
                .unwrap()
                .templates,
            [CardTemplate {
                name: "Recognition".into(),
                front: "{{Expression}}".into(),
                back: "{{FrontSide}}<hr>{{Meaning}}".into(),
            }]
        );

        let (media_url, _) =
            mock_server(200, r#"{"result":null,"error":null}"#, Duration::ZERO).await;
        assert_eq!(
            AnkiConnectTransport::new(&media_url)
                .unwrap()
                .retrieve_media_file("missing.mp3")
                .await
                .unwrap(),
            None
        );

        let (media_url, _) = mock_server(
            200,
            r#"{"result":"base64-audio","error":null}"#,
            Duration::ZERO,
        )
        .await;
        assert_eq!(
            AnkiConnectTransport::new(&media_url)
                .unwrap()
                .retrieve_media_file("audio.mp3")
                .await
                .unwrap(),
            Some(MediaFile {
                filename: "audio.mp3".into(),
                data_base64: "base64-audio".into(),
            })
        );
    }

    #[tokio::test]
    async fn adapter_uses_indexed_candidates_then_application_resolution() {
        let (url, requests) = mock_sequence(vec![
            r#"{"result":[42],"error":null}"#,
            r#"{"result":[{"noteId":42,"modelName":"Legacy","fields":{"Word":{"value":"<b>俳優</b>"}},"tags":[],"cards":[]}],"error":null}"#,
        ])
        .await;
        let resolution = AnkiConnectTransport::new(&url)
            .unwrap()
            .resolve_exact_expression(
                "Japanese::Vocabulary",
                &ExactExpressionRequest {
                    deck_key: "japanese_vocab".into(),
                    expression: " 俳優 ".into(),
                    preferred_fields: vec!["Word".into()],
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            resolution,
            ExpressionResolution::Modernize { note, .. } if note.note_id == 42
        ));
        let requests = requests.await.unwrap();
        assert!(requests[0].contains(r#""action":"findNotes"#));
        assert!(requests[0].contains(r#"deck:\"Japanese::Vocabulary\" \"俳優\""#));
        assert!(requests[1].contains(r#""action":"notesInfo"#));
    }
}

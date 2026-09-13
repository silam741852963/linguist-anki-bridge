//! Minimal asynchronous AnkiConnect transport.
//!
//! This crate deliberately starts with connectivity and permission only. Read
//! and write actions are added in later slices after their domain boundaries
//! have tests, so the desktop cannot mutate Anki by accident.

use std::time::Duration;

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
}

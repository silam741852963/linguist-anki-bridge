//! Fixed-origin dictionary reads through the shared provider boundary.
use crate::{JishoPage, parse_jisho};
use linguist_config::Effective;
use linguist_core::Language;
pub use linguist_provider::ReadError;
use linguist_provider::{Reader, Service};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Provider {
    Jisho,
    Wiktionary,
    Cambridge,
}

#[derive(Clone)]
pub struct JishoClient {
    provider: Provider,
    reader: Reader,
    endpoint: url::Url,
    limit: u64,
}

impl JishoClient {
    pub fn from_settings(settings: &Effective) -> Result<Self, ReadError> {
        Self::for_target(
            settings,
            &"ja".to_owned().try_into().map_err(|_| ReadError::Policy)?,
        )
    }
    /// Frozen plan settings carry absolute storage paths, so no environment is read.
    pub fn for_target(settings: &Effective, target: &Language) -> Result<Self, ReadError> {
        Self::for_target_in(settings, &BTreeMap::new(), target)
    }
    pub fn for_target_in(
        settings: &Effective,
        environment: &BTreeMap<String, String>,
        target: &Language,
    ) -> Result<Self, ReadError> {
        let provider = match (
            settings
                .values
                .get("dictionary.provider")
                .and_then(serde_json::Value::as_str),
            target.as_str().split('-').next(),
        ) {
            (Some("auto" | "jisho"), Some("ja")) => Provider::Jisho,
            (Some("auto" | "wiktionary"), Some("en")) => Provider::Wiktionary,
            (Some("cambridge"), Some("en")) => Provider::Cambridge,
            _ => return Err(ReadError::Unavailable),
        };
        for key in ["dictionary.provider", "dictionary.browser_fallback"] {
            linguist_config::Registry::builtin()
                .validate_value(key, settings.values.get(key).ok_or(ReadError::Policy)?)
                .map_err(|_| ReadError::Policy)?;
        }
        // The browser helper is not available in this build.
        if settings.values["dictionary.browser_fallback"] == true {
            return Err(ReadError::Unavailable);
        }
        let (host, endpoint) = match provider {
            Provider::Jisho => ("jisho.org", "https://jisho.org/api/v1/search/words"),
            Provider::Wiktionary => (
                "en.wiktionary.org",
                "https://en.wiktionary.org/api/rest_v1/page/definition/",
            ),
            Provider::Cambridge => ("dictionary.cambridge.org", crate::cambridge::PAGE),
        };
        let reader =
            Reader::from_settings(settings, environment, Service::Dictionary, &[host], &[])?;
        Ok(Self {
            provider,
            reader,
            endpoint: url::Url::parse(endpoint).map_err(|_| ReadError::Policy)?,
            limit: settings.values["network.max_response_mb"].as_u64().unwrap() * 1024 * 1024,
        })
    }
    fn parse(
        &self,
        query: &str,
        target: &Language,
        bytes: &[u8],
        limit: usize,
    ) -> Result<JishoPage, ReadError> {
        match self.provider {
            Provider::Jisho => parse_jisho(query, target, bytes, self.limit, limit),
            Provider::Wiktionary => {
                crate::wiktionary::parse_definition(query, target, bytes, self.limit, limit)
            }
            Provider::Cambridge => {
                crate::cambridge::parse_page(query, target, bytes, self.limit, limit)
            }
        }
        .map_err(|_| ReadError::Schema)
    }
    /// One lookup shares the reader's deadline, pacing, retries and cache.
    pub fn lookup(
        &self,
        query: &str,
        target: &Language,
        max_entries: usize,
    ) -> Result<JishoPage, ReadError> {
        // Reject unsupported targets before any traffic.
        self.parse(
            query,
            target,
            match self.provider {
                Provider::Jisho => b"{\"meta\":{\"status\":200},\"data\":[]}".as_slice(),
                Provider::Wiktionary => b"{}".as_slice(),
                Provider::Cambridge => b"<html></html>".as_slice(),
            },
            max_entries,
        )?;
        let mut url = self.endpoint.clone();
        match self.provider {
            Provider::Jisho => {
                url.query_pairs_mut().append_pair("keyword", query);
            }
            Provider::Wiktionary => {
                url.path_segments_mut()
                    .map_err(|_| ReadError::Policy)?
                    .pop_if_empty()
                    .push(query);
            }
            Provider::Cambridge => {
                url = crate::cambridge::request_url(query).map_err(|_| ReadError::Policy)?;
            }
        }
        let accept: &[&str] = if self.provider == Provider::Cambridge {
            &["text/html"]
        } else {
            &["application/json"]
        };
        let fetched = self.reader.get(&url, accept)?;
        // Cambridge redirects unknown words to its home page: not found.
        let path = fetched.final_url.path();
        if self.provider == Provider::Cambridge
            && (!path.starts_with("/dictionary/english/") || path == "/dictionary/english/")
        {
            return self.parse(query, target, b"<html></html>", max_entries);
        }
        self.parse(query, target, &fetched.bytes, max_entries)
    }
}

/// Both adapters use the same fixed-origin request and retry policy.
pub type DictionaryClient = JishoClient;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, time::Duration};
    fn fixture(
        provider: Provider,
        replies: Vec<String>,
    ) -> (JishoClient, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = url::Url::parse(&format!(
            "http://{}/api/v1/search/words",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                requests.push(String::from_utf8(request).unwrap());
                use std::io::Write;
                stream.write_all(reply.as_bytes()).unwrap();
            }
            requests
        });
        let mut destinations = linguist_provider::Destinations::default();
        destinations.configured.insert("127.0.0.1".into());
        let limits = linguist_provider::Limits {
            attempts: 3,
            timeout: Duration::from_secs(2),
            connect_timeout: Duration::from_secs(2),
            interval: Duration::ZERO,
            concurrency: 1,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(2),
            jitter: 0.0,
            max_body: 1024,
            max_redirects: 2,
        };
        let reader = Reader::new(Service::Dictionary, destinations, limits, "test", None)
            .unwrap()
            .with_gates(Box::leak(Box::default()));
        let client = JishoClient {
            provider,
            reader,
            endpoint,
            limit: 1024,
        };
        (client, handle)
    }
    fn response(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",
            body.len()
        )
    }
    fn ja() -> Language {
        "ja".to_owned().try_into().unwrap()
    }
    #[test]
    fn jisho_queries_retry_transient_faults_and_reject_bad_schema() {
        let good = response(
            "200 OK",
            "Content-Type: application/json\r\n",
            r#"{"meta":{"status":200},"data":[]}"#,
        );
        let (client, server) = fixture(
            Provider::Jisho,
            vec![
                response("503 Service Unavailable", "Retry-After: 0\r\n", ""),
                good,
            ],
        );
        assert!(
            client
                .lookup("食べる", &ja(), 10)
                .unwrap()
                .entries
                .is_empty()
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /api/v1/search/words?keyword="));
        let (client, server) = fixture(
            Provider::Jisho,
            vec![response(
                "200 OK",
                "Content-Type: application/json\r\n",
                "invalid private content",
            )],
        );
        assert_eq!(
            client.lookup("食べる", &ja(), 10).unwrap_err(),
            ReadError::Schema
        );
        assert_eq!(server.join().unwrap().len(), 1);
    }
    #[test]
    fn english_transport_encodes_titles_and_uses_english_parser() {
        let body = r#"{"en":[{"language":"English","partOfSpeech":"Verb","definitions":[{"definition":"To eat"}]}]}"#;
        let (client, server) = fixture(
            Provider::Wiktionary,
            vec![response(
                "200 OK",
                "Content-Type: application/json\r\n",
                body,
            )],
        );
        let page = client
            .lookup("eat/word", &"en".to_owned().try_into().unwrap(), 10)
            .unwrap();
        assert_eq!(page.entries[0].senses[0].definitions, vec!["To eat"]);
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /api/v1/search/words/eat%2Fword "));
        assert!(!requests[0].contains("keyword="));
    }
    #[test]
    fn offline_browser_and_unsupported_targets_fail_before_traffic() {
        use linguist_config::*;
        let registry = Registry::builtin();
        let mut settings = resolve(
            &registry,
            &ConfigFile::default(),
            &ResolveOptions::default(),
        )
        .unwrap();
        settings.values.insert(
            "storage.cache_dir".into(),
            serde_json::json!(std::env::temp_dir().join("lab-dictionary-cache-unused")),
        );
        settings
            .values
            .insert("network.offline".into(), serde_json::json!(true));
        let offline = JishoClient::from_settings(&settings).unwrap();
        assert_eq!(
            offline.lookup("食べる", &ja(), 10).unwrap_err(),
            ReadError::Offline
        );
        settings.values.insert(
            "dictionary.browser_fallback".into(),
            serde_json::json!(true),
        );
        assert_eq!(
            JishoClient::from_settings(&settings).err(),
            Some(ReadError::Unavailable)
        );
        settings.values.insert(
            "dictionary.browser_fallback".into(),
            serde_json::json!(false),
        );
        for provider in ["custom", "authored", "wiktionary"] {
            settings
                .values
                .insert("dictionary.provider".into(), serde_json::json!(provider));
            assert_eq!(
                JishoClient::from_settings(&settings).err(),
                Some(ReadError::Unavailable)
            );
        }
    }
}

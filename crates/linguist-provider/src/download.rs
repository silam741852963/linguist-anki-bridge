//! Bounded streaming download for explicit resource installation (OP-58).
//!
//! Destinations and every redirect pass the same host policy as provider
//! reads. The body is streamed to the caller's file while hashing; the byte
//! limit and the overall deadline are enforced, and nothing is retried.
use crate::{Destinations, PolicyResolver, ReadError, is_policy_refusal};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Limits {
    pub connect_timeout: Duration,
    pub deadline: Duration,
    pub max_bytes: u64,
    pub max_redirects: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Downloaded {
    pub final_url: String,
    pub bytes: u64,
    pub sha256: String,
    pub content_type: Option<String>,
}

pub fn download(
    url: &url::Url,
    destinations: &Destinations,
    limits: &Limits,
    user_agent: &str,
    out: &mut dyn Write,
) -> Result<Downloaded, ReadError> {
    destinations.check(url)?;
    let deadline = Instant::now()
        .checked_add(limits.deadline)
        .ok_or(ReadError::Policy)?;
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(limits.connect_timeout)
        .timeout(limits.deadline)
        .dns_resolver(Arc::new(PolicyResolver {
            destinations: destinations.clone(),
            timeout: limits.connect_timeout,
        }))
        .user_agent(user_agent)
        .build()
        .map_err(|_| ReadError::Unavailable)?;
    let mut destination = url.clone();
    let mut redirects = 0;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(ReadError::Deadline)?;
        let mut response = match http.get(destination.clone()).timeout(remaining).send() {
            Ok(response) => response,
            Err(error) if error.is_timeout() => return Err(ReadError::Deadline),
            Err(error) if is_policy_refusal(&error) => return Err(ReadError::Policy),
            Err(_) => return Err(ReadError::Transport),
        };
        let status = response.status();
        if status.is_redirection() {
            if redirects >= limits.max_redirects {
                return Err(ReadError::Redirect);
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or(ReadError::Redirect)?;
            let next = destination
                .join(location)
                .map_err(|_| ReadError::Redirect)?;
            destinations.check(&next).map_err(|_| ReadError::Redirect)?;
            destination = next;
            redirects += 1;
            continue;
        }
        if !status.is_success() {
            return Err(ReadError::Http(status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > limits.max_bytes)
        {
            return Err(ReadError::ResponseLimit);
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|v| v.split(';').next().unwrap().trim().to_ascii_lowercase());
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            if Instant::now() >= deadline {
                return Err(ReadError::Deadline);
            }
            let read = match response.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) if Instant::now() >= deadline => return Err(ReadError::Deadline),
                Err(_) => return Err(ReadError::Transport),
            };
            total += read as u64;
            if total > limits.max_bytes {
                return Err(ReadError::ResponseLimit);
            }
            hasher.update(&buffer[..read]);
            out.write_all(&buffer[..read])
                .map_err(|_| ReadError::Transport)?;
        }
        let sha256 = format!("{:x}", hasher.finalize());
        return Ok(Downloaded {
            final_url: destination.to_string(),
            bytes: total,
            sha256,
            content_type,
        });
    }
}

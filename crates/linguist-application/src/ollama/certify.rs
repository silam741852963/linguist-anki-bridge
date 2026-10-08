//! RI-06 engine certification: probe one exact local engine identity for the
//! behaviour generation relies on, so a passed run can replace
//! `GENERATION_ENGINE_UNVERIFIED` for drafts from that same identity.
//!
//! Probes (all against the configured model and generation settings):
//! - input preservation: canary words at the start and end of a prompt
//!   filling most of the input budget come back exactly, and the reported
//!   prompt tokens match that size;
//! - overflow refusal: with `truncate=false` an input over `num_ctx` is
//!   refused, never silently cut;
//! - determinism: the same request with the configured seed and temperature
//!   returns the same bytes;
//! - output cap: `num_predict` ends a long answer with `done_reason=length`;
//! - no thinking: `think=false` responses carry no reasoning text;
//! - structured output: the `format` schema is obeyed.
use super::{ModelEvidence, transport::Client};
use linguist_core::canonical;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Request shape generation uses (`think=false`, `truncate=false`,
/// `shift=false`, nonstreaming chat with a JSON schema `format`).
pub const TRANSPORT_PROFILE: &str = "lab-ollama-chat-v1";

/// Everything a certification is bound to. Any change needs a new run.
pub fn identity(evidence: &ModelEvidence, settings: &linguist_config::Effective) -> Value {
    let v = &settings.values;
    json!({
        "profile": TRANSPORT_PROFILE,
        "endpoint": v["llm.endpoint"],
        "model": evidence.identity.name,
        "model_digest": evidence.identity.digest,
        "show_digest": evidence.show_digest,
        "architecture": evidence.architecture,
        "context_tokens": v["llm.context_tokens"],
        "max_output_tokens": v["llm.max_output_tokens"],
        "temperature": v["llm.temperature"],
        "seed": v["llm.seed"],
    })
}
pub fn identity_digest(identity: &Value) -> Result<String, String> {
    canonical::digest("lab-engine-identity-v1", identity).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct Probe {
    pub name: &'static str,
    pub passed: bool,
    pub detail: Value,
}
#[derive(Debug, Serialize)]
pub struct Certification {
    pub schema_version: u16,
    pub identity: Value,
    pub identity_digest: String,
    pub engine_version: String,
    pub passed: bool,
    pub probes: Vec<Probe>,
    /// Probe request/response bytes by SHA-256, for archival.
    #[serde(skip)]
    pub assets: BTreeMap<String, Vec<u8>>,
}

/// Deterministic pronounceable nonce words; no dictionary word by design.
fn words(seed: &[u8], count: usize) -> Vec<String> {
    const C: &[u8] = b"bdfgkmnprstvz";
    const V: &[u8] = b"aeiou";
    let mut state = seed.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100000001b3)
    });
    (0..count)
        .map(|_| {
            (0..3)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    format!(
                        "{}{}",
                        C[(state % C.len() as u64) as usize] as char,
                        V[((state >> 8) % V.len() as u64) as usize] as char
                    )
                })
                .collect()
        })
        .collect()
}

struct Reply {
    status: u16,
    value: Value,
}

impl Client {
    fn probe_body(&self, user: &str, format: Option<Value>, num_predict: u64) -> Value {
        let v = &self.settings().values;
        let mut body = json!({
            "model": v["llm.model"],
            "messages": [
                {"role":"system","content":"Follow the instruction exactly. Reply with the requested JSON only."},
                {"role":"user","content":user}
            ],
            "stream": false, "think": false, "truncate": false, "shift": false,
            "keep_alive": v["llm.keep_alive"],
            "options": {"num_ctx": v["llm.context_tokens"], "num_predict": num_predict,
                        "temperature": v["llm.temperature"], "seed": v["llm.seed"]}
        });
        if let Some(format) = format {
            body["format"] = format;
        }
        body
    }
    fn send(&self, body: &Value, assets: &mut BTreeMap<String, Vec<u8>>) -> Result<Reply, String> {
        let request = canonical::bytes(body).map_err(|_| "OLLAMA_REQUEST_INVALID")?;
        let (status, raw) = self.post_chat(body)?;
        assets.insert(canonical::asset_digest(&request), request);
        let value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
        assets.insert(canonical::asset_digest(&raw), raw);
        Ok(Reply { status, value })
    }

    /// Run every probe. The result is evidence either way; only `passed`
    /// certifications may clear the engine gate.
    pub fn certify(&self) -> Result<Certification, String> {
        let before = self.model_evidence()?;
        let engine_version = self.engine_version()?;
        let v = &self.settings().values;
        let context = v["llm.context_tokens"].as_u64().unwrap();
        let output = v["llm.max_output_tokens"].as_u64().unwrap();
        let budget = context - output;
        let mut assets = BTreeMap::new();
        let mut probes = Vec::new();
        let seed = uuid::Uuid::new_v4();
        let schema = json!({"type":"object","properties":{"first":{"type":"string"},"last":{"type":"string"}},
            "required":["first","last"],"additionalProperties":false});
        let canary = |lines: usize, salt: &str| {
            let pool = words(format!("{seed}{salt}").as_bytes(), lines + 2);
            let mut text = format!("FIRST-CANARY = {}\n", pool[0]);
            for (index, word) in pool[1..=lines].iter().enumerate() {
                text.push_str(&format!("filler {:05} {word}\n", index + 1));
            }
            text.push_str(&format!(
                "LAST-CANARY = {}\nReturn JSON with \"first\" set to the value of FIRST-CANARY and \"last\" set to the value of LAST-CANARY. Ignore the filler lines.\n",
                pool[lines + 1]
            ));
            (text, pool[0].clone(), pool[lines + 1].clone())
        };
        let thinking_free = |value: &Value| {
            value["message"]
                .get("thinking")
                .is_none_or(|t| t.as_str() == Some(""))
        };
        let mut no_thinking = true;
        // Calibrate tokens per canary line from two small, also checked, runs.
        let mut counts = Vec::new();
        for lines in [20usize, 60] {
            let (text, first, last) = canary(lines, &format!("cal{lines}"));
            let reply = self.send(
                &self.probe_body(&text, Some(schema.clone()), 64),
                &mut assets,
            )?;
            no_thinking &= thinking_free(&reply.value);
            let content: Value = reply.value["message"]["content"]
                .as_str()
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or(Value::Null);
            let tokens = reply.value["prompt_eval_count"].as_u64().unwrap_or(0);
            if reply.status != 200 || content != json!({"first":first,"last":last}) || tokens == 0 {
                probes.push(Probe {
                    name: "calibration",
                    passed: false,
                    detail: json!({"lines":lines,"status":reply.status,"prompt_tokens":tokens,"content":content}),
                });
                return self.finish(before, engine_version, probes, assets);
            }
            counts.push(tokens);
        }
        let per_line = (counts[1].saturating_sub(counts[0])) as f64 / 40.0;
        let base = counts[0] as f64 - 20.0 * per_line;
        if per_line <= 0.0 {
            probes.push(Probe {
                name: "calibration",
                passed: false,
                detail: json!({"counts":counts}),
            });
            return self.finish(before, engine_version, probes, assets);
        }
        probes.push(Probe {
            name: "calibration",
            passed: true,
            detail: json!({"prompt_tokens":counts,"tokens_per_line":per_line}),
        });
        // Input preservation near the full input budget.
        let lines = ((0.8 * budget as f64 - base) / per_line).floor().max(1.0) as usize;
        let (text, first, last) = canary(lines, "full");
        let body = self.probe_body(&text, Some(schema.clone()), 64);
        let full = self.send(&body, &mut assets)?;
        no_thinking &= thinking_free(&full.value);
        let tokens = full.value["prompt_eval_count"].as_u64().unwrap_or(0);
        let raw_content = full.value["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let content: Value = serde_json::from_str(&raw_content).unwrap_or(Value::Null);
        let preserved = full.status == 200
            && content == json!({"first":first,"last":last})
            && tokens as f64 >= 0.6 * budget as f64
            && tokens <= budget;
        probes.push(Probe {
            name: "input_preservation",
            passed: preserved,
            detail: json!({"lines":lines,"prompt_tokens":tokens,"budget":budget,"status":full.status,"content":content}),
        });
        let structured = full.status == 200
            && content.as_object().is_some_and(|o| o.len() == 2)
            && content["first"].is_string()
            && content["last"].is_string();
        probes.push(Probe {
            name: "structured_output",
            passed: structured,
            detail: json!({"content":raw_content}),
        });
        // Same request, same seed and temperature: same bytes.
        let again = self.send(&body, &mut assets)?;
        no_thinking &= thinking_free(&again.value);
        probes.push(Probe {
            name: "determinism",
            passed: again.status == 200
                && again.value["message"]["content"].as_str() == Some(raw_content.as_str())
                && again.value["prompt_eval_count"].as_u64() == Some(tokens),
            detail: json!({"first":raw_content,"second":again.value["message"]["content"]}),
        });
        // Over num_ctx with truncate=false must be refused, not cut.
        let over = ((1.5 * context as f64 - base) / per_line).ceil() as usize;
        let (text, _, _) = canary(over, "over");
        let refused = self.send(&self.probe_body(&text, Some(schema), 1), &mut assets)?;
        probes.push(Probe {
            name: "overflow_refused",
            passed: (400..500).contains(&refused.status),
            detail: json!({"lines":over,"status":refused.status,
                "prompt_tokens":refused.value["prompt_eval_count"]}),
        });
        // num_predict caps the answer.
        let capped = self.send(
            &self.probe_body(
                "Write every number from 1 to 300 separated by spaces.",
                None,
                8,
            ),
            &mut assets,
        )?;
        no_thinking &= thinking_free(&capped.value);
        probes.push(Probe {
            name: "output_cap",
            passed: capped.status == 200
                && capped.value["done_reason"] == "length"
                && capped.value["eval_count"].as_u64().is_some_and(|n| n <= 8),
            detail: json!({"done_reason":capped.value["done_reason"],"eval_count":capped.value["eval_count"]}),
        });
        probes.push(Probe {
            name: "no_thinking",
            passed: no_thinking,
            detail: json!({}),
        });
        self.finish(before, engine_version, probes, assets)
    }

    fn finish(
        &self,
        before: ModelEvidence,
        engine_version: String,
        mut probes: Vec<Probe>,
        mut assets: BTreeMap<String, Vec<u8>>,
    ) -> Result<Certification, String> {
        // The model and engine must not change during the run.
        let after = self.model_evidence()?;
        let version_after = self.engine_version()?;
        let stable = after.identity.digest == before.identity.digest
            && after.show_digest == before.show_digest
            && version_after == engine_version;
        probes.push(Probe {
            name: "identity_stable",
            passed: stable,
            detail: json!({"engine_version_after":version_after}),
        });
        assets.extend(before.assets.clone());
        let identity = identity(&before, self.settings());
        let required = [
            "calibration",
            "input_preservation",
            "structured_output",
            "determinism",
            "overflow_refused",
            "output_cap",
            "no_thinking",
            "identity_stable",
        ];
        let passed = required
            .iter()
            .all(|name| probes.iter().any(|p| p.name == *name && p.passed));
        Ok(Certification {
            schema_version: 1,
            identity_digest: identity_digest(&identity)?,
            identity,
            engine_version,
            passed,
            probes,
            assets,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nonce_words_are_deterministic_and_distinct() {
        let a = words(b"seed", 50);
        assert_eq!(a, words(b"seed", 50));
        assert_ne!(a, words(b"other", 50));
        assert!(a.iter().all(|w| w.len() == 6 && w.is_ascii()));
        let unique: std::collections::BTreeSet<_> = a.iter().collect();
        assert!(unique.len() > 45);
    }
}

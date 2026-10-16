//! WP-24 collection revamp campaigns.
//!
//! A campaign freezes a selection of existing notes into cohorts and batches
//! and drives every batch through the existing, individually crash-safe
//! commands: a prepare job, policy review and generation, approval, an apply
//! job with its own checkpoint, verification, and (for duplicates) the merge.
//! All state lives in the store (`linguist_store::campaign`); a tick reads it,
//! does at most one step per lane, and records the result with
//! compare-and-set, so a kill at any point loses no work and a fresh process
//! (or a fresh agent session) continues where the last one stopped.
pub mod engine;
pub mod select;
pub mod verify;

use crate::review::policy::Policy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u16 = 1;

/// Phases a batch moves through, in order.
pub const PHASES: [&str; 12] = [
    "selected",
    "preparing",
    "reviewing",
    "needs_agent",
    "ready",
    "approved",
    "applying",
    "verifying",
    "merging",
    "done",
    "skipped",
    "deferred",
];

/// `campaign create --spec FILE` (`contracts/v2/campaign-spec.schema.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema_version: u16,
    pub name: String,
    pub sources: Vec<SourceSpec>,
    /// Cohort IDs (`SOURCE:CLASS`) in processing order.
    pub order: Vec<String>,
    pub batch_size: u32,
    /// Batch size per source ID (for example 10 for grammar).
    #[serde(default)]
    pub batch_sizes: BTreeMap<String, u32>,
    #[serde(default)]
    pub pilot: Option<Pilot>,
    /// Merge groups the duplicate rule cannot see (other note types).
    #[serde(default)]
    pub merge_extra: Vec<MergeSpec>,
    #[serde(default)]
    pub limits: Limits,
    /// Decision policy; `None` is the builtin policy.
    #[serde(default)]
    pub policy: Option<Policy>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    pub id: String,
    /// Anki search selecting this source's notes.
    pub query: String,
    pub purpose: String,
    /// `--set KEY=VALUE` overrides every command of this source runs with.
    #[serde(default)]
    pub settings: Vec<String>,
    /// Target note types a verified note must be on after apply.
    pub target_models: Vec<String>,
    /// Source fields used by the cohort rules; absent fields disable a rule.
    #[serde(default)]
    pub word_field: Option<String>,
    #[serde(default)]
    pub reading_field: Option<String>,
    #[serde(default)]
    pub picture_field: Option<String>,
    #[serde(default)]
    pub audio_field: Option<String>,
    /// Target language tag (`ja`, `en`); drives the kana and split rules.
    pub language: String,
    /// Group same word+reading notes for a merge.
    #[serde(default)]
    pub merge_duplicates: bool,
    /// Issue codes this source leaves to the agent on top of the policy's
    /// (for example OCR text of notes whose words come from screenshots).
    #[serde(default)]
    pub extra_agent_codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pilot {
    pub cohort: String,
    pub size: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeSpec {
    pub source: String,
    pub notes: Vec<String>,
    /// The note that survives; default the one with the most repetitions.
    #[serde(default)]
    pub survivor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    /// A batch with a larger share of agent-decided items pauses the campaign.
    pub agent_share_max: f64,
    /// Agent runs the tick wrapper may start per calendar day (UTC).
    pub agent_runs_per_day: u32,
    /// Checkpoints of this many newest `done` batches are kept.
    pub retain_done_checkpoints: u32,
    /// Share of generated items the agent spot-checks before approval.
    pub spot_check_share: f64,
    /// `jobs run` invocations per prepare job before failed items are left out.
    pub max_prepare_runs: u32,
    /// Policy/generation rounds per review step.
    pub max_review_rounds: u32,
    /// Batches prepared ahead of the oldest unapplied one.
    pub prepare_ahead: u32,
    /// Batches one tick prepares and reviews at once (at most
    /// `prepare_ahead`, 1-8).
    pub prepare_parallel: u32,
    /// Failed attempts of one step before the campaign pauses.
    pub max_step_failures: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            agent_share_max: 0.2,
            agent_runs_per_day: 48,
            retain_done_checkpoints: 5,
            spot_check_share: 0.1,
            max_prepare_runs: 4,
            max_review_rounds: 4,
            prepare_ahead: 2,
            prepare_parallel: 1,
            max_step_failures: 3,
        }
    }
}

impl Spec {
    pub fn validate(&self) -> Result<(), String> {
        let bad = |m: &str| Err(format!("CAMPAIGN_SPEC_INVALID: {m}"));
        if self.schema_version != SCHEMA_VERSION {
            return bad("schema_version must be 1");
        }
        if self.name.trim().is_empty() || self.sources.is_empty() || self.order.is_empty() {
            return bad("name, sources and order are required");
        }
        if !(1..=1000).contains(&self.batch_size)
            || self.batch_sizes.values().any(|s| !(1..=1000).contains(s))
        {
            return bad("batch sizes are 1-1000");
        }
        let mut ids = std::collections::BTreeSet::new();
        for source in &self.sources {
            if source.id.is_empty() || source.id.contains(':') || !ids.insert(&source.id) {
                return bad("source IDs are unique and contain no ':'");
            }
            if !matches!(
                source.purpose.as_str(),
                "japanese_vocab" | "english_vocab" | "japanese_grammar" | "english_grammar"
            ) {
                return bad("unknown purpose");
            }
            if source.target_models.is_empty() || source.query.trim().is_empty() {
                return bad("every source needs a query and target models");
            }
            for setting in &source.settings {
                if !setting.contains('=') {
                    return bad("settings are KEY=VALUE");
                }
            }
        }
        for cohort in &self.order {
            let Some((source, class)) = cohort.split_once(':') else {
                return bad("cohorts are SOURCE:CLASS");
            };
            if !ids.contains(&source.to_owned()) || !select::CLASSES.contains(&class) {
                return bad(&format!("unknown cohort {cohort}"));
            }
        }
        if let Some(pilot) = &self.pilot
            && (!self.order.contains(&pilot.cohort) || pilot.size == 0)
        {
            return bad("the pilot cohort must be in order");
        }
        for merge in &self.merge_extra {
            if !ids.contains(&merge.source) || merge.notes.len() < 2 {
                return bad("merge_extra groups name a source and two or more notes");
            }
            if merge
                .survivor
                .as_ref()
                .is_some_and(|s| !merge.notes.contains(s))
            {
                return bad("a merge survivor is one of its group's notes");
            }
        }
        let l = &self.limits;
        if !(0.0..=1.0).contains(&l.agent_share_max)
            || !(0.0..=1.0).contains(&l.spot_check_share)
            || l.max_prepare_runs == 0
            || l.max_review_rounds == 0
            || l.prepare_ahead == 0
            || l.max_step_failures == 0
            || !(1..=8).contains(&l.prepare_parallel)
            || l.prepare_parallel > l.prepare_ahead
        {
            return bad("limits out of range");
        }
        if let Some(policy) = &self.policy {
            policy.validate()?;
        }
        Ok(())
    }
    pub fn source(&self, id: &str) -> Option<&SourceSpec> {
        self.sources.iter().find(|s| s.id == id)
    }
    pub fn policy(&self) -> Policy {
        self.policy.clone().unwrap_or_else(Policy::builtin)
    }
    /// The policy one source's reviews run with.
    pub fn source_policy(&self, source: &SourceSpec) -> Policy {
        let mut policy = self.policy();
        for code in &source.extra_agent_codes {
            if !policy.agent_codes.contains(code) {
                policy.agent_codes.push(code.clone());
            }
        }
        policy
    }
}

/// The frozen policy file of one source, next to the campaign's policy.
pub fn source_policy_path(policy_path: &str, source: &str) -> String {
    let path = std::path::Path::new(policy_path);
    path.with_file_name(format!("policy-{source}.json"))
        .to_string_lossy()
        .into_owned()
}

/// The working data of one batch (`campaign_batches.data`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchData {
    pub source: String,
    pub notes: Vec<String>,
    /// Duplicate merge: the survivor is `notes[0]`; these are merged into it.
    pub merge: Vec<String>,
    pub pilot: bool,
    /// Notes the cohort rules flagged as holding several words.
    pub split_candidates: Vec<String>,
    pub prepare_job: Option<uuid::Uuid>,
    pub prepare_runs: u32,
    /// Times failed reads (Anki busy or unreachable) were read again.
    pub read_retries: u32,
    pub read_retry_after_ms: Option<u64>,
    /// note ID -> (failed drafts, last error); after three the agent decides.
    pub draft_failures: BTreeMap<String, (u32, String)>,
    pub plan: Option<uuid::Uuid>,
    /// note ID -> plan document ID.
    pub documents: BTreeMap<String, uuid::Uuid>,
    /// note ID -> why it is left out of this batch.
    pub deferred: BTreeMap<String, String>,
    /// Notes the agent marked settled (split done or not needed).
    pub settled: Vec<String>,
    /// Open issues the policy left, per note.
    pub needs: BTreeMap<String, Vec<String>>,
    pub checked_revision: Option<u32>,
    pub review_rounds: u32,
    pub approval: Option<Approval>,
    /// Card facts read just before the apply job was created.
    pub before: BTreeMap<String, verify::NoteFacts>,
    pub apply_intent_ms: Option<u64>,
    pub apply_jobs: Vec<uuid::Uuid>,
    /// Apply jobs already run once (later invocations resume).
    pub started_jobs: Vec<uuid::Uuid>,
    pub finished_jobs: Vec<uuid::Uuid>,
    pub groups_done: Vec<uuid::Uuid>,
    pub committed: Vec<String>,
    pub merged: Vec<String>,
    pub step_failures: u32,
    pub last_error: Option<String>,
    pub verify_problems: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    pub revision: u32,
    pub digest: String,
    pub actor: String,
    /// Plan items the apply job takes (not deferred, not in a split group).
    pub items: Vec<uuid::Uuid>,
    /// Split groups applied one by one after the job.
    pub groups: Vec<uuid::Uuid>,
    /// Every approved note (job items and split anchors).
    pub notes: Vec<String>,
    pub agent_decided: Vec<String>,
}

impl BatchData {
    pub fn from_value(value: &serde_json::Value) -> Result<Self, String> {
        serde_json::from_value(value.clone()).map_err(|e| format!("CAMPAIGN_BATCH_CORRUPT: {e}"))
    }
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("batch data serializes")
    }
    /// Notes still part of the batch.
    pub fn active_notes(&self) -> impl Iterator<Item = &String> {
        self.notes
            .iter()
            .filter(|n| !self.deferred.contains_key(*n))
    }
}

/// The frozen campaign definition (`campaigns.definition`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema_version: u16,
    pub spec: Spec,
    pub created_ms: u64,
    /// Global CLI arguments every child command repeats (`--config`, `--profile`).
    pub global_args: Vec<String>,
    /// Path of the frozen policy file children read.
    pub policy_path: String,
    pub selection: select::SelectionReport,
}

/// An agent run that has not reported its end holds `needs_agent` batches
/// for at most this long (a killed agent session releases them then).
pub const AGENT_HOLD_MS: u64 = 45 * 60 * 1000;

/// Limits `campaign tune` may change: throughput only, never a guardrail
/// (agent share, spot checks, step failures).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tuning {
    pub prepare_ahead: Option<u32>,
    pub prepare_parallel: Option<u32>,
    pub agent_runs_per_day: Option<u32>,
}

impl Tuning {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
    /// Apply onto `limits`, refusing values outside the spec's ranges.
    pub fn apply(&self, limits: &mut Limits) -> Result<(), String> {
        if let Some(v) = self.prepare_ahead {
            limits.prepare_ahead = v;
        }
        if let Some(v) = self.prepare_parallel {
            limits.prepare_parallel = v;
        }
        if let Some(v) = self.agent_runs_per_day {
            limits.agent_runs_per_day = v;
        }
        if !(1..=8).contains(&limits.prepare_parallel)
            || !(1..=32).contains(&limits.prepare_ahead)
            || limits.prepare_parallel > limits.prepare_ahead
            || limits.agent_runs_per_day > 288
        {
            return Err("CAMPAIGN_TUNING_INVALID: prepare_parallel 1-8 and at most prepare_ahead (1-32); agent_runs_per_day at most 288".into());
        }
        Ok(())
    }
}

impl Definition {
    /// The frozen definition with the newest `campaign tune` applied.
    pub fn effective(store: &linguist_store::Store, id: uuid::Uuid) -> Result<Self, String> {
        let record = store.campaign(id)?;
        let mut definition: Self = serde_json::from_value(record.definition)
            .map_err(|e| format!("CAMPAIGN_RECORD_CORRUPT: {e}"))?;
        if let Some(event) = store.last_campaign_event(id, "tuned")? {
            let limits: Limits = serde_json::from_value(event.body["limits"].clone())
                .map_err(|e| format!("CAMPAIGN_RECORD_CORRUPT: {e}"))?;
            definition.spec.limits = limits;
        }
        Ok(definition)
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

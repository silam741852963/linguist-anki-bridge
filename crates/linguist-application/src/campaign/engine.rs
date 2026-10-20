//! The campaign tick: one step per lane per round, each recorded with
//! compare-and-set before the next. Every step either repeats safely
//! (`jobs run`, `resolve-batch --policy`, `generate --pending`, verification)
//! or records its intent first (apply jobs), so a kill between the effect and
//! the record is recovered by the next tick.
use super::{Approval, BatchData, Definition, SourceSpec, verify};
use linguist_store::{Store, campaign::BatchRecord, lease::Resource};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use uuid::Uuid;

/// One child command's result.
#[derive(Clone, Debug)]
pub struct Output {
    pub code: i32,
    pub json: Value,
    pub stderr: String,
}

impl Output {
    /// The error code a failed command printed (`{"error":"CODE: ..."}`).
    pub fn error(&self) -> String {
        let line = self.stderr.lines().rev().find(|l| l.contains("\"error\""));
        line.and_then(|l| serde_json::from_str::<Value>(l).ok())
            .and_then(|v| v["error"].as_str().map(str::to_owned))
            .unwrap_or_else(|| {
                let tail: String = self.stderr.chars().rev().take(300).collect();
                format!(
                    "EXIT_{}: {}",
                    self.code,
                    tail.chars().rev().collect::<String>()
                )
            })
    }
    pub fn code_name(&self) -> String {
        self.error()
            .split(':')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned()
    }
    /// Anki, the companion or a local service is not reachable now.
    pub fn unavailable(&self) -> bool {
        self.code == 3
            || ["UNAVAILABLE", "CONNECTION", "TIMEOUT", "ANKI_HTTP_FAILURE"]
                .iter()
                .any(|k| self.code_name().contains(k))
    }
}

/// Runs this binary's own subcommands for a source (purpose, settings and
/// the definition's global arguments are added by the implementation).
pub trait Runner: Sync {
    fn run(&self, source: &SourceSpec, args: &[String]) -> Output;
}

/// Read-only companion note evidence (`inspect_note`); `None` when the note
/// no longer exists.
pub trait Collection: Sync {
    fn note_evidence(&self, note_id: &str) -> Result<Option<Value>, String>;
}

pub struct Context<'a> {
    pub root: &'a std::path::Path,
    pub id: Uuid,
    pub definition: Definition,
    pub runner: &'a dyn Runner,
    pub collection: &'a dyn Collection,
    pub clock: &'a (dyn Fn() -> u64 + Sync),
    /// How often an idle lane looks again while another lane is busy.
    pub poll: std::time::Duration,
}

/// What one step did.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The batch moved (or changed) and was recorded.
    Moved(String),
    /// Nothing to do now (a service is down, the agent or user is needed).
    Waiting(String),
    /// Stop the campaign for the user.
    Pause { code: String, detail: String },
}

#[derive(Debug, serde::Serialize)]
pub struct TickReport {
    pub campaign: Uuid,
    pub steps: Vec<Value>,
    /// idle, deadline, paused, cancelled, closed, or the pause code.
    pub stopped: String,
    pub pruned: Vec<String>,
}

/// State the lanes of one tick share.
struct Shared {
    /// Why the tick stops (the first lane to decide wins).
    stop: Mutex<Option<String>>,
    steps: Mutex<Vec<Value>>,
    /// Per lane: found nothing to do since the last move on any lane.
    idle: Mutex<Vec<bool>>,
}

const PREP_PHASES: [&str; 3] = ["preparing", "reviewing", "needs_agent"];
const WRITE_PHASES: [&str; 4] = ["approved", "applying", "verifying", "merging"];
const LEASE_SECONDS: u64 = 120;
/// Failed drafts of one note before the agent decides (WP-24).
const MAX_DRAFT_FAILURES: u32 = 3;

fn s(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

impl Context<'_> {
    fn source(&self, data: &BatchData) -> Result<&SourceSpec, String> {
        self.definition
            .spec
            .source(&data.source)
            .ok_or_else(|| "CAMPAIGN_SOURCE_UNKNOWN".into())
    }
    fn cli(&self, data: &BatchData, args: Vec<String>) -> Result<Output, String> {
        Ok(self.runner.run(self.source(data)?, &args))
    }
    fn store(&self) -> Result<Store, String> {
        Store::open_existing(self.root)
    }
    /// Compare-and-set one batch.
    fn save(
        &self,
        record: &BatchRecord,
        phase: &str,
        data: &BatchData,
        event: Value,
    ) -> Result<(), String> {
        self.store()?.update_campaign_batch(
            self.id,
            record.batch,
            record.version,
            phase,
            &data.to_value(),
            if phase == record.phase {
                "batch"
            } else {
                "phase"
            },
            &json!({"from": record.phase, "to": phase, "detail": event}),
            (self.clock)(),
        )?;
        Ok(())
    }
    fn moved(
        &self,
        record: &BatchRecord,
        phase: &str,
        data: &BatchData,
        what: &str,
    ) -> Result<Step, String> {
        // A step that moves the batch ends its run of failures.
        let mut data = data.clone();
        data.step_failures = 0;
        data.last_error = None;
        self.save(record, phase, &data, json!(what))?;
        Ok(Step::Moved(format!("batch {}: {what}", record.batch)))
    }

    // ------------------------------------------------------------ prep lane

    fn start(&self, record: &BatchRecord, mut data: BatchData) -> Result<Step, String> {
        // A batch larger than the largest one of its source that published
        // after a size split is split before it is prepared (WP-24: each
        // failed attempt cost a whole preparation).
        if data.merge.is_empty()
            && let Some(cap) = self.size_cap(&data.source)?
            && data.active_notes().count() > cap
        {
            return self.split(record, data, "SIZE_CAP_FROM_EARLIER_SPLIT");
        }
        let mut args = s(&["jobs", "create", "--mode", "prepare"]);
        for note in data.active_notes() {
            args.extend(["--note-id".into(), note.clone()]);
        }
        let out = self.cli(&data, args)?;
        if out.unavailable() {
            return Ok(Step::Waiting(out.error()));
        }
        let job = out.json["job_id"]
            .as_str()
            .and_then(|j| Uuid::parse_str(j).ok())
            .ok_or_else(|| out.error())?;
        data.prepare_job = Some(job);
        self.moved(record, "preparing", &data, "prepare job created")
    }

    fn prepare(&self, record: &BatchRecord, mut data: BatchData) -> Result<Step, String> {
        let job = data
            .prepare_job
            .ok_or("CAMPAIGN_PREPARE_JOB_MISSING")?
            .to_string();
        if data
            .read_retry_after_ms
            .is_some_and(|at| (self.clock)() < at)
        {
            return Ok(Step::Waiting(format!(
                "batch {}: waiting before reading failed notes again",
                record.batch
            )));
        }
        let out = self.cli(&data, s(&["jobs", "run", &job]))?;
        if out.code == 7 && out.code_name() == "PREPARATION_ACTIVE_ITEM_REQUIRES_RECOVERY" {
            let rec = self.cli(
                &data,
                s(&["jobs", "recover", &job, "--execute", "--actor", "campaign"]),
            )?;
            if rec.code != 0 {
                return Err(rec.error());
            }
            return self.moved(record, "preparing", &data, "interrupted reads recovered");
        }
        if out.unavailable() && out.json.is_null() {
            return Ok(Step::Waiting(out.error()));
        }
        if out.json.is_null() && Self::too_big(&out) {
            return self.split(record, data, &out.code_name());
        }
        if out.json.is_null() {
            return Err(out.error());
        }
        let counts = &out.json["error_counts"];
        let connection_only = counts.as_object().is_some_and(|c| {
            !c.is_empty()
                && c.keys().all(|k| {
                    matches!(
                        k.as_str(),
                        "SOURCE_READ_CONNECTION_FAILED" | "SOURCE_READ_UNAVAILABLE"
                    )
                })
        });
        let pending = out.json["item_counts"]["pending"].as_u64().unwrap_or(0);
        if out.json["plan"].is_null()
            && out.json["captured_this_run"].as_u64() == Some(0)
            && connection_only
            && pending > 0
        {
            return Ok(Step::Waiting(format!(
                "batch {}: Anki is not reachable",
                record.batch
            )));
        }
        data.prepare_runs += 1;
        let mut plan = out.json["plan"]["id"].as_str().map(str::to_owned);
        let failed = out.json["item_counts"]["failed"].as_u64().unwrap_or(0);
        // Reads that failed because Anki was busy or briefly unreachable are
        // read again (after five minutes, three times) before any note is
        // left out of the batch.
        let transient = counts.as_object().is_some_and(|c| {
            !c.is_empty()
                && c.keys().all(|k| {
                    matches!(
                        k.as_str(),
                        "SOURCE_READ_CONNECTION_FAILED"
                            | "SOURCE_READ_UNAVAILABLE"
                            | "SOURCE_READ_TIMEOUT"
                    )
                })
        });
        if plan.is_none() && pending == 0 && failed > 0 && transient && data.read_retries < 3 {
            let retried = self.cli(&data, s(&["jobs", "retry", &job, "--failed"]))?;
            let none_eligible = retried.code == 4
                && retried.json["eligible"]
                    .as_array()
                    .is_some_and(|e| e.is_empty());
            if retried.code != 0 && !none_eligible {
                return Err(retried.error());
            }
            if none_eligible {
                // The job's own attempt budget is spent: leave the notes
                // out (deferred; `campaign requeue` reads them in a new job).
                data.read_retries = 3;
            } else {
                data.read_retries += 1;
                data.read_retry_after_ms = Some((self.clock)() + 5 * 60 * 1000);
                return self.moved(
                    record,
                    "preparing",
                    &data,
                    &format!("{failed} notes failed to read (Anki busy); read again later"),
                );
            }
        }
        if plan.is_none() && pending == 0 && failed > 0
            || plan.is_none() && data.prepare_runs >= self.definition.spec.limits.max_prepare_runs
        {
            let published = self.cli(&data, s(&["jobs", "publish", &job, "--skip-failed"]))?;
            for item in published.json["skipped"].as_array().into_iter().flatten() {
                if let Some(note) = item["note_id"].as_str() {
                    data.deferred.insert(
                        note.into(),
                        match item["error_detail"].as_str() {
                            Some(detail) => format!(
                                "PREPARE_FAILED: {} ({detail})",
                                item["error_code"].as_str().unwrap_or("")
                            ),
                            None => format!(
                                "PREPARE_FAILED: {}",
                                item["error_code"].as_str().unwrap_or("")
                            ),
                        },
                    );
                }
            }
            plan = published.json["plan"]["id"].as_str().map(str::to_owned);
            if plan.is_none() && published.code == 5 && pending == 0 {
                return self.moved(record, "skipped", &data, "every note failed to prepare");
            }
            // A refused publish is an error (counted, then a pause), never a
            // silent loop of prepare runs.
            if plan.is_none() && pending == 0 && published.json["plan"].is_null() {
                return Err(published.error());
            }
        }
        match plan {
            Some(plan) => {
                data.plan = Some(Uuid::parse_str(&plan).map_err(|_| "CAMPAIGN_PLAN_INVALID")?);
                self.map_documents(&mut data)?;
                self.moved(record, "reviewing", &data, "plan published")
            }
            None => self.moved(record, "preparing", &data, "prepare job ran; items remain"),
        }
    }

    fn reenrich(
        &self,
        record: &BatchRecord,
        data: &mut BatchData,
        plan: &str,
    ) -> Result<Result<Vec<String>, Step>, String> {
        let shown = self.plan_show(data, false)?;
        let mut exhausted = Vec::new();
        let mut targets = Vec::new();
        for document in shown["documents"].as_array().into_iter().flatten() {
            let outage = document["issues"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|i| {
                    i["code"] == "AUDIO_SYNTHESIS_FAILED"
                        && i["message"]
                            .as_str()
                            .is_some_and(|m| m.contains("UNAVAILABLE"))
                });
            let has_audio = document["media"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|m| m["role"] == "audio");
            let Some(id) = document["id"].as_str() else {
                continue;
            };
            if outage && !has_audio {
                targets.push((id.to_owned(), "enrichment"));
            }
            // A dictionary entry the generation chain could not write while
            // its provider was at a usage limit or down (WP-24: notes prepared
            // during a Codex cooldown kept no meaning) is written again.
            if dictionary_outage(document) && meaning_empty(document) {
                targets.push((id.to_owned(), "dictionary"));
            }
        }
        for (id, stage) in targets {
            let marker = match stage {
                "enrichment" => format!("reenrich:{id}"),
                _ => format!("re{stage}:{id}"),
            };
            let tries = data.settled.iter().filter(|s| **s == marker).count();
            if tries >= 3 {
                if stage == "enrichment"
                    && let Some(note) = self.note_of(data, &id)
                {
                    exhausted.push(note);
                }
                continue;
            }
            let head = self.plan_show(data, true)?;
            let rev = head["revision"].to_string();
            let digest = head["digest"].as_str().unwrap_or_default().to_owned();
            let out = self.cli(
                data,
                s(&[
                    "plans",
                    "regenerate",
                    plan,
                    "--stage",
                    stage,
                    "--item-id",
                    &id,
                    "--base-revision",
                    &rev,
                    "--digest",
                    &digest,
                    "--use-current-settings",
                ]),
            )?;
            if out.json.is_null() {
                if out.unavailable() {
                    return Ok(Err(Step::Waiting(out.error())));
                }
                return Err(out.error());
            }
            // Still at the usage limit: wait for the provider, like drafting,
            // without spending a try.
            if stage == "dictionary" {
                let again = self.plan_show(data, false)?;
                let limited = again["documents"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|d| d["id"] == id.as_str())
                    .flat_map(|d| d["issues"].as_array().into_iter().flatten())
                    .filter(|i| i["code"] == "DICTIONARY_ENTRY_GENERATION_FAILED")
                    .filter_map(|i| i["message"].as_str())
                    .find(|m| crate::agents::usage_limited(m));
                if let Some(message) = limited {
                    return Ok(Err(Step::Waiting(format!(
                        "DICTIONARY_ENTRY_UNAVAILABLE: {}",
                        message.chars().take(200).collect::<String>()
                    ))));
                }
            }
            data.settled.push(marker);
            self.save(
                record,
                &record.phase,
                data,
                json!(format!("{stage} run again after a service outage")),
            )?;
            return Ok(Err(Step::Moved(format!(
                "batch {}: item {id}: {stage} run again after a service outage",
                record.batch
            ))));
        }
        Ok(Ok(exhausted))
    }

    /// The smallest size a split of this source kept, if any batch of the
    /// source was split for size.
    fn size_cap(&self, source: &str) -> Result<Option<usize>, String> {
        let store = self.store()?;
        let batches = store.campaign_batches(self.id)?;
        let of_source: BTreeSet<u32> = batches
            .iter()
            .filter(|b| b.data["source"] == source)
            .map(|b| b.batch)
            .collect();
        let mut cap: Option<usize> = None;
        let mut after = 0;
        loop {
            let events = store.campaign_events(self.id, after, 1000)?;
            let Some(last) = events.last() else { break };
            after = last.sequence;
            for e in events.iter().filter(|e| e.kind == "split") {
                if e.batch.is_some_and(|b| of_source.contains(&b))
                    && e.body["detail"]["reason"] != "SIZE_CAP_FROM_EARLIER_SPLIT"
                    && let Some(kept) = e.body["detail"]["kept"].as_u64()
                {
                    let kept = kept as usize;
                    cap = Some(cap.map_or(kept, |c| c.min(kept)));
                }
            }
        }
        Ok(cap)
    }

    /// The batch's plan or archive is over the frozen size limits.
    fn too_big(out: &Output) -> bool {
        matches!(
            out.code_name().as_str(),
            "REVAMP_BATCH_ARCHIVE_LIMIT" | "PREPARATION_PLAN_SIZE_LIMIT"
        )
    }

    /// Halve a batch whose plan cannot be published: the first half is
    /// prepared again here, the second half becomes a new batch at the end.
    fn split(&self, record: &BatchRecord, data: BatchData, why: &str) -> Result<Step, String> {
        let notes: Vec<String> = data.active_notes().cloned().collect();
        if notes.len() < 2 || !data.merge.is_empty() {
            return Err(format!("{why}: a single note is over the size limit"));
        }
        let (first, second) = notes.split_at(notes.len() / 2);
        let part = |ids: &[String], pilot: bool| BatchData {
            source: data.source.clone(),
            notes: ids.to_vec(),
            pilot,
            split_candidates: data
                .split_candidates
                .iter()
                .filter(|n| ids.contains(n))
                .cloned()
                .collect(),
            ..Default::default()
        };
        let new = self.store()?.split_campaign_batch(
            self.id,
            record.batch,
            record.version,
            "selected",
            &part(first, data.pilot).to_value(),
            &part(second, false).to_value(),
            &json!({"reason": why, "kept": first.len(), "moved": second.len()}),
            (self.clock)(),
        )?;
        Ok(Step::Moved(format!(
            "batch {}: {why}; split, {} notes moved to batch {new}",
            record.batch,
            second.len()
        )))
    }

    fn plan_show(&self, data: &BatchData, issues_only: bool) -> Result<Value, String> {
        let plan = data.plan.ok_or("CAMPAIGN_PLAN_MISSING")?.to_string();
        let mut args = s(&["plans", "show", &plan]);
        if issues_only {
            args.push("--issues-only".into());
        }
        let out = self.cli(data, args)?;
        if out.json.is_null() {
            return Err(out.error());
        }
        Ok(out.json)
    }

    /// note ID -> document ID (split units map to their anchor's note).
    fn map_documents(&self, data: &mut BatchData) -> Result<(), String> {
        let plan = self.plan_show(data, false)?;
        data.documents.clear();
        for document in plan["documents"].as_array().into_iter().flatten() {
            let note = document["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| s["location"].as_str())
                .find_map(|l| l.strip_prefix("anki_note:"));
            if let (Some(note), Some(id)) = (note, document["id"].as_str()) {
                data.documents
                    .entry(note.to_owned())
                    .or_insert(Uuid::parse_str(id).map_err(|_| "CAMPAIGN_PLAN_INVALID")?);
            }
        }
        Ok(())
    }

    fn note_of(&self, data: &BatchData, document: &str) -> Option<String> {
        data.documents
            .iter()
            .find(|(_, d)| d.to_string() == document)
            .map(|(n, _)| n.clone())
    }

    /// Policy decisions and generation until nothing changes.
    fn review(&self, record: &BatchRecord, mut data: BatchData) -> Result<Step, String> {
        let plan = data.plan.ok_or("CAMPAIGN_PLAN_MISSING")?.to_string();
        let head = self.plan_show(&data, true)?;
        let revision = head["revision"].as_u64().unwrap_or(0) as u32;
        if record.phase == "needs_agent"
            && data.checked_revision == Some(revision)
            && !needs_met(&data, &head)
        {
            return Ok(Step::Waiting(format!(
                "batch {}: waiting for the agent",
                record.batch
            )));
        }
        if !data.merge.is_empty() && !data.settled.iter().any(|n| n == "merge:added") {
            let document = data
                .documents
                .get(&data.notes[0])
                .ok_or("CAMPAIGN_MERGE_SURVIVOR_NOT_PREPARED")?
                .to_string();
            let mut args = s(&["plans", "merge-notes", &plan, "--item", &document]);
            for note in &data.merge {
                args.extend(["--note".into(), note.clone()]);
            }
            let out = self.cli(&data, args)?;
            if out.unavailable() {
                return Ok(Step::Waiting(out.error()));
            }
            if out.code != 0 && out.code != 4 {
                return Err(out.error());
            }
            data.settled.push("merge:added".into());
            return self.moved(record, &record.phase, &data, "merged notes' content added");
        }
        // Audio is required on every card (WP-20): enrichment that found a
        // synthesis service down is run again, up to three times per item.
        let exhausted = match self.reenrich(record, &mut data, &plan)? {
            Ok(exhausted) => exhausted,
            Err(step) => return Ok(step),
        };
        let policy = {
            let source = self.source(&data)?;
            let path = super::source_policy_path(&self.definition.policy_path, &source.id);
            if source.extra_agent_codes.is_empty() || !std::path::Path::new(&path).exists() {
                self.definition.policy_path.clone()
            } else {
                path
            }
        };
        let mut undecided = Vec::new();
        // The revision the undecided list describes: an agent's decision
        // that lands while this step runs makes a newer one, reviewed again.
        let mut evaluated: Option<u32> = None;
        for round in 0..self.definition.spec.limits.max_review_rounds {
            let out = self.cli(
                &data,
                s(&["plans", "resolve-batch", &plan, "--policy", &policy]),
            )?;
            if out.unavailable() {
                return Ok(Step::Waiting(out.error()));
            }
            if out.json.is_null() {
                return Err(out.error());
            }
            undecided = out.json["policy"]["undecided"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            evaluated = out.json["revision"].as_u64().map(|r| r as u32);
            // The loop always ends on a policy pass: generation reopens the
            // reviews of every item it drafts.
            if round + 1 == self.definition.spec.limits.max_review_rounds {
                // A companion without its collection session (Anki closed
                // and reopened the collection, as for a checkpoint export)
                // comes back by itself: wait, it is no step failure (WP-24:
                // three such reviews paused the campaign).
                if out.code == 5
                    && out.json["outcome"]["conflict"]
                        .to_string()
                        .contains("CAPABILITY_UNAVAILABLE")
                {
                    return Ok(Step::Waiting(format!(
                        "REVIEW_BATCH_CONFLICT: {}",
                        out.json["outcome"]["conflict"]
                    )));
                }
                if out.code == 5 {
                    return Err(format!(
                        "REVIEW_BATCH_CONFLICT: {}",
                        out.json["outcome"]["conflict"]
                    ));
                }
                break;
            }
            let head = self.plan_show(&data, true)?;
            let (rev, digest) = (
                head["revision"].to_string(),
                head["digest"].as_str().unwrap_or_default().to_owned(),
            );
            let generated = self.cli(
                &data,
                s(&[
                    "plans",
                    "generate",
                    &plan,
                    "--all-items",
                    "--pending",
                    "--base-revision",
                    &rev,
                    "--digest",
                    &digest,
                    "--use-current-settings",
                    "--certified-engine-only",
                ]),
            )?;
            if generated.unavailable() && generated.json.is_null() {
                return Ok(Step::Waiting(generated.error()));
            }
            let items = generated.json["items"].as_array().into_iter().flatten();
            // A draft that failed (usage limit, provider down, a draft the
            // checks refused) is not "nothing new": the item would reach
            // approval without its generated fields. A usage limit waits;
            // otherwise each note is drafted again up to MAX_DRAFT_FAILURES
            // times, then left to the agent.
            let mut again = false;
            for item in items.clone() {
                let Some(error) = item.get("error").and_then(Value::as_str) else {
                    if item.get("result").is_some()
                        && let Some(note) =
                            self.note_of(&data, item["document_id"].as_str().unwrap_or_default())
                    {
                        data.draft_failures.remove(&note);
                    }
                    continue;
                };
                // A local engine no certification covers drafts nothing until
                // it is certified again (an upgraded Ollama): the user's step.
                if error.contains("GENERATION_ENGINE_UNVERIFIED") {
                    return Ok(Step::Pause {
                        code: "GENERATION_ENGINE_UNVERIFIED".into(),
                        detail: error.chars().take(300).collect(),
                    });
                }
                // Only when every provider of the chain hit its usage limit:
                // a fallback that failed otherwise counts as a failed draft.
                if error.split("; fallback ").all(crate::agents::usage_limited) {
                    return Ok(Step::Waiting(format!("GENERATION_UNAVAILABLE: {error}")));
                }
                let document = item["document_id"].as_str().unwrap_or_default();
                let Some(note) = self.note_of(&data, document) else {
                    continue;
                };
                if data.deferred.contains_key(&note) {
                    continue;
                }
                let failures = data.draft_failures.entry(note).or_default();
                failures.0 += 1;
                failures.1 = error.chars().take(300).collect();
                again |= failures.0 < MAX_DRAFT_FAILURES;
            }
            if again {
                return self.moved(
                    record,
                    &record.phase,
                    &data,
                    "drafts failed; drafting again",
                );
            }
            let fresh = items.filter(|i| i.get("result").is_some()).count();
            // Nothing new was drafted: the policy pass above saw the final plan.
            if fresh == 0 && out.code != 5 {
                break;
            }
        }
        data.review_rounds += 1;
        data.needs.clear();
        for entry in &undecided {
            let document = entry["document_id"].as_str().unwrap_or_default();
            let Some(note) = self.note_of(&data, document) else {
                continue;
            };
            if data.deferred.contains_key(&note) {
                continue;
            }
            data.needs.entry(note).or_default().push(format!(
                "{}: {}",
                entry["issue_id"].as_str().unwrap_or_default(),
                entry["reason"].as_str().unwrap_or_default()
            ));
        }
        // Every merge is confirmed by the agent: the survivor and its
        // duplicates must be the same word (homographs stay apart).
        if !data.merge.is_empty() && !data.settled.contains(&data.notes[0]) {
            data.needs.entry(data.notes[0].clone()).or_default().push(format!(
                "MERGE_CONFIRM: confirm that {} hold the same word as this survivor, then mark it settled; otherwise skip the batch",
                data.merge.join(", ")
            ));
        }
        for (note, (_, error)) in &data.draft_failures {
            if !data.deferred.contains_key(note) {
                data.needs.entry(note.clone()).or_default().push(format!(
                    "GENERATION_FAILED: {MAX_DRAFT_FAILURES} drafts failed ({error}); defer it, or `campaign reopen --fresh` the batch"
                ));
            }
        }
        for note in exhausted {
            if !data.deferred.contains_key(&note) {
                data.needs.entry(note).or_default().push(
                    "AUDIO_SYNTHESIS_UNAVAILABLE: start VOICEVOX, then `plans regenerate --stage enrichment` for this item, or defer it".into(),
                );
            }
        }
        for note in &data.split_candidates {
            if !data.settled.contains(note) && !data.deferred.contains_key(note) {
                data.needs.entry(note.clone()).or_default().push(
                    "SPLIT_CANDIDATE: split with `plans split-vocab`, or mark it settled".into(),
                );
            }
        }
        let head = self.plan_show(&data, true)?;
        data.checked_revision = evaluated.or(head["revision"].as_u64().map(|r| r as u32));
        self.map_documents(&mut data)?;
        // Open issues left only on deferred notes do not hold the batch.
        let open_active = head["issues"].as_array().into_iter().flatten().any(|e| {
            e["request_identity"]["document_id"]
                .as_str()
                .and_then(|d| self.note_of(&data, d))
                .is_none_or(|n| !data.deferred.contains_key(&n))
        });
        // Readiness is judged on the final plan, never on an earlier pass.
        if data.needs.is_empty() && !open_active {
            self.moved(record, "ready", &data, "ready for approval")
        } else {
            if data.needs.is_empty() {
                data.needs.insert(
                    "*".into(),
                    vec![
                        "PLAN_NOT_READY: open issues remain; see `plans show --issues-only`".into(),
                    ],
                );
            }
            let n = data.needs.len();
            self.moved(
                record,
                "needs_agent",
                &data,
                &format!("{n} notes need the agent"),
            )
        }
    }

    // ------------------------------------------------------------ write lane

    /// Create the apply job for the approved items, or for `pending` ones
    /// after a session ended. The intent is recorded first.
    fn create_apply_job(
        &self,
        record: &BatchRecord,
        mut data: BatchData,
        items: &[Uuid],
        why: &str,
    ) -> Result<Step, String> {
        let approval = data.approval.clone().ok_or("CAMPAIGN_APPROVAL_MISSING")?;
        if data.apply_intent_ms.is_none() {
            data.apply_intent_ms = Some((self.clock)());
            return self.moved(record, &record.phase, &data, "apply job intent recorded");
        }
        let plan = data.plan.ok_or("CAMPAIGN_PLAN_MISSING")?.to_string();
        let rev = approval.revision.to_string();
        let mut args = s(&[
            "jobs",
            "create",
            "--mode",
            "apply",
            "--plan",
            &plan,
            "--revision",
            &rev,
            "--digest",
            &approval.digest,
        ]);
        for item in items {
            args.extend(["--item-id".into(), item.to_string()]);
        }
        args.extend(s(&["--accept-schema-change", "--create-checkpoint"]));
        let out = self.cli(&data, args)?;
        if out.unavailable() && out.json.is_null() {
            return Ok(Step::Waiting(out.error()));
        }
        let job = out.json["job"]["job_id"]
            .as_str()
            .and_then(|j| Uuid::parse_str(j).ok())
            .ok_or_else(|| out.error())?;
        // The job adopts its checkpoint's group: record it for retention.
        self.record_checkpoints(record.batch, data.apply_intent_ms.unwrap(), &data)?;
        data.apply_jobs.push(job);
        data.apply_intent_ms = None;
        self.moved(record, "applying", &data, why)
    }

    /// Checkpoints created since `since_ms` belong to this batch (including a
    /// job created by a tick that was killed before recording it).
    fn record_checkpoints(
        &self,
        batch: u32,
        since_ms: u64,
        data: &BatchData,
    ) -> Result<(), String> {
        let out = self.cli(data, s(&["backup", "list", "--limit", "50"]))?;
        let mut store = self.store()?;
        for receipt in out.json["checkpoints"]
            .as_array()
            .or_else(|| out.json["receipts"].as_array())
            .into_iter()
            .flatten()
        {
            let created = receipt["created_ms"].as_u64().unwrap_or(0);
            let (Some(id), Some(path)) = (
                receipt["id"]
                    .as_str()
                    .or_else(|| receipt["receipt"]["id"].as_str()),
                receipt["path"]
                    .as_str()
                    .or_else(|| receipt["receipt"]["path"].as_str()),
            ) else {
                continue;
            };
            if created + 1000 >= since_ms {
                store.record_campaign_checkpoint(self.id, id, batch, path, created)?;
            }
        }
        Ok(())
    }

    fn approved(&self, record: &BatchRecord, mut data: BatchData) -> Result<Step, String> {
        let approval = data.approval.clone().ok_or("CAMPAIGN_APPROVAL_MISSING")?;
        if data.before.is_empty() {
            for note in &approval.notes {
                let Some(evidence) = self.collection.note_evidence(note)? else {
                    data.deferred
                        .insert(note.clone(), "NOTE_GONE_BEFORE_APPLY".into());
                    continue;
                };
                data.before.insert(note.clone(), verify::facts(&evidence)?);
            }
            return self.moved(record, "approved", &data, "card facts read before apply");
        }
        if approval.items.is_empty() {
            return self.moved(record, "applying", &data, "no job items; split groups only");
        }
        let items = approval.items.clone();
        self.create_apply_job(record, data, &items, "apply job created")
    }

    fn applying(&self, record: &BatchRecord, mut data: BatchData) -> Result<Step, String> {
        let approval = data.approval.clone().ok_or("CAMPAIGN_APPROVAL_MISSING")?;
        if data.apply_intent_ms.is_some() {
            // A session ended: the follow-up job for the pending items.
            let pending: Vec<Uuid> = serde_json::from_value(
                data.needs
                    .get("#pending")
                    .map(|p| json!(p))
                    .unwrap_or(json!([])),
            )
            .unwrap_or_default();
            return self.create_apply_job(record, data, &pending, "follow-up apply job created");
        }
        if let Some(job) = data
            .apply_jobs
            .last()
            .copied()
            .filter(|j| !data.finished_jobs.contains(j))
        {
            let job_text = job.to_string();
            let verb = if data.started_jobs.contains(&job) {
                "resume"
            } else {
                "run"
            };
            let out = self.cli(&data, s(&["jobs", verb, &job_text, "--apply"]))?;
            let run = &out.json["run"];
            if run.is_null() {
                if out.unavailable() {
                    return Ok(Step::Waiting(out.error()));
                }
                return Err(out.error());
            }
            if !data.started_jobs.contains(&job) {
                data.started_jobs.push(job);
            }
            let counts = &run["status"]["counts"];
            let n = |k: &str| counts[k].as_u64().unwrap_or(0);
            if n("needs_recovery") > 0 || run["stop"] == "recovery_required" {
                return self.reconcile(record, data);
            }
            let items = self.cli(&data, s(&["jobs", "items", &job_text, "--limit", "10000"]))?;
            let mut pending = Vec::new();
            for item in items.json["items"].as_array().into_iter().flatten() {
                let state = item["state"].as_str().unwrap_or_default();
                let document = item["item_id"].as_str().unwrap_or_default();
                let note = self.note_of(&data, document);
                match (state, note) {
                    ("committed", Some(note)) if !data.committed.contains(&note) => {
                        data.committed.push(note)
                    }
                    ("failed" | "failed_before_write", Some(note)) => {
                        data.deferred.entry(note).or_insert(format!(
                            "APPLY_FAILED: {}",
                            item["error_code"]
                                .as_str()
                                .or(item["code"].as_str())
                                .unwrap_or("")
                        ));
                    }
                    ("pending", _) => pending.push(document.to_owned()),
                    _ => {}
                }
            }
            let stop_code = run["stop_code"].as_str().unwrap_or_default();
            if stop_code == "JOB_CHECKPOINT_SESSION_ENDED" && !pending.is_empty() {
                data.needs.insert("#pending".into(), pending);
                data.apply_intent_ms = Some((self.clock)());
                return self.moved(
                    record,
                    "applying",
                    &data,
                    "session ended; follow-up job intent recorded",
                );
            }
            if run["stop"] == "halted" {
                return Ok(Step::Pause {
                    code: "CAMPAIGN_PAUSED_IDENTITY".into(),
                    detail: format!("apply job {job} halted: {stop_code}"),
                });
            }
            if !pending.is_empty() {
                if out.unavailable() {
                    return Ok(Step::Waiting(out.error()));
                }
                return self.moved(record, "applying", &data, "apply job continues");
            }
            data.finished_jobs.push(job);
            data.needs.remove("#pending");
            return self.moved(record, "applying", &data, "apply job finished");
        }
        // Split groups, each with its own checkpoint, after the job items.
        let plan = data.plan.ok_or("CAMPAIGN_PLAN_MISSING")?.to_string();
        for group in &approval.groups {
            if data.groups_done.contains(group) {
                continue;
            }
            let rev = approval.revision.to_string();
            let since = (self.clock)();
            let out = self.cli(
                &data,
                s(&[
                    "apply",
                    &plan,
                    "--revision",
                    &rev,
                    "--digest",
                    &approval.digest,
                    "--split-group",
                    &group.to_string(),
                    "--accept-schema-change",
                    "--apply",
                ]),
            )?;
            if out.json.is_null() {
                if out.unavailable() {
                    return Ok(Step::Waiting(out.error()));
                }
                return Err(out.error());
            }
            self.record_checkpoints(record.batch, since, &data)?;
            match out.json["split"]["state"].as_str() {
                Some("complete") => {
                    data.groups_done.push(*group);
                    return self.moved(record, "applying", &data, "split group applied");
                }
                _ => {
                    return Ok(Step::Pause {
                        code: "CAMPAIGN_PAUSED_SPLIT".into(),
                        detail: format!("split group {group}: {}", out.json["split"]),
                    });
                }
            }
        }
        self.moved(record, "verifying", &data, "every apply finished")
    }

    fn reconcile(&self, record: &BatchRecord, data: BatchData) -> Result<Step, String> {
        let pending = self.cli(&data, s(&["recover", "inspect", "--pending"]))?;
        for journal in pending.json["journals"].as_array().into_iter().flatten() {
            let Some(op) = journal["journal"]["id"].as_str() else {
                continue;
            };
            // `--rebind` accepts a new Anki session; it is refused while
            // the session is unchanged, so it is only the second attempt.
            let mut out = self.cli(&data, s(&["recover", "reconcile", op, "--apply"]))?;
            if out.code != 0 && out.error().contains("SESSION_CHANGED") {
                out = self.cli(
                    &data,
                    s(&["recover", "reconcile", op, "--rebind", "--apply"]),
                )?;
            }
            if out.code != 0 {
                if out.unavailable() {
                    return Ok(Step::Waiting(out.error()));
                }
                // Exit 4: reconciled but still unresolved, an outcome on
                // stdout (Anki was away a moment longer). Reconciling is
                // idempotent: a step failure tries again, and the step
                // failure guardrail pauses with it if it never resolves.
                if out.code == 4 && !out.json["reconcile"].is_null() {
                    let why = format!("unresolved: {}", out.json["reconcile"]);
                    return Err(format!(
                        "CAMPAIGN_RECONCILE_UNRESOLVED: operation {op}: {}",
                        why.chars().take(600).collect::<String>()
                    ));
                }
                return Ok(Step::Pause {
                    code: "CAMPAIGN_PAUSED_UNKNOWN_OUTCOME".into(),
                    detail: format!("operation {op}: {}", out.error()),
                });
            }
        }
        self.moved(record, "applying", &data, "unknown outcomes reconciled")
    }

    fn verifying(&self, record: &BatchRecord, mut data: BatchData) -> Result<Step, String> {
        let source = self.source(&data)?.clone();
        let mut problems = Vec::new();
        for (note, before) in &data.before {
            if data.deferred.contains_key(note) {
                continue;
            }
            let after = match self.collection.note_evidence(note)? {
                Some(evidence) => Some(verify::facts(&evidence)?),
                None => None,
            };
            problems.extend(verify::compare(
                before,
                after.as_ref(),
                &source.target_models,
            ));
        }
        if !problems.is_empty() {
            data.verify_problems = problems.clone();
            self.save(record, "verifying", &data, json!("verification failed"))?;
            return Ok(Step::Pause {
                code: "CAMPAIGN_PAUSED_VERIFY".into(),
                detail: problems.join("; "),
            });
        }
        let next = if data.merge.is_empty() {
            "done"
        } else {
            "merging"
        };
        self.moved(record, next, &data, "verified")
    }

    fn merging(&self, record: &BatchRecord, mut data: BatchData) -> Result<Step, String> {
        let survivor = data.notes[0].clone();
        if data.deferred.contains_key(&survivor) {
            return Ok(Step::Pause {
                code: "CAMPAIGN_PAUSED_MERGE".into(),
                detail: format!("survivor {survivor} was not applied; nothing is deleted"),
            });
        }
        let plan = data.plan.ok_or("CAMPAIGN_PLAN_MISSING")?.to_string();
        for note in data.merge.clone() {
            if data.merged.contains(&note) {
                continue;
            }
            let out = self.cli(
                &data,
                s(&[
                    "notes",
                    "delete-merged",
                    &note,
                    "--survivor",
                    &survivor,
                    "--plan",
                    &plan,
                    "--apply",
                ]),
            )?;
            if out.code != 0 {
                if out.unavailable() {
                    return Ok(Step::Waiting(out.error()));
                }
                return Ok(Step::Pause {
                    code: "CAMPAIGN_PAUSED_MERGE".into(),
                    detail: format!("delete of merged note {note}: {}", out.error()),
                });
            }
            if self.collection.note_evidence(&note)?.is_some() {
                return Ok(Step::Pause {
                    code: "CAMPAIGN_PAUSED_MERGE".into(),
                    detail: format!("merged note {note} still exists after its delete"),
                });
            }
            data.merged.push(note);
            return self.moved(record, "merging", &data, "merged note deleted");
        }
        self.moved(record, "done", &data, "merge complete")
    }

    // ------------------------------------------------------------ dispatch

    fn step(&self, record: &BatchRecord) -> Result<Step, String> {
        let data = BatchData::from_value(&record.data)?;
        match record.phase.as_str() {
            "selected" => self.start(record, data),
            "preparing" => self.prepare(record, data),
            "reviewing" | "needs_agent" => self.review(record, data),
            "approved" => self.approved(record, data),
            "applying" => self.applying(record, data),
            "verifying" => self.verifying(record, data),
            "merging" => self.merging(record, data),
            other => Ok(Step::Waiting(format!("batch {}: {other}", record.batch))),
        }
    }

    /// Run one step and turn repeated failures into a pause.
    fn guarded(&self, record: &BatchRecord) -> Step {
        match self.step(record) {
            Ok(step) => step,
            Err(error) => {
                let Ok(current) = self
                    .store()
                    .and_then(|s| s.campaign_batch(self.id, record.batch))
                else {
                    return Step::Waiting(error);
                };
                let Ok(mut data) = BatchData::from_value(&current.data) else {
                    return Step::Pause {
                        code: "CAMPAIGN_BATCH_CORRUPT".into(),
                        detail: error,
                    };
                };
                data.step_failures += 1;
                data.last_error = Some(error.clone());
                let failures = data.step_failures;
                let _ = self.save(&current, &current.phase, &data, json!({"error": error}));
                if failures >= self.definition.spec.limits.max_step_failures {
                    Step::Pause {
                        code: "CAMPAIGN_PAUSED_STEP_FAILED".into(),
                        detail: format!("batch {} {}: {error}", record.batch, record.phase),
                    }
                } else {
                    Step::Waiting(format!("batch {}: {error}", record.batch))
                }
            }
        }
    }

    /// Whether an agent run started and has not reported its end (it holds
    /// `needs_agent` batches so a review never races the agent's edits).
    fn agent_active(&self, store: &Store) -> Result<bool, String> {
        let Some(start) = store.last_campaign_event(self.id, "agent_run")? else {
            return Ok(false);
        };
        let ended = store
            .last_campaign_event(self.id, "agent_done")?
            .is_some_and(|done| done.sequence > start.sequence);
        Ok(!ended && (self.clock)().saturating_sub(start.at_ms) < super::AGENT_HOLD_MS)
    }

    /// Step `batch` if it is still in one of `phases` (read fresh: another
    /// lane or command may have moved it since the caller's snapshot).
    fn step_fresh(&self, batch: u32, phases: &[&str]) -> Result<Option<Step>, String> {
        let record = self.batch_record(batch)?;
        if !phases.contains(&record.phase.as_str()) {
            return Ok(None);
        }
        Ok(Some(self.guarded(&record)))
    }

    /// One prep worker's step: the first prep-phase batch no other worker
    /// holds whose step moves, else a new batch while fewer than
    /// `prepare_ahead` are in flight.
    fn prep_lane(&self, claims: &Mutex<BTreeSet<u32>>) -> Result<Option<(u32, Step)>, String> {
        let store = self.store()?;
        let hold = self.agent_active(&store)?;
        let batches = store.campaign_batches(self.id)?;
        drop(store);
        for record in batches.iter().filter(|b| {
            PREP_PHASES.contains(&b.phase.as_str()) && !(hold && b.phase == "needs_agent")
        }) {
            if !claims.lock().unwrap().insert(record.batch) {
                continue;
            }
            let phases: &[&str] = if hold {
                &PREP_PHASES[..2]
            } else {
                &PREP_PHASES
            };
            let step = self.step_fresh(record.batch, phases);
            claims.lock().unwrap().remove(&record.batch);
            match step? {
                Some(Step::Waiting(_)) | None => {}
                Some(step) => return Ok(Some((record.batch, step))),
            }
        }
        let next = {
            let mut claims = claims.lock().unwrap();
            let batches = self.store()?.campaign_batches(self.id)?;
            let in_flight = batches
                .iter()
                .filter(|b| {
                    matches!(
                        b.phase.as_str(),
                        "preparing" | "reviewing" | "needs_agent" | "ready" | "approved"
                    ) || (b.phase == "selected" && claims.contains(&b.batch))
                })
                .count() as u32;
            if in_flight >= self.definition.spec.limits.prepare_ahead {
                return Ok(None);
            }
            let Some(record) = batches
                .iter()
                .find(|b| b.phase == "selected" && !claims.contains(&b.batch))
            else {
                return Ok(None);
            };
            claims.insert(record.batch);
            record.batch
        };
        let step = self.step_fresh(next, &["selected"]);
        claims.lock().unwrap().remove(&next);
        Ok(step?.map(|step| (next, step)))
    }

    fn write_lane(&self) -> Result<Option<(u32, Step)>, String> {
        let batches = self.store()?.campaign_batches(self.id)?;
        let Some(record) = batches
            .iter()
            .find(|b| WRITE_PHASES.contains(&b.phase.as_str()))
        else {
            return Ok(None);
        };
        Ok(self
            .step_fresh(record.batch, &WRITE_PHASES)?
            .map(|step| (record.batch, step)))
    }

    /// The pilot stops the campaign for the user's review once it is done.
    fn pilot_review(&self) -> Result<bool, String> {
        let mut store = self.store()?;
        let batches = store.campaign_batches(self.id)?;
        let Some(pilot) = batches.iter().find(|b| {
            b.phase == "done" && b.data["pilot"] == true && b.data["pilot_reviewed"] != true
        }) else {
            return Ok(false);
        };
        let mut data = pilot.data.clone();
        data["pilot_reviewed"] = true.into();
        store.update_campaign_batch(
            self.id,
            pilot.batch,
            pilot.version,
            "done",
            &data,
            "batch",
            &json!("pilot review requested"),
            (self.clock)(),
        )?;
        self.pause(
            "CAMPAIGN_PILOT_REVIEW",
            "the pilot batch is applied; review it, then run `campaign resume`",
        )?;
        Ok(true)
    }

    /// One lane's loop: step until every lane is idle at once, the campaign
    /// stops running, a step pauses it, or the deadline passes. A move on
    /// any lane wakes the idle ones (it may have made work for them).
    fn lane_loop(
        &self,
        lane: usize,
        shared: &Shared,
        deadline: u64,
        step: &(dyn Fn() -> Result<Option<(u32, Step)>, String> + Sync),
    ) {
        let stop = |why: String| {
            shared.stop.lock().unwrap().get_or_insert(why);
        };
        loop {
            if shared.stop.lock().unwrap().is_some() {
                return;
            }
            if (self.clock)() >= deadline {
                return stop("deadline".into());
            }
            match self.store().and_then(|s| s.campaign(self.id)) {
                Ok(c) if c.status != "running" => return stop(c.status),
                Ok(_) => {}
                Err(e) => return stop(format!("error: {e}")),
            }
            let moved = match step() {
                Err(e) => return stop(format!("error: {e}")),
                Ok(None) | Ok(Some((_, Step::Waiting(_)))) => false,
                Ok(Some((batch, step))) => {
                    shared
                        .steps
                        .lock()
                        .unwrap()
                        .push(json!({"batch": batch, "step": format!("{step:?}")}));
                    if let Step::Pause { code, detail } = &step {
                        if let Err(e) = self.pause(code, detail) {
                            return stop(format!("error: {e}"));
                        }
                        return stop(code.clone());
                    }
                    true
                }
            };
            if moved {
                shared
                    .idle
                    .lock()
                    .unwrap()
                    .iter_mut()
                    .for_each(|i| *i = false);
                if lane == 0 {
                    match self.pilot_review() {
                        Ok(true) => return stop("CAMPAIGN_PILOT_REVIEW".into()),
                        Ok(false) => {}
                        Err(e) => return stop(format!("error: {e}")),
                    }
                }
                continue;
            }
            {
                let mut idle = shared.idle.lock().unwrap();
                idle[lane] = true;
                if idle.iter().all(|i| *i) {
                    return stop("idle".into());
                }
            }
            std::thread::sleep(self.poll);
        }
    }

    /// Delete checkpoint files of `done` batches beyond the newest
    /// `retain_done_checkpoints`; never a checkpoint the campaign did not create.
    pub fn retention(&self) -> Result<Vec<String>, String> {
        let mut store = self.store()?;
        let batches = store.campaign_batches(self.id)?;
        let mut done: Vec<&BatchRecord> = batches.iter().filter(|b| b.phase == "done").collect();
        done.sort_by_key(|b| std::cmp::Reverse(b.updated_ms));
        let keep: BTreeSet<u32> = done
            .iter()
            .take(self.definition.spec.limits.retain_done_checkpoints as usize)
            .map(|b| b.batch)
            .collect();
        let finished: BTreeSet<u32> = done.iter().map(|b| b.batch).collect();
        let mut pruned = Vec::new();
        for checkpoint in store.campaign_checkpoints(self.id)? {
            if checkpoint.pruned_ms.is_some()
                || !finished.contains(&checkpoint.batch)
                || keep.contains(&checkpoint.batch)
            {
                continue;
            }
            match std::fs::remove_file(&checkpoint.path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("CAMPAIGN_PRUNE_IO: {}: {e}", checkpoint.path)),
            }
            store.mark_campaign_checkpoint_pruned(
                self.id,
                &checkpoint.checkpoint,
                (self.clock)(),
            )?;
            pruned.push(checkpoint.checkpoint);
        }
        Ok(pruned)
    }

    fn pause(&self, code: &str, detail: &str) -> Result<(), String> {
        let mut store = self.store()?;
        let record = store.campaign(self.id)?;
        if record.status != "running" {
            return Ok(());
        }
        store.set_campaign_status(
            self.id,
            record.version,
            "paused",
            Some(code),
            Some(detail),
            "paused",
            &json!({"code": code, "detail": detail}),
            (self.clock)(),
        )?;
        Ok(())
    }

    /// One tick: the write lane and `prepare_parallel` prep workers loop on
    /// their own until all are idle, the campaign stops, or out of time.
    /// Holds the campaign lease throughout.
    pub fn tick(&self, max_seconds: u64) -> Result<TickReport, String> {
        let mut store = self.store()?;
        let lease = store.acquire_lease(&Resource::Campaign(self.id), LEASE_SECONDS)?;
        let deadline = (self.clock)().saturating_add(max_seconds.saturating_mul(1000));
        let workers = self.definition.spec.limits.prepare_parallel.clamp(1, 8) as usize;
        let shared = Shared {
            stop: Mutex::new(None),
            steps: Mutex::new(Vec::new()),
            idle: Mutex::new(vec![false; workers + 1]),
        };
        let claims = Mutex::new(BTreeSet::new());
        let write = || self.write_lane();
        let prep = || self.prep_lane(&claims);
        // A panicking lane stops the others (as a killed process would).
        let guarded =
            |lane: usize, step: &(dyn Fn() -> Result<Option<(u32, Step)>, String> + Sync)| {
                let run =
                    std::panic::AssertUnwindSafe(|| self.lane_loop(lane, &shared, deadline, step));
                if let Err(panic) = std::panic::catch_unwind(run) {
                    shared
                        .stop
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get_or_insert("error: CAMPAIGN_LANE_PANICKED".into());
                    std::panic::resume_unwind(panic);
                }
            };
        let renewed = std::thread::scope(|scope| {
            let mut handles = vec![scope.spawn(|| guarded(0, &write))];
            for lane in 1..=workers {
                let (guarded, prep) = (&guarded, &prep);
                handles.push(scope.spawn(move || guarded(lane, prep)));
            }
            let mut renewed = Ok(());
            while !handles.iter().all(|h| h.is_finished()) {
                std::thread::sleep(self.poll.min(std::time::Duration::from_secs(5)));
                if renewed.is_ok() {
                    renewed = store.renew_lease(&lease, LEASE_SECONDS);
                    if let Err(e) = &renewed {
                        shared
                            .stop
                            .lock()
                            .unwrap()
                            .get_or_insert(format!("error: {e}"));
                    }
                }
            }
            let panicked = handles.into_iter().any(|h| h.join().is_err());
            renewed.and(if panicked {
                Err("CAMPAIGN_LANE_PANICKED".to_owned())
            } else {
                Ok(())
            })
        });
        let result = renewed.and_then(|()| {
            let stopped = shared.stop.into_inner().unwrap().unwrap_or("idle".into());
            match stopped.strip_prefix("error: ") {
                Some(error) => Err(error.to_owned()),
                None => Ok(stopped),
            }
        });
        let pruned = self
            .retention()
            .unwrap_or_else(|e| vec![format!("error: {e}")]);
        let released = store.release_lease(&lease);
        let stopped = result?;
        released?;
        Ok(TickReport {
            campaign: self.id,
            steps: shared.steps.into_inner().unwrap(),
            stopped,
            pruned,
        })
    }

    // ------------------------------------------------------------ commands

    fn batch_record(&self, batch: u32) -> Result<BatchRecord, String> {
        self.store()?.campaign_batch(self.id, batch)
    }

    /// `campaign approve BATCH --actor A`: bind, validate and approve the
    /// batch's settled items. Refused when the agent decided too many.
    pub fn approve(&self, batch: u32, actor: &str) -> Result<Value, String> {
        let record = self.batch_record(batch)?;
        if record.phase != "ready" {
            return Err(format!(
                "CAMPAIGN_BATCH_NOT_READY: batch {batch} is {}",
                record.phase
            ));
        }
        let mut data = BatchData::from_value(&record.data)?;
        let plan_id = data.plan.ok_or("CAMPAIGN_PLAN_MISSING")?.to_string();
        let plan = self.plan_show(&data, false)?;
        let policy_actor = self.definition.spec.policy().actor;
        let mut agent_decided = BTreeSet::new();
        for document in plan["documents"].as_array().into_iter().flatten() {
            let id = document["id"].as_str().unwrap_or_default();
            let Some(note) = self.note_of(&data, id) else {
                continue;
            };
            if data.deferred.contains_key(&note) {
                continue;
            }
            if document["reviews"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|r| r["actor"].as_str().is_some_and(|a| a != policy_actor))
            {
                agent_decided.insert(note);
            }
        }
        let active = data.active_notes().count().max(1);
        let share = agent_decided.len() as f64 / active as f64;
        if share > self.definition.spec.limits.agent_share_max && !data.pilot {
            self.pause(
                "CAMPAIGN_PAUSED_AGENT_SHARE",
                &format!(
                    "batch {batch}: {} of {active} notes were decided by the agent",
                    agent_decided.len()
                ),
            )?;
            return Err(format!(
                "CAMPAIGN_PAUSED_AGENT_SHARE: {:.0}% of batch {batch} was decided by the agent (limit {:.0}%)",
                share * 100.0,
                self.definition.spec.limits.agent_share_max * 100.0
            ));
        }
        let head = self.plan_show(&data, true)?;
        let digest = head["digest"].as_str().unwrap_or_default().to_owned();
        let bound = self.cli(&data, s(&["plans", "bind", &plan_id, "--digest", &digest]))?;
        if bound.json.is_null() {
            return Err(bound.error());
        }
        let head = self.plan_show(&data, true)?;
        let revision = head["revision"].as_u64().ok_or("CAMPAIGN_PLAN_INVALID")? as u32;
        let digest = head["digest"].as_str().unwrap_or_default().to_owned();
        let rev = revision.to_string();
        let validated = self.cli(
            &data,
            s(&["plans", "validate", &plan_id, "--revision", &rev]),
        )?;
        if validated.json.is_null() {
            return Err(validated.error());
        }
        let plan = self.plan_show(&data, false)?;
        let in_group: BTreeMap<String, Uuid> = plan["grammar_groups"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|g| {
                let id = Uuid::parse_str(g["id"].as_str().unwrap_or_default()).unwrap_or_default();
                g["units"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .chain([g["anchor_document"].clone()])
                    .filter_map(|u| u.as_str().map(str::to_owned))
                    .map(move |u| (u, id))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut warnings = BTreeSet::new();
        let mut items = Vec::new();
        let mut approved_docs = Vec::new();
        let mut groups = BTreeSet::new();
        let mut notes = BTreeSet::new();
        for item in validated.json["evidence"]["items"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let document = item["document_id"].as_str().unwrap_or_default();
            let note = self.note_of(&data, document);
            if note.as_ref().is_some_and(|n| data.deferred.contains_key(n)) {
                continue;
            }
            let errors: Vec<&str> = item["issues"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|i| i["severity"] == "error" || i["severity"] == "review")
                .filter_map(|i| i["code"].as_str())
                .collect();
            if !errors.is_empty() {
                if let Some(note) = &note {
                    data.deferred
                        .insert(note.clone(), format!("VALIDATION: {}", errors.join(", ")));
                }
                continue;
            }
            for issue in item["issues"].as_array().into_iter().flatten() {
                if issue["severity"] == "warning"
                    && let Some(code) = issue["code"].as_str()
                {
                    warnings.insert(code.to_owned());
                }
            }
            let id = Uuid::parse_str(document).map_err(|_| "CAMPAIGN_PLAN_INVALID")?;
            approved_docs.push(id);
            match in_group.get(document) {
                Some(group) => {
                    groups.insert(*group);
                }
                None => items.push(id),
            }
            if let Some(note) = note {
                notes.insert(note);
            }
        }
        // A split group is approved whole, or not at all.
        let deferred_docs: BTreeSet<String> = data
            .deferred
            .keys()
            .filter_map(|n| data.documents.get(n).map(|d| d.to_string()))
            .collect();
        groups.retain(|g| {
            !in_group
                .iter()
                .any(|(doc, group)| group == g && deferred_docs.contains(doc))
        });
        approved_docs.retain(|d| {
            in_group
                .get(&d.to_string())
                .is_none_or(|g| groups.contains(g))
        });
        if approved_docs.is_empty() {
            // Every note deferred: nothing to write, and the batch is finished
            // so `campaign requeue` can send its notes round again (WP-24: a
            // batch whose whole sample failed sat in `ready` for good).
            if !data.notes.is_empty() && data.notes.iter().all(|n| data.deferred.contains_key(n)) {
                self.save(
                    &record,
                    "done",
                    &data,
                    json!({"approval": "every note deferred"}),
                )?;
                return Ok(json!({"batch": batch, "phase": "done", "approval": null}));
            }
            return Err(format!("CAMPAIGN_NOTHING_TO_APPROVE: batch {batch}"));
        }
        let mut args = s(&[
            "plans",
            "approve",
            &plan_id,
            "--revision",
            &rev,
            "--digest",
            &digest,
            "--actor",
            actor,
        ]);
        for warning in &warnings {
            args.extend(["--accept-warning".into(), warning.clone()]);
        }
        for item in &approved_docs {
            args.extend(["--item-id".into(), item.to_string()]);
        }
        let approved = self.cli(&data, args)?;
        if approved.code != 0 {
            return Err(approved.error());
        }
        data.approval = Some(Approval {
            revision,
            digest,
            actor: actor.into(),
            items,
            groups: groups.into_iter().collect(),
            notes: notes.into_iter().collect(),
            agent_decided: agent_decided.into_iter().collect(),
        });
        self.save(&record, "approved", &data, json!({"approved_by": actor}))?;
        Ok(
            json!({"batch": batch, "phase": "approved", "approval": data.approval, "deferred": data.deferred}),
        )
    }

    /// `campaign item BATCH NOTE --settled|--defer REASON|--undefer`.
    pub fn item(&self, batch: u32, note: &str, action: ItemAction) -> Result<Value, String> {
        let record = self.batch_record(batch)?;
        if !matches!(record.phase.as_str(), "reviewing" | "needs_agent" | "ready") {
            return Err(format!(
                "CAMPAIGN_BATCH_LOCKED: batch {batch} is {}",
                record.phase
            ));
        }
        let mut data = BatchData::from_value(&record.data)?;
        if !data.notes.iter().any(|n| n == note) && !data.merge.iter().any(|n| n == note) {
            return Err("CAMPAIGN_NOTE_NOT_IN_BATCH".into());
        }
        match &action {
            ItemAction::Settled => {
                if !data.settled.iter().any(|n| n == note) {
                    data.settled.push(note.into());
                }
            }
            ItemAction::Defer(reason) => {
                if data.merge.iter().any(|n| n == note)
                    || (!data.merge.is_empty() && data.notes[0] == note)
                {
                    return Err("CAMPAIGN_MERGE_NOT_DEFERRABLE: defer the whole merge batch with `campaign skip`".into());
                }
                data.deferred
                    .insert(note.into(), format!("AGENT: {reason}"));
            }
            ItemAction::Undefer => {
                data.deferred.remove(note);
            }
        }
        data.needs.remove(note);
        data.checked_revision = None;
        // Back to review so the tick rechecks the batch.
        self.save(
            &record,
            "needs_agent",
            &data,
            json!({"item": note, "action": format!("{action:?}")}),
        )?;
        Ok(json!({"batch": batch, "note": note, "phase": "needs_agent"}))
    }

    /// `campaign reopen BATCH --reason R`: send an approved batch back to
    /// review before any apply job exists for it. Deferrals the approval
    /// made (`VALIDATION:`) are cleared; the next tick reviews it again and
    /// a new approval is needed.
    pub fn reopen(&self, batch: u32, reason: &str, fresh: bool) -> Result<Value, String> {
        let record = self.batch_record(batch)?;
        let mut data = BatchData::from_value(&record.data)?;
        // --fresh: prepare the batch again from scratch (a new prepare job
        // and plan with the current settings, for example a stronger model);
        // nothing was written, so every note returns, deferred ones too.
        if fresh {
            if !matches!(record.phase.as_str(), "ready" | "needs_agent" | "approved")
                || !data.apply_jobs.is_empty()
                || data.apply_intent_ms.is_some()
            {
                return Err(format!(
                    "CAMPAIGN_BATCH_LOCKED: batch {batch} is {} (only a ready, needs_agent or approved batch without an apply job is prepared again)",
                    record.phase
                ));
            }
            let again = BatchData {
                source: data.source.clone(),
                notes: data.notes.clone(),
                merge: data.merge.clone(),
                pilot: data.pilot,
                split_candidates: data.split_candidates.clone(),
                ..BatchData::default()
            };
            self.save(
                &record,
                "selected",
                &again,
                json!({"reopen": reason, "fresh": true, "old_plan": data.plan}),
            )?;
            return Ok(json!({"batch": batch, "phase": "selected", "notes": again.notes.len()}));
        }
        if record.phase != "approved"
            || !data.apply_jobs.is_empty()
            || data.apply_intent_ms.is_some()
        {
            return Err(format!(
                "CAMPAIGN_BATCH_LOCKED: batch {batch} is {} (only an approved batch without an apply job reopens)",
                record.phase
            ));
        }
        data.approval = None;
        data.before.clear();
        data.deferred
            .retain(|_, why| !why.starts_with("VALIDATION:"));
        data.checked_revision = None;
        self.save(&record, "reviewing", &data, json!({"reopen": reason}))?;
        Ok(json!({"batch": batch, "phase": "reviewing", "deferred": data.deferred}))
    }

    /// `campaign skip BATCH --reason R`: leave a whole batch out (only
    /// before approval).
    pub fn skip(&self, batch: u32, reason: &str) -> Result<Value, String> {
        let record = self.batch_record(batch)?;
        if !matches!(
            record.phase.as_str(),
            "selected" | "preparing" | "reviewing" | "needs_agent" | "ready"
        ) {
            return Err(format!(
                "CAMPAIGN_BATCH_LOCKED: batch {batch} is {}",
                record.phase
            ));
        }
        let mut data = BatchData::from_value(&record.data)?;
        data.last_error = Some(format!("SKIPPED: {reason}"));
        self.save(&record, "deferred", &data, json!({"skip": reason}))?;
        Ok(json!({"batch": batch, "phase": "deferred"}))
    }
}

#[derive(Clone, Debug)]
pub enum ItemAction {
    Settled,
    Defer(String),
    Undefer,
}

/// Agent-facing status: counts, what needs the agent or the user, and the
/// commands that move it on.
/// `campaign requeue`: notes a finished batch deferred for one of
/// `reasons` (reason prefixes such as `AGENT:`) go round again in a new
/// `selected` batch per source batch, prepared from scratch with the current
/// generation settings. The finished batch keeps them as deferred with the
/// new batch named, so a note is requeued at most once per pass.
pub fn requeue(store: &mut Store, id: Uuid, reasons: &[String], now: u64) -> Result<Value, String> {
    let campaign = store.campaign(id)?;
    if matches!(campaign.status.as_str(), "cancelled" | "closed") {
        return Err("CAMPAIGN_FINISHED".into());
    }
    // No tick runs meanwhile: the new batch number is named in the old one.
    let lease = store.acquire_lease(&Resource::Campaign(id), LEASE_SECONDS)?;
    let result = requeue_locked(store, id, reasons, now);
    let released = store.release_lease(&lease);
    let value = result?;
    released?;
    Ok(value)
}

fn requeue_locked(
    store: &mut Store,
    id: Uuid,
    reasons: &[String],
    now: u64,
) -> Result<Value, String> {
    let mut created = Vec::new();
    for record in store.campaign_batches(id)? {
        if record.phase != "done" {
            continue;
        }
        let mut data = BatchData::from_value(&record.data)?;
        if !data.merge.is_empty() {
            continue;
        }
        let notes: Vec<String> = data
            .deferred
            .iter()
            .filter(|(_, why)| reasons.iter().any(|r| why.starts_with(r.as_str())))
            .map(|(note, _)| note.clone())
            .collect();
        if notes.is_empty() {
            continue;
        }
        let fresh = BatchData {
            source: data.source.clone(),
            split_candidates: data
                .split_candidates
                .iter()
                .filter(|n| notes.contains(n))
                .cloned()
                .collect(),
            notes: notes.clone(),
            ..BatchData::default()
        };
        let next = store
            .campaign_batches(id)?
            .iter()
            .map(|b| b.batch)
            .max()
            .unwrap_or(0)
            + 1;
        for note in &notes {
            let why = data.deferred[note].clone();
            data.deferred
                .insert(note.clone(), format!("REQUEUED to batch {next}: {why}"));
        }
        let batch = store.move_to_new_batch(
            id,
            record.batch,
            record.version,
            ("done", &data.to_value()),
            ("selected", &fresh.to_value()),
            "requeued",
            &json!({"notes": notes, "reasons": reasons}),
            now,
        )?;
        created.push(json!({"from": record.batch, "batch": batch, "notes": notes.len()}));
    }
    Ok(json!({"campaign": id, "requeued": created}))
}

pub fn status(store: &Store, id: Uuid, now: u64) -> Result<Value, String> {
    let campaign = store.campaign(id)?;
    let definition = Definition::effective(store, id)?;
    let batches = store.campaign_batches(id)?;
    let mut phases: BTreeMap<String, usize> = BTreeMap::new();
    let mut notes_by_phase: BTreeMap<String, usize> = BTreeMap::new();
    let mut agent = Vec::new();
    let mut applied = 0usize;
    let mut deferred = 0usize;
    for record in &batches {
        *phases.entry(record.phase.clone()).or_default() += 1;
        let data = BatchData::from_value(&record.data)?;
        *notes_by_phase.entry(record.phase.clone()).or_default() +=
            data.notes.len() + data.merge.len();
        applied += data.committed.len();
        // A requeued note counts in the batch it went round again in.
        deferred += data
            .deferred
            .values()
            .filter(|why| !why.starts_with("REQUEUED"))
            .count();
        match record.phase.as_str() {
            "needs_agent" => agent.push(json!({
                "batch": record.batch, "cohort": record.cohort, "phase": record.phase,
                "plan": data.plan, "needs": data.needs,
                "commands": [
                    format!("linguist-anki-bridge plans show {} --issues-only", data.plan.map(|p| p.to_string()).unwrap_or_default()),
                    format!("linguist-anki-bridge campaign item {id} {} NOTE --settled|--defer REASON", record.batch),
                ],
            })),
            "ready" => {
                let share = definition.spec.limits.spot_check_share;
                let sample: Vec<&String> = {
                    let active: Vec<&String> = data.active_notes().collect();
                    let n = ((active.len() as f64) * share).ceil().max(1.0) as usize;
                    let step = (active.len() / n.max(1)).max(1);
                    active.into_iter().step_by(step).take(n).collect()
                };
                agent.push(json!({
                    "batch": record.batch, "cohort": record.cohort, "phase": "ready",
                    "plan": data.plan, "spot_check_notes": sample,
                    "commands": [format!("linguist-anki-bridge campaign approve {id} {} --actor claude-agent", record.batch)],
                }));
            }
            _ => {}
        }
        if record.data["last_error"].is_string()
            && record.data["step_failures"].as_u64().unwrap_or(0) > 0
            && !matches!(record.phase.as_str(), "done" | "deferred" | "skipped")
        {
            agent.push(json!({"batch": record.batch, "phase": record.phase, "error": record.data["last_error"], "failures": record.data["step_failures"]}));
        }
    }
    let day = now - now % 86_400_000;
    let runs_today = store.count_campaign_events(id, "agent_run", day)?;
    let user_needed = campaign.status == "paused";
    let agent_active = match store.last_campaign_event(id, "agent_run")? {
        Some(start) => {
            !store
                .last_campaign_event(id, "agent_done")?
                .is_some_and(|done| done.sequence > start.sequence)
                && now.saturating_sub(start.at_ms) < super::AGENT_HOLD_MS
        }
        None => false,
    };
    let finished = batches
        .iter()
        .all(|b| matches!(b.phase.as_str(), "done" | "deferred" | "skipped"));
    Ok(json!({
        "schema_version": 1,
        "campaign": id,
        "name": definition.spec.name,
        "status": campaign.status,
        "code": campaign.code,
        "detail": campaign.detail,
        "batches": batches.len(),
        "phases": phases,
        "notes_by_phase": notes_by_phase,
        "applied_notes": applied,
        "deferred_notes": deferred,
        "finished": finished,
        "agent_needed": campaign.status == "running" && !agent.is_empty(),
        "agent": agent,
        "agent_runs_today": runs_today,
        "agent_runs_per_day": definition.spec.limits.agent_runs_per_day,
        "agent_active": agent_active,
        "limits": definition.spec.limits,
        "user_needed": user_needed,
        "sync_reminder": format!(
            "{applied} applied notes wait for one AnkiWeb full sync: choose Upload when the campaign is closed; review only in this desktop profile until then"
        ),
    }))
}

/// Every need is an open review issue of the plan that the agent has since
/// settled: the batch is reviewed again instead of waiting for nothing (a
/// review that raced the agent's decisions recorded needs already met).
/// Needs that are no issue (a failed draft, a merge to confirm, a split
/// candidate) always wait for the agent.
fn needs_met(data: &BatchData, head: &Value) -> bool {
    let open: BTreeSet<&str> = head["issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["issue"]["id"].as_str())
        .collect();
    let mut any = false;
    for need in data.needs.values().flatten() {
        let Some((issue, _)) = need.split_once(": ") else {
            return false;
        };
        // Issue ids are CODE:subject; the agent's own needs are bare codes.
        if !issue.contains(':') || open.contains(issue) {
            return false;
        }
        any = true;
    }
    any
}

/// The generation chain could not write a missing dictionary entry because
/// its provider was at a usage limit, down or timed out: a retry may succeed.
fn dictionary_outage(document: &Value) -> bool {
    document["issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| i["code"] == "DICTIONARY_ENTRY_GENERATION_FAILED")
        .filter_map(|i| i["message"].as_str())
        .any(|m| {
            crate::agents::usage_limited(m)
                || ["UNAVAILABLE", "AGENT_TIMEOUT", "AGENT_API_HTTP_FAILED"]
                    .iter()
                    .any(|code| m.contains(code))
        })
}

fn meaning_empty(document: &Value) -> bool {
    document["content"]["kind"] == "vocabulary"
        && document["content"]["body"]["meaning"]
            .as_str()
            .is_some_and(|m| m.trim().is_empty())
}

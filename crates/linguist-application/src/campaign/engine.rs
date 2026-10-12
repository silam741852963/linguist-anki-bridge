//! The campaign tick: one step per lane per round, each recorded with
//! compare-and-set before the next. Every step either repeats safely
//! (`jobs run`, `resolve-batch --policy`, `generate --pending`, verification)
//! or records its intent first (apply jobs), so a kill between the effect and
//! the record is recovered by the next tick.
use super::{Approval, BatchData, Definition, SourceSpec, verify};
use linguist_store::{Store, campaign::BatchRecord, lease::Resource};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
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

const PREP_PHASES: [&str; 3] = ["preparing", "reviewing", "needs_agent"];
const WRITE_PHASES: [&str; 4] = ["approved", "applying", "verifying", "merging"];
const LEASE_SECONDS: u64 = 120;

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
        self.save(record, phase, data, json!(what))?;
        Ok(Step::Moved(format!("batch {}: {what}", record.batch)))
    }

    // ------------------------------------------------------------ prep lane

    fn start(&self, record: &BatchRecord, mut data: BatchData) -> Result<Step, String> {
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
        if out.json["plan"].is_null()
            && out.json["captured_this_run"].as_u64() == Some(0)
            && connection_only
        {
            return Ok(Step::Waiting(format!(
                "batch {}: Anki is not reachable",
                record.batch
            )));
        }
        data.prepare_runs += 1;
        let mut plan = out.json["plan"]["id"].as_str().map(str::to_owned);
        let pending = out.json["item_counts"]["pending"].as_u64().unwrap_or(0);
        let failed = out.json["item_counts"]["failed"].as_u64().unwrap_or(0);
        if plan.is_none() && pending == 0 && failed > 0
            || plan.is_none() && data.prepare_runs >= self.definition.spec.limits.max_prepare_runs
        {
            let published = self.cli(&data, s(&["jobs", "publish", &job, "--skip-failed"]))?;
            for item in published.json["skipped"].as_array().into_iter().flatten() {
                if let Some(note) = item["note_id"].as_str() {
                    data.deferred.insert(
                        note.into(),
                        format!(
                            "PREPARE_FAILED: {}",
                            item["error_code"].as_str().unwrap_or("")
                        ),
                    );
                }
            }
            plan = published.json["plan"]["id"].as_str().map(str::to_owned);
            if plan.is_none() && published.code == 5 && pending == 0 {
                return self.moved(record, "skipped", &data, "every note failed to prepare");
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
        if record.phase == "needs_agent" && data.checked_revision == Some(revision) {
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
            // The loop always ends on a policy pass: generation reopens the
            // reviews of every item it drafts.
            if round + 1 == self.definition.spec.limits.max_review_rounds {
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
                ]),
            )?;
            if generated.unavailable() && generated.json.is_null() {
                return Ok(Step::Waiting(generated.error()));
            }
            let fresh = generated.json["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|i| i.get("result").is_some())
                .count();
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
        for note in &data.split_candidates {
            if !data.settled.contains(note) && !data.deferred.contains_key(note) {
                data.needs.entry(note.clone()).or_default().push(
                    "SPLIT_CANDIDATE: split with `plans split-vocab`, or mark it settled".into(),
                );
            }
        }
        let head = self.plan_show(&data, true)?;
        data.checked_revision = head["revision"].as_u64().map(|r| r as u32);
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

    fn prep_lane(&self, batches: &[BatchRecord]) -> Option<(u32, Step)> {
        for record in batches
            .iter()
            .filter(|b| PREP_PHASES.contains(&b.phase.as_str()))
        {
            let step = self.guarded(record);
            if !matches!(step, Step::Waiting(_)) {
                return Some((record.batch, step));
            }
        }
        let in_flight = batches
            .iter()
            .filter(|b| {
                matches!(
                    b.phase.as_str(),
                    "preparing" | "reviewing" | "needs_agent" | "ready" | "approved"
                )
            })
            .count() as u32;
        if in_flight >= self.definition.spec.limits.prepare_ahead {
            return None;
        }
        let record = batches.iter().find(|b| b.phase == "selected")?;
        Some((record.batch, self.guarded(record)))
    }

    fn write_lane(&self, batches: &[BatchRecord]) -> Option<(u32, Step)> {
        let record = batches
            .iter()
            .find(|b| WRITE_PHASES.contains(&b.phase.as_str()))?;
        Some((record.batch, self.guarded(record)))
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

    /// One tick: rounds of (write lane ∥ prep lane) until idle, paused or
    /// out of time. Holds the campaign lease throughout.
    pub fn tick(&self, max_seconds: u64) -> Result<TickReport, String> {
        let mut store = self.store()?;
        let lease = store.acquire_lease(&Resource::Campaign(self.id), LEASE_SECONDS)?;
        let deadline = (self.clock)().saturating_add(max_seconds.saturating_mul(1000));
        let mut steps = Vec::new();
        let result = (|| -> Result<String, String> {
            loop {
                store.renew_lease(&lease, LEASE_SECONDS)?;
                let campaign = store.campaign(self.id)?;
                if campaign.status != "running" {
                    return Ok(campaign.status);
                }
                if (self.clock)() >= deadline {
                    return Ok("deadline".into());
                }
                let batches = store.campaign_batches(self.id)?;
                let (write, prep) = std::thread::scope(|scope| {
                    let write = scope.spawn(|| self.write_lane(&batches));
                    let prep = scope.spawn(|| self.prep_lane(&batches));
                    (write.join(), prep.join())
                });
                let results: Vec<(u32, Step)> = [write, prep]
                    .into_iter()
                    .map(|r| r.map_err(|_| "CAMPAIGN_LANE_PANICKED".to_owned()))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect();
                let mut moved = false;
                for (batch, step) in &results {
                    steps.push(json!({"batch": batch, "step": format!("{step:?}")}));
                    match step {
                        Step::Moved(_) => moved = true,
                        Step::Pause { code, detail } => {
                            self.pause(code, detail)?;
                            return Ok(code.clone());
                        }
                        Step::Waiting(_) => {}
                    }
                }
                // The pilot stops the campaign for the user's review.
                let batches = store.campaign_batches(self.id)?;
                if let Some(pilot) = batches.iter().find(|b| {
                    b.phase == "done" && b.data["pilot"] == true && b.data["pilot_reviewed"] != true
                }) {
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
                    return Ok("CAMPAIGN_PILOT_REVIEW".into());
                }
                if !moved {
                    return Ok("idle".into());
                }
            }
        })();
        let pruned = self
            .retention()
            .unwrap_or_else(|e| vec![format!("error: {e}")]);
        let released = store.release_lease(&lease);
        let stopped = result?;
        released?;
        Ok(TickReport {
            campaign: self.id,
            steps,
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
    pub fn reopen(&self, batch: u32, reason: &str) -> Result<Value, String> {
        let record = self.batch_record(batch)?;
        let mut data = BatchData::from_value(&record.data)?;
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
pub fn status(store: &Store, id: Uuid, now: u64) -> Result<Value, String> {
    let campaign = store.campaign(id)?;
    let definition: Definition = serde_json::from_value(campaign.definition.clone())
        .map_err(|e| format!("CAMPAIGN_RECORD_CORRUPT: {e}"))?;
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
        deferred += data.deferred.len();
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
        "user_needed": user_needed,
        "sync_reminder": format!(
            "{applied} applied notes wait for one AnkiWeb full sync: choose Upload when the campaign is closed; review only in this desktop profile until then"
        ),
    }))
}

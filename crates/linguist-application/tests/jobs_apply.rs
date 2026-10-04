//! WP-13 ALG-JOB over the fake native port: simulate/apply jobs, controls
//! during a sent write, lease liveness, retry budgets, group halts,
//! restart reconciliation without duplicate creation, audit and tombstones.
mod common;
use common::*;
use linguist_application::job_executor::{self, CreateRequest, RunRequest};
use linguist_store::apply_job::{ApplyJobStage, JobControl, RetryClass, StopReason};

fn settings(root: &std::path::Path, overrides: &[(&str, serde_json::Value)]) -> ResolvedSettings {
    let mut values = linguist_config::Registry::builtin().defaults();
    values.insert(
        "storage.state_dir".into(),
        serde_json::json!(root.join("state").to_str().unwrap()),
    );
    values.insert(
        "purposes.japanese_vocab.target_deck".into(),
        serde_json::json!("Japanese::Vocab"),
    );
    for (key, value) in overrides {
        values.insert((*key).into(), value.clone());
    }
    ResolvedSettings {
        semantic_fingerprint: String::new(),
        execution_fingerprint: String::new(),
        version: 2,
        values,
        provenance: BTreeMap::new(),
        resource_hashes: BTreeMap::new(),
        secret_refs: BTreeMap::new(),
        fingerprint: "jobs".into(),
    }
}

fn create(s: &mut Setup, mode: JobMode, overrides: &[(&str, serde_json::Value)]) -> Uuid {
    let mut all = vec![("backup.reuse_max_age_seconds", serde_json::json!(600))];
    all.extend(overrides.iter().cloned());
    let frozen = settings(&s.root, &all);
    let checkpoint = (mode == JobMode::Apply).then_some(s.checkpoint);
    job_executor::create(
        &mut s.store,
        &frozen,
        &CreateRequest {
            mode,
            plan_id: s.plan.id,
            revision: 1,
            digest: s.digest,
            approval_id: s.approval,
            item_ids: vec![],
            checkpoint_id: checkpoint,
            protected_manifest_digest: "protected-v1",
            accept_schema_change: false,
            now_ms: 1_005_000,
        },
    )
    .unwrap()
    .job_id
}

fn run(
    s: &mut Setup,
    port: &mut dyn ApplyPort,
    job: Uuid,
    apply: bool,
) -> Result<job_executor::RunReport, String> {
    let token = s.token.clone();
    job_executor::run(
        &mut s.store,
        Some(&token),
        port,
        &RunRequest {
            job,
            apply,
            now_ms: 1_010_000,
        },
    )
}

fn states(s: &Setup, job: Uuid) -> Vec<String> {
    job_executor::items(&s.store, job, 0, 100)
        .unwrap()
        .into_iter()
        .map(|i| i.state)
        .collect()
}

type MutateHook<'a> = Box<dyn FnMut(&mut Anki, usize) + 'a>;

/// Wraps the fake port with a hook that runs inside a main mutation, while
/// the write is in flight, and optional transient read failures.
struct Hooked<'a> {
    inner: Anki,
    on_mutate: Option<MutateHook<'a>>,
    deck_failures: usize,
    calls: usize,
}

impl<'a> Hooked<'a> {
    fn new(inner: Anki) -> Self {
        Self {
            inner,
            on_mutate: None,
            deck_failures: 0,
            calls: 0,
        }
    }
}

impl ApplyPort for Hooked<'_> {
    fn execution_binding(&mut self) -> Result<CollectionBinding, String> {
        self.inner.execution_binding()
    }
    fn mutation_variants(&mut self) -> Result<Vec<String>, String> {
        self.inner.mutation_variants()
    }
    fn begin(&mut self, b: &CollectionBinding, d: &str) -> Result<OwnerToken, String> {
        self.inner.begin(b, d)
    }
    fn end(&mut self, o: &OwnerToken) -> Result<(), String> {
        self.inner.end(o)
    }
    fn note(&mut self, id: i64) -> Result<Option<ObservedNote>, String> {
        self.inner.note(id)
    }
    fn notes_tagged(&mut self, tag: &str) -> Result<Vec<ObservedNote>, String> {
        self.inner.notes_tagged(tag)
    }
    fn models_named(&mut self, name: &str) -> Result<Vec<ObservedModel>, String> {
        self.inner.models_named(name)
    }
    fn deck(&mut self, name: &str) -> Result<Option<ObservedDeck>, String> {
        if self.deck_failures > 0 {
            self.deck_failures -= 1;
            return Err("ANKI_READ_TIMEOUT".into());
        }
        self.inner.deck(name)
    }
    fn media(&mut self, f: &str) -> Result<Option<ObservedMedia>, String> {
        self.inner.media(f)
    }
    fn media_bytes(&mut self, f: &str, m: u64) -> Result<Option<Vec<u8>>, String> {
        self.inner.media_bytes(f, m)
    }
    fn mutate(&mut self, request: &MutationRequest) -> Result<NativeStatus, PortFailure> {
        self.calls += 1;
        if let Some(hook) = &mut self.on_mutate {
            hook(&mut self.inner, self.calls);
        }
        self.inner.mutate(request)
    }
    fn status(&mut self, id: Uuid) -> Result<NativeStatus, String> {
        self.inner.status(id)
    }
}

fn control_from_other_process(root: &std::path::Path, job: Uuid, action: JobControl) {
    let mut other = Store::open_existing(&root.join("state")).unwrap();
    other.request_apply_job_control(job, action).unwrap();
}

#[test]
fn pause_during_sent_write_finishes_the_item_then_resume_continues_without_duplicates() {
    let mut s = setup_many(3);
    let job = create(&mut s, JobMode::Apply, &[]);
    let root = s.root.clone();
    let mut port = Hooked::new(Anki::new(&s, Kind::Create));
    port.on_mutate = Some(Box::new(move |_, call| {
        if call == 1 {
            control_from_other_process(&root, job, JobControl::Pause);
        }
    }));
    let report = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(report.stop, StopReason::Paused);
    assert_eq!(report.dispatched.len(), 1);
    // The sent write was not cancelled: its durable result is committed.
    assert_eq!(report.dispatched[0].outcome, ItemOutcomeAlias::Committed);
    assert_eq!(states(&s, job), ["committed", "pending", "pending"]);
    assert_eq!(report.status.state, "paused");
    assert!(report.status.worker_stopped_confirmed);
    assert_eq!(marker_notes(&port.inner), 1);
    // Runs while paused dispatch nothing.
    let again = run(&mut s, &mut port, job, true).unwrap();
    assert!(again.dispatched.is_empty());
    assert_eq!(port.inner.main_mutations, 1);

    let first_op = job_executor::items(&s.store, job, 0, 1).unwrap()[0].operation_id;
    port.on_mutate = None;
    job_executor::request_control(&mut s.store, job, JobControl::Resume).unwrap();
    let resumed = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(resumed.stop, StopReason::Idle);
    assert_eq!(states(&s, job), ["committed", "committed", "committed"]);
    assert_eq!(resumed.status.state, "completed");
    assert_eq!(marker_notes(&port.inner), 3);
    // Resume never regenerated the plan or replaced the first operation.
    assert_eq!(s.store.latest_revision(s.plan.id).unwrap(), 1);
    assert_eq!(
        job_executor::items(&s.store, job, 0, 1).unwrap()[0].operation_id,
        first_op
    );
    assert_eq!(resumed.exit_code, 0);
}

use linguist_store::apply_job::ItemOutcome as ItemOutcomeAlias;

#[test]
fn cancel_during_sent_write_keeps_the_committed_item_and_is_terminal() {
    let mut s = setup_many(2);
    let job = create(&mut s, JobMode::Apply, &[]);
    let root = s.root.clone();
    let mut port = Hooked::new(Anki::new(&s, Kind::Create));
    port.on_mutate = Some(Box::new(move |_, call| {
        if call == 1 {
            control_from_other_process(&root, job, JobControl::Cancel);
        }
    }));
    let report = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(report.stop, StopReason::Cancelled);
    assert_eq!(states(&s, job), ["committed", "pending"]);
    assert_eq!(report.status.state, "cancelled");
    assert!(report.status.worker_stopped_confirmed);
    // Cancellation is not rollback; nothing was undone.
    assert_eq!(marker_notes(&port.inner), 1);
    let refused = job_executor::request_control(&mut s.store, job, JobControl::Resume).unwrap_err();
    assert_eq!(refused, "APPLY_JOB_CANCEL_IS_TERMINAL");
    let refused = job_executor::request_retry(&mut s.store, job, &[], true);
    assert!(refused.is_err() || refused.unwrap().accepted.is_empty());
    let after = run(&mut s, &mut port, job, true).unwrap();
    assert!(after.dispatched.is_empty());
    assert_eq!(port.inner.main_mutations, 1);
}

#[test]
fn idle_controls_are_confirmed_but_a_held_lease_only_records_the_request() {
    let mut s = setup_many(1);
    let job = create(&mut s, JobMode::Apply, &[]);
    let paused = job_executor::request_control(&mut s.store, job, JobControl::Pause).unwrap();
    assert!(paused.worker_stopped_confirmed);
    assert_eq!(paused.status.state, "paused");
    // Repeating the current request reuses its receipt.
    let again = job_executor::request_control(&mut s.store, job, JobControl::Pause).unwrap();
    assert_eq!(again.control.digest, paused.control.digest);
    assert!(again.acknowledgement.is_none());
    job_executor::request_control(&mut s.store, job, JobControl::Resume).unwrap();

    // A live worker holds the lease: the request is not a confirmed stop.
    let worker = s
        .store
        .acquire_lease(&Resource::JobWorker(job), 60)
        .unwrap();
    let requested = job_executor::request_control(&mut s.store, job, JobControl::Pause).unwrap();
    assert!(!requested.worker_stopped_confirmed);
    assert_eq!(requested.status.state, "pause_requested");
    // The worker's own start is refused inside the transaction.
    let item = s.plan.documents[0].id;
    let blocked = s
        .store
        .append_apply_job_event(
            job,
            ApplyJobStage::Started {
                item_id: item,
                attempt: 1,
                operation_id: Some(Uuid::new_v4()),
                main_step: Some(Uuid::new_v4()),
            },
            &worker,
        )
        .unwrap_err();
    assert_eq!(blocked, "APPLY_JOB_CONTROL_BLOCKS_DISPATCH");
    s.store.release_lease(&worker).unwrap();
}

#[test]
fn an_expired_lease_of_a_live_worker_is_never_reclaimed() {
    let mut s = setup_many(1);
    let job = create(&mut s, JobMode::Apply, &[]);
    // A one-second lease held by this live process.
    let stale = s.store.acquire_lease(&Resource::JobWorker(job), 1).unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    let mut port = Anki::new(&s, Kind::Create);
    let refused = run(&mut s, &mut port, job, true).unwrap_err();
    assert_eq!(refused, "LEASE_HELD_OR_OWNER_UNVERIFIED");
    assert_eq!(port.mutations(), 0);
    // The expired token cannot append progress either.
    let item = s.plan.documents[0].id;
    let fenced = s
        .store
        .append_apply_job_event(
            job,
            ApplyJobStage::Started {
                item_id: item,
                attempt: 1,
                operation_id: None,
                main_step: None,
            },
            &stale,
        )
        .unwrap_err();
    assert_eq!(fenced, "LEASE_STALE_OR_EXPIRED");
    let shown = job_executor::show(&s.store, job).unwrap();
    assert_eq!(shown.worker.lease, "active");
    assert!(shown.worker.expired);
    assert_eq!(shown.worker.owner, "alive");
    assert_eq!(shown.status.event_count, 0);
}

#[test]
fn simulate_never_writes_and_apply_jobs_need_the_current_flag() {
    let mut s = setup_many(2);
    let simulate = create(&mut s, JobMode::Simulate, &[]);
    let mut port = Anki::new(&s, Kind::Create);
    let refused = run(&mut s, &mut port, simulate, true).unwrap_err();
    assert!(refused.starts_with("JOB_MODE_NEVER_WRITES"));
    let report = job_executor::run(
        &mut s.store,
        None,
        &mut port,
        &RunRequest {
            job: simulate,
            apply: false,
            now_ms: 1_010_000,
        },
    )
    .unwrap();
    assert_eq!(states(&s, simulate), ["simulated", "simulated"]);
    assert!(!report.writes_enabled);
    let simulated = report.dispatched[0].simulated.as_ref().unwrap();
    assert!(!simulated.checkpoint_checked);
    assert_eq!(port.mutations(), 0);
    assert_eq!(port.owners, 0);
    for doc in &s.plan.documents {
        assert!(
            s.store
                .apply_operations_for_item(s.plan.id, doc.id)
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(s.store.pending_journal_count().unwrap(), 0);

    let apply = create(&mut s, JobMode::Apply, &[]);
    let refused = run(&mut s, &mut port, apply, false).unwrap_err();
    assert!(refused.starts_with("APPLY_FLAG_REQUIRED"));
    let refused = job_executor::run(
        &mut s.store,
        None,
        &mut port,
        &RunRequest {
            job: apply,
            apply: true,
            now_ms: 1_010_000,
        },
    )
    .unwrap_err();
    assert_eq!(refused, "APPLY_WRITER_LEASE_REQUIRED");
    assert_eq!(
        job_executor::show(&s.store, apply)
            .unwrap()
            .status
            .event_count,
        0
    );
    assert_eq!(port.mutations(), 0);
}

#[test]
fn apply_job_creation_requires_a_checkpoint_and_approved_ready_items() {
    let mut s = setup_many(1);
    let frozen = settings(&s.root, &[]);
    let mut request = CreateRequest {
        mode: JobMode::Apply,
        plan_id: s.plan.id,
        revision: 1,
        digest: s.digest,
        approval_id: s.approval,
        item_ids: vec![],
        checkpoint_id: None,
        protected_manifest_digest: "protected-v1",
        accept_schema_change: false,
        now_ms: 1,
    };
    let refused = job_executor::create(&mut s.store, &frozen, &request).unwrap_err();
    assert!(refused.starts_with("JOB_CHECKPOINT_REQUIRED"));
    request.checkpoint_id = Some(s.checkpoint);
    request.digest = "f".repeat(64).leak();
    assert!(
        job_executor::create(&mut s.store, &frozen, &request)
            .unwrap_err()
            .starts_with("APPLY_DIGEST_MISMATCH")
    );
    request.digest = s.digest;
    request.item_ids = vec![Uuid::new_v4()];
    assert!(
        job_executor::create(&mut s.store, &frozen, &request)
            .unwrap_err()
            .starts_with("APPLY_ITEM_NOT_FOUND")
    );
    request.mode = JobMode::Prepare;
    assert!(
        job_executor::create(&mut s.store, &frozen, &request)
            .unwrap_err()
            .starts_with("JOB_MODE_INVALID")
    );
    // Created jobs freeze the exact settings and plan item order.
    let job = create(
        &mut s,
        JobMode::Apply,
        &[("jobs.max_item_attempts", serde_json::json!(4))],
    );
    let def = s.store.apply_job(job).unwrap();
    assert_eq!(def.job.item_ids, vec![s.plan.documents[0].id]);
    assert_eq!(def.job.settings.values["jobs.max_item_attempts"], 4);
    assert_eq!(def.max_attempts(), 4);
}

#[test]
fn transient_failures_are_retried_only_by_explicit_envelopes_within_the_budget() {
    let mut s = setup_many(3);
    let job = create(
        &mut s,
        JobMode::Apply,
        &[("jobs.max_item_attempts", serde_json::json!(2))],
    );
    let mut port = Hooked::new(Anki::new(&s, Kind::Create));
    port.deck_failures = 1;
    let report = run(&mut s, &mut port, job, true).unwrap();
    // Independent items continue past an ordinary item fault.
    assert_eq!(report.stop, StopReason::Idle);
    assert_eq!(states(&s, job), ["failed", "committed", "committed"]);
    assert_eq!(report.dispatched[0].retry, RetryClass::Transient);
    assert_eq!(
        report.dispatched[0].code.as_deref(),
        Some("ANKI_READ_TIMEOUT")
    );
    assert_eq!(report.exit_code, 4);
    // A plain rerun does not retry failures automatically.
    let rerun = run(&mut s, &mut port, job, true).unwrap();
    assert!(rerun.dispatched.is_empty());

    let first = s.plan.documents[0].id;
    let committed = s.plan.documents[1].id;
    let retry = job_executor::request_retry(&mut s.store, job, &[first, committed], false).unwrap();
    assert_eq!(retry.accepted, vec![first]);
    assert_eq!(retry.refused[0].code, "JOB_RETRY_ALREADY_SUCCEEDED");
    port.deck_failures = 1;
    let second = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(second.dispatched.len(), 1);
    assert_eq!(second.dispatched[0].attempt, 2);
    let item = &job_executor::items(&s.store, job, 0, 1).unwrap()[0];
    assert_eq!(item.attempt, 2);
    assert!(!item.retry_eligible);
    let exhausted = job_executor::request_retry(&mut s.store, job, &[], true).unwrap();
    assert!(exhausted.accepted.is_empty());
    assert_eq!(exhausted.refused[0].code, "JOB_RETRY_ATTEMPTS_EXHAUSTED");
    assert_eq!(marker_notes(&port.inner), 2);
}

#[test]
fn permanent_failures_need_a_new_revision_and_the_stop_policy_stops_the_group() {
    let mut s = setup_many(3);
    let job = create(
        &mut s,
        JobMode::Apply,
        &[("jobs.on_item_error", serde_json::json!("stop"))],
    );
    let mut port = Hooked::new(Anki::new(&s, Kind::Create));
    // The target deck is gone: every item is refused before any write.
    port.inner.decks.retain(|d| d.name != "Japanese::Vocab");
    let report = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(report.stop, StopReason::ItemErrorPolicy);
    assert_eq!(states(&s, job), ["failed", "pending", "pending"]);
    assert_eq!(report.dispatched[0].retry, RetryClass::Permanent);
    let retry = job_executor::request_retry(&mut s.store, job, &[], true).unwrap();
    assert!(retry.accepted.is_empty());
    assert_eq!(retry.refused[0].code, "JOB_RETRY_REQUIRES_NEW_REVISION");
    assert_eq!(port.inner.mutations(), 0);
}

#[test]
fn an_identity_fault_halts_the_group_and_the_audit_reports_a_partial_batch() {
    let mut s = setup_many(3);
    let job = create(&mut s, JobMode::Apply, &[]);
    let mut port = Hooked::new(Anki::new(&s, Kind::Create));
    port.on_mutate = Some(Box::new(|anki, call| {
        if call == 1 {
            // The collection is replaced after the first write is accepted.
            anki.binding.lineage_id = Uuid::from_u128(99);
        }
    }));
    let report = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(report.stop, StopReason::Halted);
    assert_eq!(report.exit_code, 5);
    let states = states(&s, job);
    assert_eq!(states[2], "pending", "the halt prevents later dispatch");
    assert_eq!(states[1], "failed");
    assert_eq!(report.dispatched.last().unwrap().retry, RetryClass::Halt);
    assert!(
        report
            .stop_code
            .as_deref()
            .unwrap()
            .starts_with("APPLY_IDENTITY_MISMATCH")
    );
    assert_eq!(report.status.state, "halted");

    let audit = job_executor::audit(&s.store, job).unwrap();
    assert!(audit.partial);
    assert!(audit.plan_verified && audit.approval_verified);
    assert!(audit.issues.is_empty(), "{:?}", audit.issues);
    assert!(!audit.native_verified);
    assert_eq!(
        audit.items[0].journal_state,
        Some(OperationState::Committed)
    );
    assert!(audit.items[0].receipt_digest.is_some());
    assert_eq!(audit.items[2].state, "pending");
    assert!(audit.events_verified >= 4);
}

#[test]
fn an_unknown_outcome_stops_dispatch_and_a_restart_reconciles_without_duplicates() {
    let mut s = setup_many(3);
    let job = create(&mut s, JobMode::Apply, &[]);
    let mut port = Anki::new(&s, Kind::Create);
    port.skip = 1;
    port.fault = Fault::LoseAfterEffect;
    let report = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(report.stop, StopReason::RecoveryRequired);
    assert_eq!(report.exit_code, 7);
    assert_eq!(states(&s, job), ["committed", "needs_recovery", "pending"]);
    assert_eq!(report.dispatched[1].retry, RetryClass::Reconcile);
    let retry = job_executor::request_retry(&mut s.store, job, &[], true).unwrap();
    assert_eq!(retry.refused[0].code, "JOB_RETRY_RECONCILE_FIRST");
    let unknown_op = job_executor::items(&s.store, job, 1, 1).unwrap()[0].operation_id;
    assert_eq!(marker_notes(&port), 2);

    let restarted = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(restarted.reconciled.len(), 1);
    assert_eq!(restarted.reconciled[0].outcome, ItemOutcomeAlias::Committed);
    assert_eq!(states(&s, job), ["committed", "committed", "committed"]);
    // The unknown write was adopted under its original operation, not resent.
    let item = &job_executor::items(&s.store, job, 1, 1).unwrap()[0];
    assert_eq!(item.operation_id, unknown_op);
    assert_eq!(item.operations.len(), 1);
    assert_eq!(marker_notes(&port), 3);
    assert_eq!(port.main_mutations, 3);
}

#[test]
fn a_worker_lost_mid_item_is_reconciled_from_its_recorded_operation() {
    let mut s = setup_many(2);
    let job = create(&mut s, JobMode::Apply, &[]);
    let mut port = Anki::new(&s, Kind::Create);
    let first = s.plan.documents[0].id;
    let second = s.plan.documents[1].id;
    let def = s.store.apply_job(job).unwrap();
    // A previous worker recorded both starts, sent the first write (whose
    // response was lost) and died before recording any outcome.
    let worker = s
        .store
        .acquire_lease(&Resource::JobWorker(job), 60)
        .unwrap();
    let (op, step) = (Uuid::new_v4(), Uuid::new_v4());
    s.store
        .append_apply_job_event(
            job,
            ApplyJobStage::Started {
                item_id: first,
                attempt: 1,
                operation_id: Some(op),
                main_step: Some(step),
            },
            &worker,
        )
        .unwrap();
    port.fault = Fault::LoseAfterEffect;
    let mut request = s.request();
    request.item_id = first;
    request.group_id = Some(job);
    request.checkpoint_id = def.checkpoint_id.unwrap();
    let token = s.token.clone();
    let lost = linguist_application::apply::apply_item_as(
        &mut s.store,
        &token,
        &mut port,
        &request,
        op,
        step,
    )
    .unwrap();
    assert_eq!(lost.state, OperationState::NeedsRecovery);
    s.store
        .append_apply_job_event(
            job,
            ApplyJobStage::Started {
                item_id: second,
                attempt: 1,
                operation_id: Some(Uuid::new_v4()),
                main_step: Some(Uuid::new_v4()),
            },
            &worker,
        )
        .unwrap();
    // Only one unfinished start per item.
    let duplicate = s
        .store
        .append_apply_job_event(
            job,
            ApplyJobStage::Started {
                item_id: second,
                attempt: 1,
                operation_id: None,
                main_step: None,
            },
            &worker,
        )
        .unwrap_err();
    assert_eq!(duplicate, "APPLY_JOB_ITEM_IN_FLIGHT");
    s.store.release_lease(&worker).unwrap();
    assert_eq!(
        job_executor::show(&s.store, job).unwrap().status.state,
        "interrupted"
    );
    // Deleting an interrupted job is refused.
    assert!(
        job_executor::delete(&mut s.store, job, false, 1)
            .unwrap_err()
            .starts_with("JOB_DELETE_NOT_TERMINAL")
    );

    let report = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(report.reconciled.len(), 2);
    assert_eq!(report.reconciled[0].operation_id, Some(op));
    assert_eq!(report.reconciled[0].outcome, ItemOutcomeAlias::Committed);
    // The second start had no intent: proven no effect, dispatched again.
    assert_eq!(
        report.reconciled[1].code.as_deref(),
        Some("JOB_ITEM_INTERRUPTED_BEFORE_INTENT")
    );
    assert_eq!(report.dispatched.len(), 1);
    assert_eq!(report.dispatched[0].attempt, 2);
    assert_eq!(states(&s, job), ["committed", "committed"]);
    assert_eq!(marker_notes(&port), 2);
    assert_eq!(port.main_mutations, 2);
}

#[test]
fn rollback_preview_and_group_rollback_use_the_job_as_group() {
    let mut s = setup_many(2);
    let job = create(&mut s, JobMode::Apply, &[]);
    let mut port = Anki::new(&s, Kind::Create);
    run(&mut s, &mut port, job, true).unwrap();
    let preview = job_executor::rollback_preview(&s.store, job, &[]).unwrap();
    assert_eq!(preview.len(), 2);
    assert!(preview.iter().all(|p| p.state == OperationState::Committed));
    let only = job_executor::rollback_preview(&s.store, job, &[s.plan.documents[1].id]).unwrap();
    assert_eq!(only.len(), 1);
    let live =
        linguist_application::restore::plan_group_rollback(&s.store, &mut port, job).unwrap();
    assert_eq!(live.len(), 2);
    assert!(live.iter().all(|i| i.plan.is_some()));
}

#[test]
fn delete_tombstones_only_terminal_jobs_and_keeps_every_record() {
    let mut s = setup_many(2);
    let job = create(&mut s, JobMode::Apply, &[]);
    let refused = job_executor::delete(&mut s.store, job, true, 1).unwrap_err();
    assert!(refused.starts_with("JOB_DELETE_NOT_TERMINAL"));
    let mut port = Anki::new(&s, Kind::Create);
    run(&mut s, &mut port, job, true).unwrap();
    let preview = job_executor::delete(&mut s.store, job, false, 2).unwrap();
    assert!(!preview.executed);
    assert_eq!(preview.tombstone.final_state, "completed");
    assert_eq!(preview.tombstone.retained_operations.len(), 2);
    assert!(s.store.job_tombstone(job).unwrap().is_none());
    let done = job_executor::delete(&mut s.store, job, true, 2).unwrap();
    assert!(done.executed && !done.anki_deletion);
    assert_eq!(
        job_executor::delete(&mut s.store, job, true, 3).unwrap_err(),
        "JOB_ALREADY_TOMBSTONED"
    );
    assert!(
        run(&mut s, &mut port, job, true)
            .unwrap_err()
            .starts_with("JOB_TOMBSTONED")
    );
    assert_eq!(
        job_executor::request_control(&mut s.store, job, JobControl::Pause).unwrap_err(),
        "JOB_TOMBSTONED"
    );
    // Hidden from the default listing, still inspectable and auditable.
    let listed = s.store.list_jobs(None, 100, None, false).unwrap();
    assert!(listed.iter().all(|j| j.id != job));
    let all = s.store.list_jobs(None, 100, None, true).unwrap();
    assert!(all.iter().any(|j| j.id == job && j.tombstoned));
    let audit = job_executor::audit(&s.store, job).unwrap();
    assert!(audit.issues.is_empty());
    for op in &preview.tombstone.retained_operations {
        let journal = s.store.journal(*op).unwrap().journal;
        assert!(
            s.store
                .snapshot(journal.snapshot_id)
                .unwrap()
                .after
                .is_some()
        );
    }
    assert_eq!(marker_notes(&port), 2);
}

#[test]
fn a_tampered_event_chain_fails_the_audit() {
    let mut s = setup_many(1);
    let job = create(&mut s, JobMode::Apply, &[]);
    let mut port = Anki::new(&s, Kind::Create);
    run(&mut s, &mut port, job, true).unwrap();
    let db = rusqlite::Connection::open(s.root.join("state").join("state.sqlite3")).unwrap();
    db.execute_batch(
        "DROP TRIGGER apply_job_events_no_delete; DELETE FROM apply_job_events WHERE sequence=2;",
    )
    .unwrap();
    assert_eq!(
        job_executor::audit(&s.store, job).unwrap_err(),
        "APPLY_JOB_EVENT_CORRUPT"
    );
}

#[test]
fn a_job_adopts_its_checkpoint_group_so_items_reuse_it_without_an_age_window() {
    let mut s = setup_many(2);
    let group = Uuid::new_v4();
    let checkpoint = checkpoint_for_group(&mut s, &[], &[], 1_004_000, Some(group));
    // Default settings: cross-group reuse is disabled (reuse_max_age_seconds=0).
    let frozen = settings(&s.root, &[]);
    let request = CreateRequest {
        mode: JobMode::Apply,
        plan_id: s.plan.id,
        revision: 1,
        digest: s.digest,
        approval_id: s.approval,
        item_ids: vec![],
        checkpoint_id: Some(checkpoint),
        protected_manifest_digest: "protected-v1",
        accept_schema_change: false,
        now_ms: 1_005_000,
    };
    // Without the group, the same defaults reject reuse and halt before any write.
    let plain = create_with(&mut s, &[]);
    let mut port = Anki::new(&s, Kind::Create);
    let halted = run(&mut s, &mut port, plain, true).unwrap();
    assert_eq!(halted.stop, StopReason::Halted);
    assert_eq!(
        halted.stop_code.as_deref(),
        Some("CHECKPOINT_REUSE_REJECTED")
    );
    assert_eq!(port.mutations(), 0);

    let job = job_executor::create(&mut s.store, &frozen, &request)
        .unwrap()
        .job_id;
    assert_eq!(job, group);
    let report = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(report.status.state, "completed");
    // The group already names a job.
    let refused = job_executor::create(&mut s.store, &frozen, &request).unwrap_err();
    assert!(refused.starts_with("JOB_CHECKPOINT_GROUP_IN_USE"));
}

fn create_with(s: &mut Setup, overrides: &[(&str, serde_json::Value)]) -> Uuid {
    let frozen = settings(&s.root, overrides);
    job_executor::create(
        &mut s.store,
        &frozen,
        &CreateRequest {
            mode: JobMode::Apply,
            plan_id: s.plan.id,
            revision: 1,
            digest: s.digest,
            approval_id: s.approval,
            item_ids: vec![s.plan.documents[0].id],
            checkpoint_id: Some(s.checkpoint),
            protected_manifest_digest: "protected-v1",
            accept_schema_change: false,
            now_ms: 1_005_000,
        },
    )
    .unwrap()
    .job_id
}

#[test]
fn commit_interval_paces_mutations_and_only_the_verified_native_adapter_is_accepted() {
    let mut s = setup_many(3);
    let frozen = settings(
        &s.root,
        &[("anki.native_adapter", serde_json::json!("other-v9"))],
    );
    let request = CreateRequest {
        mode: JobMode::Apply,
        plan_id: s.plan.id,
        revision: 1,
        digest: s.digest,
        approval_id: s.approval,
        item_ids: vec![],
        checkpoint_id: Some(s.checkpoint),
        protected_manifest_digest: "protected-v1",
        accept_schema_change: false,
        now_ms: 1_005_000,
    };
    let refused = job_executor::create(&mut s.store, &frozen, &request).unwrap_err();
    assert!(
        refused.starts_with("NATIVE_ADAPTER_UNSUPPORTED"),
        "{refused}"
    );
    let job = create(
        &mut s,
        JobMode::Apply,
        &[("anki.commit_interval_seconds", serde_json::json!(0.2))],
    );
    let times = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = times.clone();
    let mut port = Hooked::new(Anki::new(&s, Kind::Create));
    port.on_mutate = Some(Box::new(move |_, _| {
        seen.borrow_mut().push(std::time::Instant::now())
    }));
    let report = run(&mut s, &mut port, job, true).unwrap();
    assert_eq!(report.stop, StopReason::Idle);
    let times = times.borrow();
    assert_eq!(times.len(), 3);
    for pair in times.windows(2) {
        assert!(
            pair[1] - pair[0] >= std::time::Duration::from_millis(190),
            "{:?}",
            pair[1] - pair[0]
        );
    }
}

//! WP-24 campaign engine over a fake CLI and companion: the full batch
//! lifecycle, the guardrails, and recovery when the tick is killed after
//! any child command (the effect happened, its record did not).
use linguist_application::campaign::{
    self, BatchData, Definition, Limits, SourceSpec, Spec,
    engine::{Collection, Context, ItemAction, Output, Runner},
    select::{self, NoteInfo},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use uuid::Uuid;

fn source(id: &str) -> SourceSpec {
    SourceSpec {
        id: id.into(),
        query: "deck:x".into(),
        purpose: "japanese_vocab".into(),
        settings: vec!["audio.provider=dictionary".into()],
        target_models: vec!["Linguist Vocabulary v3".into()],
        word_field: Some("Word".into()),
        reading_field: Some("Reading".into()),
        picture_field: Some("Picture".into()),
        audio_field: Some("Audio".into()),
        language: "ja".into(),
        merge_duplicates: true,
        extra_agent_codes: vec![],
    }
}

fn note(id: &str, word: &str, reading: &str, reps: u32) -> NoteInfo {
    NoteInfo {
        id: id.into(),
        model: "2. Picture Words".into(),
        fields: BTreeMap::from([
            ("Word".into(), word.into()),
            ("Reading".into(), reading.into()),
            ("Picture".into(), "<img src=\"a.jpg\">".into()),
            ("Audio".into(), "[sound:a.mp3]".into()),
        ]),
        reps,
    }
}

#[test]
fn cohorts_follow_the_collection_analysis_rules() {
    let s = source("ja");
    let class = |n: &NoteInfo| select::classify(&s, n, false);
    assert_eq!(class(&note("1", "冷房", "れいぼう", 0)), "plain");
    assert_eq!(
        class(&note("2", "朝  ご  飯", "あさごはん", 0)),
        "plain",
        "spaces are formatting"
    );
    assert_eq!(
        class(&note("3", "祖父 お祖父さん", "そふ", 0)),
        "split_candidate"
    );
    assert_eq!(
        class(&note("4", "治る; 直る", "なおる", 0)),
        "split_candidate"
    );
    assert_eq!(
        class(&note("5", "味見(する/を見る)", "あじみ", 0)),
        "split_candidate"
    );
    assert_eq!(class(&note("6", "なぜか", "なぜか", 0)), "kana_only");
    let mut silent = note("7", "神秘", "しんぴ", 0);
    silent.fields.insert("Audio".into(), "しんぴ".into());
    assert_eq!(class(&silent), "missing_media");
    assert_eq!(
        select::text("<span style=\"x\">よ</span>&nbsp;<b>り</b>[sound:x.mp3]"),
        "よ り"
    );
    let notes = [
        note("10", "本", "ほん", 3),
        note("11", "本", "ほん", 9),
        note("12", "本", "もと", 1),
        note("13", "僧侶", "そう りょ", 2),
        note("14", "僧侶", "そうりょ", 2),
    ];
    let groups = select::duplicate_groups(&s, &notes);
    assert_eq!(
        groups,
        vec![
            vec!["13".to_owned(), "14".into()],
            vec!["11".into(), "10".into()]
        ]
    );
}

/// A fake `linguist-anki-bridge`: just enough state to answer each command
/// the engine sends, with every effect counted.
#[derive(Default)]
struct World {
    calls: Vec<String>,
    /// Threads that sent commands (lanes and prep workers).
    threads: BTreeSet<String>,
    prepare_delay_ms: u64,
    next: u64,
    prepare_jobs: BTreeMap<String, Vec<String>>,
    plans: BTreeMap<String, Plan>,
    apply_jobs: BTreeMap<String, (String, Vec<String>, bool)>,
    applied: BTreeMap<String, u32>,
    deleted: BTreeSet<String>,
    checkpoints: Vec<(String, u64)>,
    /// Notes whose sense the policy cannot decide until the agent does.
    hard: BTreeSet<String>,
    agent_decided: BTreeSet<String>,
    /// Drafts made by the next `plans generate` (each reopens its note's reviews).
    drafts: usize,
    /// A prepare job with more notes cannot publish its plan.
    max_notes: Option<usize>,
    /// Notes whose audio synthesis found the service down, and how many
    /// enrichment runs it takes until it is back.
    outage: BTreeMap<String, u32>,
    reenriched: Vec<String>,
    /// Notes whose missing dictionary entry was not written (provider error),
    /// and how many dictionary runs fail the same way.
    dictionary_outage: BTreeMap<String, (u32, String)>,
    redictionaried: Vec<String>,
    /// Notes whose read fails (Anki busy) this many more times.
    read_fail: BTreeMap<String, u32>,
    read_retries: u32,
    /// Retries the prepare job's attempt budget still allows (None: any).
    retry_budget: Option<u32>,
    /// The next `plans generate` calls fail every item with this error.
    draft_fail: Option<(u32, String)>,
    /// An apply that ends needing recovery, then reconcile attempts that
    /// stay unresolved (exit 4) this many times before one resolves it.
    recovery: Option<u32>,
    /// The agent decides the hard notes right after the next policy pass,
    /// while the review step is still running.
    agent_edits_mid_review: bool,
    /// The next policy passes find the companion without its collection
    /// session (Anki reopening the collection): conflict, exit 5.
    no_session: u32,
}

#[derive(Clone, Default)]
struct Plan {
    revision: u32,
    notes: Vec<String>,
    decided: BTreeSet<String>,
    merged: Vec<String>,
}

fn doc(note: &str) -> String {
    format!(
        "00000000-0000-4000-8000-{:012}",
        note.parse::<u64>().unwrap()
    )
}

struct FakeCli {
    world: Mutex<World>,
    /// Panic (a killed tick) after this many child commands.
    kill_after: AtomicU64,
    clock: AtomicU64,
}

impl FakeCli {
    fn answer(&self, args: &[String]) -> Output {
        let mut w = self.world.lock().unwrap();
        w.calls.push(args.join(" "));
        w.next += 1;
        let id = format!("00000000-0000-4000-9000-{:012}", w.next);
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let ok = |json: Value| Output {
            code: 0,
            json,
            stderr: String::new(),
        };
        let plan_view = |p: &Plan, issues_only: bool| {
            let open: Vec<Value> = p
                .notes
                .iter()
                .filter(|n| !p.decided.contains(*n))
                .map(|n| json!({"issue": {"id": format!("DICTIONARY_SENSE_REVIEW:{n}")}, "request_identity": {"document_id": doc(n)}}))
                .collect();
            let mut v = json!({"revision": p.revision, "digest": format!("d{}", p.revision), "issues": open, "grammar_groups": []});
            if !issues_only {
                v["documents"] = p.notes.iter().map(|n| json!({"id": doc(n),
                    "issues": if w.outage.get(n).is_some_and(|left| *left > 0) { json!([{"code": "AUDIO_SYNTHESIS_FAILED", "message": "VOICEVOX_UNAVAILABLE: engine down; no audio candidate was staged. Audio is optional."}]) }
                        else if let Some((left, error)) = w.dictionary_outage.get(n).filter(|(left, _)| *left > 0) { let _ = left; json!([{"code": "DICTIONARY_ENTRY_GENERATION_FAILED", "message": format!("DICTIONARY_ENTRY_FAILED: codex: {error}; author the meaning instead.")}]) }
                        else { json!([]) },
                    "content": {"kind": "vocabulary", "body": {"meaning": if w.dictionary_outage.get(n).is_some_and(|(left, _)| *left > 0) { "" } else { "meaning" }}},
                    "media": [],
                    "sources": [{"location": format!("anki_note:{n}")}],
                    "reviews": if p.decided.contains(n) { json!([{"actor": if w.agent_decided.contains(n) { "claude-agent" } else { "campaign-policy" }}]) } else { json!([]) }})).collect();
            }
            v
        };
        match a.as_slice() {
            ["jobs", "create", "--mode", "prepare", rest @ ..] => {
                let notes: Vec<String> = rest.chunks(2).map(|c| c[1].to_owned()).collect();
                w.prepare_jobs.insert(id.clone(), notes);
                ok(json!({"job_id": id}))
            }
            ["jobs", "run", job] => {
                let notes = w.prepare_jobs[*job].clone();
                if w.max_notes.is_some_and(|max| notes.len() > max) {
                    return Output {
                        code: 2,
                        json: Value::Null,
                        stderr: json!({"error": "REVAMP_BATCH_ARCHIVE_LIMIT"}).to_string(),
                    };
                }
                let failing: Vec<String> = notes
                    .iter()
                    .filter(|n| w.read_fail.get(*n).is_some_and(|left| *left > 0))
                    .cloned()
                    .collect();
                if !failing.is_empty() && !w.plans.contains_key(*job) {
                    return ok(json!({"plan": null, "captured_this_run": 0,
                        "item_counts": {"captured": notes.len() - failing.len(), "failed": failing.len(), "pending": 0},
                        "error_counts": {"SOURCE_READ_CONNECTION_FAILED": failing.len()}}));
                }
                w.plans.entry((*job).to_owned()).or_insert(Plan {
                    revision: 1,
                    notes: notes.clone(),
                    ..Default::default()
                });
                ok(
                    json!({"plan": {"id": job}, "item_counts": {"captured": notes.len()}, "error_counts": {}, "captured_this_run": notes.len()}),
                )
            }
            ["jobs", "retry", _, "--failed"] if w.retry_budget == Some(0) => Output {
                code: 4,
                json: json!({"eligible": [], "refused": [{"code": "JOB_RETRY_ATTEMPTS_EXHAUSTED"}]}),
                stderr: String::new(),
            },
            ["jobs", "retry", _, "--failed"] => {
                if let Some(left) = w.retry_budget.as_mut() {
                    *left -= 1;
                }
                w.read_retries += 1;
                for left in w.read_fail.values_mut() {
                    *left = left.saturating_sub(1);
                }
                ok(json!({"retried": true}))
            }
            ["jobs", "publish", job, "--skip-failed"] => {
                let notes = w.prepare_jobs[*job].clone();
                let (bad, good): (Vec<String>, Vec<String>) = notes
                    .into_iter()
                    .partition(|n| w.read_fail.get(n).is_some_and(|left| *left > 0));
                w.plans.insert(
                    (*job).to_owned(),
                    Plan {
                        revision: 1,
                        notes: good,
                        ..Default::default()
                    },
                );
                ok(json!({"plan": {"id": job},
                    "skipped": bad.iter().map(|n| json!({"note_id": n, "error_code": "SOURCE_READ_CONNECTION_FAILED"})).collect::<Vec<_>>()}))
            }
            ["plans", "show", plan, rest @ ..] => {
                let p = w.plans[*plan].clone();
                ok(plan_view(&p, rest.contains(&"--issues-only")))
            }
            ["plans", "merge-notes", plan, "--item", _, rest @ ..] => {
                let merged: Vec<String> = rest.chunks(2).map(|c| c[1].to_owned()).collect();
                let p = w.plans.get_mut(*plan).unwrap();
                p.merged = merged;
                p.revision += 1;
                ok(json!({"revision": p.revision}))
            }
            ["plans", "resolve-batch", _, "--policy", _] if w.no_session > 0 => {
                w.no_session -= 1;
                Output {
                    code: 5,
                    json: json!({"outcome": {"conflict": {"error": "CAPABILITY_UNAVAILABLE: the companion declares no mutation variants (API key or collection session missing)"}}}),
                    stderr: String::new(),
                }
            }
            ["plans", "resolve-batch", plan, "--policy", _] => {
                let hard = w.hard.clone();
                let p = w.plans.get_mut(*plan).unwrap();
                let todo: Vec<String> = p
                    .notes
                    .iter()
                    .filter(|n| !p.decided.contains(*n) && !hard.contains(*n))
                    .cloned()
                    .collect();
                for n in &todo {
                    p.decided.insert(n.clone());
                }
                if !todo.is_empty() {
                    p.revision += 1;
                }
                let undecided: Vec<Value> = p.notes.iter().filter(|n| !p.decided.contains(*n))
                    .map(|n| json!({"document_id": doc(n), "issue_id": format!("DICTIONARY_SENSE_REVIEW:{n}"), "reason": "SENSE_READING_UNMATCHED"})).collect();
                let ready = undecided.is_empty();
                let revision = p.revision;
                if std::mem::take(&mut w.agent_edits_mid_review) {
                    let p = w.plans.get_mut(*plan).unwrap();
                    for n in &hard {
                        p.decided.insert(n.clone());
                    }
                    p.revision += 1;
                    w.agent_decided.extend(hard.iter().cloned());
                }
                Output {
                    code: if ready { 0 } else { 4 },
                    json: json!({"revision": revision, "ready": ready, "policy": {"decided": todo.len(), "undecided": undecided}}),
                    stderr: String::new(),
                }
            }
            [
                "plans",
                "regenerate",
                plan,
                "--stage",
                "enrichment",
                "--item-id",
                item,
                ..,
            ] => {
                let note = w.plans[*plan]
                    .notes
                    .iter()
                    .find(|n| doc(n) == *item)
                    .cloned()
                    .unwrap();
                w.reenriched.push(note.clone());
                if let Some(left) = w.outage.get_mut(&note) {
                    *left = left.saturating_sub(1);
                }
                w.plans.get_mut(*plan).unwrap().revision += 1;
                ok(json!({"revision": 0}))
            }
            [
                "plans",
                "regenerate",
                plan,
                "--stage",
                "dictionary",
                "--item-id",
                item,
                ..,
            ] => {
                let note = w.plans[*plan]
                    .notes
                    .iter()
                    .find(|n| doc(n) == *item)
                    .cloned()
                    .unwrap();
                w.redictionaried.push(note.clone());
                if let Some((left, _)) = w.dictionary_outage.get_mut(&note) {
                    *left = left.saturating_sub(1);
                }
                w.plans.get_mut(*plan).unwrap().revision += 1;
                ok(json!({"revision": 0}))
            }
            ["plans", "generate", plan, ..] => {
                if let Some((left, error)) = w.draft_fail.clone() {
                    w.draft_fail = (left > 1).then(|| (left - 1, error.clone()));
                    let notes = w.plans[*plan].notes.clone();
                    return ok(
                        json!({"items": notes.iter().map(|n| json!({"document_id": doc(n), "error": error})).collect::<Vec<_>>()}),
                    );
                }
                let drafts = std::mem::take(&mut w.drafts);
                let p = w.plans.get_mut(*plan).unwrap();
                let drafted: Vec<String> = p.notes.iter().take(drafts).cloned().collect();
                for n in &drafted {
                    p.decided.remove(n);
                }
                if !drafted.is_empty() {
                    p.revision += 1;
                }
                ok(
                    json!({"items": drafted.iter().map(|n| json!({"document_id": doc(n), "result": {}})).collect::<Vec<_>>()}),
                )
            }
            ["plans", "bind", plan, ..] => {
                let p = w.plans.get_mut(*plan).unwrap();
                p.revision += 1;
                ok(json!({"revision": p.revision}))
            }
            ["plans", "validate", plan, ..] => {
                let p = &w.plans[*plan];
                ok(
                    json!({"evidence": {"items": p.notes.iter().map(|n| json!({"document_id": doc(n), "issues": [{"severity": "warning", "code": "SCHEMA_CHANGE"}]})).collect::<Vec<_>>()}}),
                )
            }
            ["plans", "approve", ..] => ok(json!({"noop": false})),
            [
                "jobs",
                "create",
                "--mode",
                "apply",
                "--plan",
                plan,
                rest @ ..,
            ] => {
                let items: Vec<String> = rest
                    .windows(2)
                    .filter(|w| w[0] == "--item-id")
                    .map(|w| w[1].to_owned())
                    .collect();
                let now = self.clock.load(Ordering::SeqCst);
                w.checkpoints.push((format!("cp-{id}"), now));
                w.apply_jobs
                    .insert(id.clone(), ((*plan).to_owned(), items, false));
                ok(json!({"job": {"job_id": id}}))
            }
            ["backup", "list", ..] => {
                let list: Vec<Value> = w.checkpoints.iter().map(|(c, t)| json!({"id": c, "path": format!("/nonexistent/{c}.colpkg"), "created_ms": t})).collect();
                ok(json!({"checkpoints": list}))
            }
            ["jobs", verb @ ("run" | "resume"), job, "--apply"] if w.recovery.is_some() => {
                let _ = verb;
                // The write lands once; its outcome is unknown until reconciled.
                let (plan, items, ran) = w.apply_jobs[*job].clone();
                for n in w.plans[&plan]
                    .notes
                    .clone()
                    .iter()
                    .filter(|n| !ran && items.contains(&doc(n)))
                {
                    *w.applied.entry(n.clone()).or_default() += 1;
                }
                w.apply_jobs.get_mut(*job).unwrap().2 = true;
                ok(
                    json!({"run": {"stop": "recovery_required", "stop_code": null, "status": {"counts": {"needs_recovery": 1}}}}),
                )
            }
            ["recover", "inspect", "--pending"] => ok(match w.recovery {
                Some(_) => json!({"journals": [{"journal": {"id": "op-1"}}]}),
                None => json!({"journals": []}),
            }),
            ["recover", "reconcile", "op-1", "--apply"] => match w.recovery {
                Some(left) if left > 0 => {
                    w.recovery = Some(left - 1);
                    Output {
                        code: 4,
                        json: json!({"reconcile": {"state": "needs_recovery"}}),
                        stderr: String::new(),
                    }
                }
                _ => {
                    w.recovery = None;
                    // The write had landed: reconcile adopts it.
                    ok(json!({"reconcile": {"state": "committed"}}))
                }
            },
            ["jobs", verb @ ("run" | "resume"), job, "--apply"] => {
                let _ = verb;
                let (plan, items, ran) = w.apply_jobs[*job].clone();
                let notes = w.plans[&plan].notes.clone();
                // Like the executor: committed items are never dispatched again.
                for n in notes.iter().filter(|n| !ran && items.contains(&doc(n))) {
                    *w.applied.entry(n.clone()).or_default() += 1;
                }
                w.apply_jobs.get_mut(*job).unwrap().2 = true;
                ok(
                    json!({"run": {"stop": "idle", "stop_code": null, "status": {"counts": {"committed": items.len()}}}}),
                )
            }
            ["jobs", "items", job, ..] => {
                let (_, items, _) = w.apply_jobs[*job].clone();
                ok(
                    json!({"items": items.iter().map(|i| json!({"item_id": i, "state": "committed"})).collect::<Vec<_>>()}),
                )
            }
            ["notes", "delete-merged", note, ..] => {
                w.deleted.insert((*note).to_owned());
                ok(json!({"state": "deleted"}))
            }
            other => panic!("unexpected command {other:?}"),
        }
    }
}

impl Runner for FakeCli {
    fn run(&self, source: &SourceSpec, args: &[String]) -> Output {
        assert_eq!(source.purpose, "japanese_vocab");
        // Preparation takes a while, outside the world lock (as a child
        // process would), so parallel prep workers overlap.
        if args.first().is_some_and(|a| a == "jobs") && args.get(1).is_some_and(|a| a == "run") {
            let delay = self.world.lock().unwrap().prepare_delay_ms;
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
        let out = self.answer(args);
        self.world
            .lock()
            .unwrap()
            .threads
            .insert(format!("{:?}", std::thread::current().id()));
        let left = self.kill_after.load(Ordering::SeqCst);
        if left > 0 {
            self.kill_after.store(left - 1, Ordering::SeqCst);
        }
        if left == 1 {
            panic!("tick killed after `{}`", args.join(" "));
        }
        out
    }
}

struct FakeAnki<'a>(&'a FakeCli);
impl Collection for FakeAnki<'_> {
    fn note_evidence(&self, note: &str) -> Result<Option<Value>, String> {
        let w = self.0.world.lock().unwrap();
        if w.deleted.contains(note) {
            return Ok(None);
        }
        let model = if w.applied.contains_key(note) {
            "Linguist Vocabulary v3"
        } else {
            "2. Picture Words"
        };
        Ok(Some(
            json!({"note_id": note, "model": {"name": model}, "media": [],
            "cards": [{"id": format!("{note}1"), "ordinal": 0, "repetitions": 8, "reviews": [{"id": 1}, {"id": 2}]}]}),
        ))
    }
}

fn spec(pilot: bool) -> Spec {
    Spec {
        schema_version: 1,
        name: "test".into(),
        sources: vec![source("ja")],
        order: vec!["ja:plain".into(), "ja:duplicate".into()],
        batch_size: 2,
        batch_sizes: BTreeMap::new(),
        pilot: pilot.then(|| campaign::Pilot {
            cohort: "ja:plain".into(),
            size: 1,
        }),
        merge_extra: vec![],
        limits: Limits {
            max_step_failures: 50,
            ..Limits::default()
        },
        policy: None,
    }
}

fn create(root: &std::path::Path, spec: Spec, notes: Vec<NoteInfo>) -> Uuid {
    spec.validate().unwrap();
    let (batches, selection) =
        select::build(&spec, &BTreeMap::from([("ja".to_owned(), notes)])).unwrap();
    let definition = Definition {
        schema_version: 1,
        spec,
        created_ms: 1,
        global_args: vec![],
        policy_path: "/policy.json".into(),
        selection,
    };
    let id = Uuid::new_v4();
    let new: Vec<_> = batches
        .into_iter()
        .enumerate()
        .map(|(i, (cohort, data))| linguist_store::campaign::NewBatch {
            batch: i as u32 + 1,
            cohort,
            phase: "selected".into(),
            data: data.to_value(),
        })
        .collect();
    linguist_store::Store::open(root)
        .unwrap()
        .create_campaign(id, &serde_json::to_value(&definition).unwrap(), &new, 1)
        .unwrap();
    id
}

fn context<'a>(
    root: &'a std::path::Path,
    id: Uuid,
    cli: &'a FakeCli,
    anki: &'a FakeAnki<'a>,
    clock: &'a (dyn Fn() -> u64 + Sync),
) -> Context<'a> {
    let store = linguist_store::Store::read_only(root).unwrap();
    Context {
        root,
        id,
        definition: linguist_application::campaign::Definition::effective(&store, id).unwrap(),
        runner: cli,
        collection: anki,
        clock,
        poll: std::time::Duration::from_millis(5),
    }
}

fn phases(root: &std::path::Path, id: Uuid) -> Vec<String> {
    linguist_store::Store::read_only(root)
        .unwrap()
        .campaign_batches(id)
        .unwrap()
        .into_iter()
        .map(|b| b.phase)
        .collect()
}

/// Drive the campaign to the end: ticks, then the agent's approvals.
fn drive(root: &std::path::Path, id: Uuid, cli: &FakeCli, kill_every: Option<u64>) -> usize {
    let anki = FakeAnki(cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let mut kills = 0;
    if let Some(k) = kill_every {
        cli.kill_after.store(k, Ordering::SeqCst);
    }
    for _ in 0..400 {
        let ctx = context(root, id, cli, &anki, &clock);
        // A panic in a lane is a killed tick; the lease dies with it.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.tick(600))) {
            Err(_) | Ok(Err(_)) => {
                // A lane panic is caught by the tick, which releases its
                // lease as a dead process's lease would be reclaimed.
                kills += 1;
                continue;
            }
            Ok(Ok(_)) => {}
        }
        let batches = linguist_store::Store::read_only(root)
            .unwrap()
            .campaign_batches(id)
            .unwrap();
        // The agent confirms each merge (same word).
        for b in batches.iter().filter(|b| b.phase == "needs_agent") {
            let data = BatchData::from_value(&b.data).unwrap();
            if !data.merge.is_empty() && !data.settled.contains(&data.notes[0]) {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    ctx.item(b.batch, &data.notes[0], ItemAction::Settled)
                })) {
                    Ok(result) => {
                        result.unwrap();
                    }
                    Err(_) => kills += 1,
                }
            }
        }
        for b in batches.iter().filter(|b| b.phase == "ready") {
            // The agent's approval can be killed midway too; it is repeated.
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ctx.approve(b.batch, "claude-agent")
            })) {
                Ok(result) => {
                    result.unwrap();
                }
                Err(_) => kills += 1,
            }
        }
        let record = linguist_store::Store::read_only(root)
            .unwrap()
            .campaign(id)
            .unwrap();
        if record.status == "paused" {
            let mut store = linguist_store::Store::open_existing(root).unwrap();
            store
                .set_campaign_status(
                    id,
                    record.version,
                    "running",
                    None,
                    None,
                    "resumed",
                    &json!({}),
                    0,
                )
                .unwrap();
        }
        if phases(root, id).iter().all(|p| p == "done") {
            return kills;
        }
    }
    panic!("campaign did not finish: {:?}", phases(root, id));
}

fn world_notes() -> Vec<NoteInfo> {
    vec![
        note("101", "冷房", "れいぼう", 5),
        note("102", "神秘", "しんぴ", 5),
        note("103", "因子", "いんし", 5),
        note("104", "本", "ほん", 9),
        note("105", "本", "ほん", 2),
    ]
}

#[test]
fn a_campaign_runs_every_batch_to_done_and_merges_duplicates() {
    let root = std::env::temp_dir().join(format!("lab-campaign-engine-{}", Uuid::new_v4()));
    let mut spec = spec(true);
    spec.limits.retain_done_checkpoints = 1;
    let id = create(&root, spec, world_notes());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    drive(&root, id, &cli, None);
    let w = cli.world.lock().unwrap();
    // Every note applied exactly once; the duplicate deleted after its survivor.
    assert_eq!(
        w.applied.keys().cloned().collect::<Vec<_>>(),
        ["101", "102", "103", "104"]
    );
    assert!(w.applied.values().all(|n| *n == 1), "{:?}", w.applied);
    assert_eq!(w.deleted, BTreeSet::from(["105".to_owned()]));
    let survivor_applied = w
        .calls
        .iter()
        .position(|c| {
            c.starts_with("jobs run")
                && c.contains("--apply")
                && w.apply_jobs
                    .iter()
                    .any(|(j, (p, _, _))| c.contains(j) && w.plans[p].notes == ["104"])
        })
        .unwrap();
    let delete = w
        .calls
        .iter()
        .position(|c| c.starts_with("notes delete-merged 105"))
        .unwrap();
    assert!(survivor_applied < delete);
    // Retention keeps the newest done batch's checkpoint and prunes the rest.
    let checkpoints = linguist_store::Store::read_only(&root)
        .unwrap()
        .campaign_checkpoints(id)
        .unwrap();
    assert_eq!(
        checkpoints.len(),
        3,
        "one per apply job (pilot, plain, merge)"
    );
    assert_eq!(
        checkpoints.iter().filter(|c| c.pruned_ms.is_none()).count(),
        1,
        "{checkpoints:?}"
    );
    // The pilot paused the campaign for the user once.
    let events = linguist_store::Store::read_only(&root)
        .unwrap()
        .campaign_events(id, 0, 1000)
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.body["code"] == "CAMPAIGN_PILOT_REVIEW")
            .count(),
        1
    );
}

#[test]
fn a_tick_killed_after_any_command_loses_no_work_and_repeats_no_write() {
    // How many commands a clean run sends.
    let total = {
        let root = std::env::temp_dir().join(format!("lab-campaign-count-{}", Uuid::new_v4()));
        let id = create(&root, spec(false), world_notes());
        let cli = FakeCli {
            world: Mutex::default(),
            kill_after: AtomicU64::new(0),
            clock: AtomicU64::new(1000),
        };
        drive(&root, id, &cli, None);
        cli.world.into_inner().unwrap().calls.len() as u64
    };
    assert!(total > 20, "{total}");
    // Kill the tick once, right after the k-th command, for every k.
    for every in 1..=total {
        let root = std::env::temp_dir().join(format!("lab-campaign-kill-{}", Uuid::new_v4()));
        let id = create(&root, spec(false), world_notes());
        let cli = FakeCli {
            world: Mutex::default(),
            kill_after: AtomicU64::new(0),
            clock: AtomicU64::new(1000),
        };
        let kills = drive(&root, id, &cli, Some(every));
        let w = cli.world.lock().unwrap();
        // Idle lanes look again while another lane works, so a run may send
        // a few commands fewer than the counting run and never reach `every`.
        assert!(
            kills == 1 || (kills == 0 && (w.calls.len() as u64) < every),
            "kill after command {every}: {kills} kills, {} commands",
            w.calls.len()
        );
        assert_eq!(w.applied.len(), 4, "kill every {every}");
        // A note may be applied by a job only once, even when a killed tick
        // left an orphan job behind (it is never run).
        assert!(
            w.applied.values().all(|n| *n == 1),
            "kill every {every}: {:?}",
            w.applied
        );
        assert_eq!(w.deleted, BTreeSet::from(["105".to_owned()]));
        let store = linguist_store::Store::read_only(&root).unwrap();
        // Every checkpoint any job made is the campaign's (so retention sees it).
        let recorded: BTreeSet<String> = store
            .campaign_checkpoints(id)
            .unwrap()
            .into_iter()
            .map(|c| c.checkpoint)
            .collect();
        for (checkpoint, _) in &w.checkpoints {
            assert!(
                recorded.contains(checkpoint),
                "kill every {every}: {checkpoint} not recorded"
            );
        }
    }
}

#[test]
fn the_agent_settles_or_defers_what_the_policy_left_and_too_many_agent_decisions_pause() {
    let root = std::env::temp_dir().join(format!("lab-campaign-agent-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    cli.world.lock().unwrap().hard.insert("102".into());
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    ctx.tick(600).unwrap();
    assert_eq!(phases(&root, id), ["needs_agent"]);
    let status =
        campaign::engine::status(&linguist_store::Store::read_only(&root).unwrap(), id, 0).unwrap();
    assert_eq!(status["agent_needed"], true);
    assert!(
        status["agent"][0]["needs"]["102"][0]
            .as_str()
            .unwrap()
            .starts_with("DICTIONARY_SENSE_REVIEW:102")
    );
    // An unchanged plan is not rechecked.
    let calls = cli.world.lock().unwrap().calls.len();
    ctx.tick(600).unwrap();
    assert_eq!(
        cli.world.lock().unwrap().calls.len(),
        calls + 1,
        "one plans show, nothing else"
    );
    // The agent decides the sense: half the batch is now agent-decided.
    {
        let mut w = cli.world.lock().unwrap();
        w.hard.clear();
        w.agent_decided.insert("102".into());
        let plan = w.plans.values_mut().next().unwrap();
        plan.decided.insert("102".into());
        plan.revision += 1;
    }
    ctx.tick(600).unwrap();
    assert_eq!(phases(&root, id), ["ready"]);
    let refused = ctx.approve(1, "claude-agent").unwrap_err();
    assert!(
        refused.starts_with("CAMPAIGN_PAUSED_AGENT_SHARE"),
        "{refused}"
    );
    let record = linguist_store::Store::read_only(&root)
        .unwrap()
        .campaign(id)
        .unwrap();
    assert_eq!(record.code.as_deref(), Some("CAMPAIGN_PAUSED_AGENT_SHARE"));
    // Deferring the agent-decided note brings the share back under the limit.
    let mut store = linguist_store::Store::open_existing(&root).unwrap();
    store
        .set_campaign_status(
            id,
            record.version,
            "running",
            None,
            None,
            "resumed",
            &json!({}),
            0,
        )
        .unwrap();
    drop(store);
    ctx.item(
        1,
        "102",
        ItemAction::Defer("homograph, decide later".into()),
    )
    .unwrap();
    ctx.tick(600).unwrap();
    assert_eq!(phases(&root, id), ["ready"]);
    let approved = ctx.approve(1, "claude-agent").unwrap();
    assert_eq!(approved["approval"]["notes"], json!(["101"]));
    let batch = BatchData::from_value(
        &linguist_store::Store::read_only(&root)
            .unwrap()
            .campaign_batch(id, 1)
            .unwrap()
            .data,
    )
    .unwrap();
    assert_eq!(batch.deferred["102"], "AGENT: homograph, decide later");
}

#[test]
fn verification_pauses_on_a_lost_review_and_accepts_new_ones() {
    use campaign::verify::{CardFacts, NoteFacts, compare};
    let before = NoteFacts {
        note_id: "1".into(),
        model: "2. Picture Words".into(),
        cards: vec![CardFacts {
            id: "11".into(),
            ordinal: 0,
            repetitions: 3,
            reviews: vec!["a".into(), "b".into()],
        }],
        missing_media: vec![],
    };
    let targets = vec!["Linguist Vocabulary v3".to_owned()];
    let mut after = before.clone();
    after.model = targets[0].clone();
    after.cards[0].repetitions = 4;
    after.cards[0].reviews.push("c".into());
    assert!(
        compare(&before, Some(&after), &targets).is_empty(),
        "a review done meanwhile is fine"
    );
    after.cards[0].reviews.remove(0);
    assert_eq!(
        compare(&before, Some(&after), &targets),
        ["1: card 11 lost 1 review rows"]
    );
    after.missing_media.push("x.png".into());
    after.cards.clear();
    assert_eq!(compare(&before, Some(&after), &targets).len(), 2);
    assert_eq!(compare(&before, None, &targets), ["1: note is gone"]);
}

#[test]
fn a_batch_is_ready_only_when_the_final_plan_has_no_open_issue() {
    let root = std::env::temp_dir().join(format!("lab-campaign-ready-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    // Generation drafts both notes after the first policy pass, reopening them.
    cli.world.lock().unwrap().drafts = 2;
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    ctx.tick(600).unwrap();
    assert_eq!(phases(&root, id), ["ready"]);
    let w = cli.world.lock().unwrap();
    let plan = w.plans.values().next().unwrap();
    assert_eq!(
        plan.decided.len(),
        2,
        "the drafted notes were decided again"
    );
    let last = w
        .calls
        .iter()
        .rposition(|c| c.starts_with("plans resolve-batch"))
        .unwrap();
    // The generate that drafted is the first one; a policy pass follows it.
    let generate = w
        .calls
        .iter()
        .position(|c| c.starts_with("plans generate"))
        .unwrap();
    assert!(last > generate, "a policy pass follows the drafts");
}

#[test]
fn a_batch_whose_drafts_failed_is_never_ready() {
    let root = std::env::temp_dir().join(format!("lab-campaign-draftfail-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    // A usage limit waits without counting failures; then drafting works.
    cli.world.lock().unwrap().draft_fail = Some((
        3,
        "AGENT_FAILED: exit Some(1): ERROR: You've hit your usage limit.".into(),
    ));
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    ctx.tick(600).unwrap();
    assert_eq!(phases(&root, id), ["reviewing"], "waits while drafts fail");
    for _ in 0..4 {
        ctx.tick(600).unwrap();
    }
    assert_eq!(phases(&root, id), ["ready"]);
    let generates = cli
        .world
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|c| c.starts_with("plans generate"))
        .count();
    assert!(generates >= 4, "ready only after a generate without errors");
    let store = linguist_store::Store::read_only(&root).unwrap();
    let batch = &store.campaign_batches(id).unwrap()[0];
    assert_eq!(
        batch.data["step_failures"], 0,
        "a usage limit is not a failure"
    );
    drop(store);

    // A draft the checks refuse is drafted again, then left to the agent.
    let root = std::env::temp_dir().join(format!("lab-campaign-draftfail2-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    // Codex at its usage limit, the local fallback's draft refused: that
    // counts as a failed draft, not a wait for the quota.
    cli.world.lock().unwrap().draft_fail = Some((
        99,
        "AGENT_QUOTA_COOLDOWN: codex reported its usage limit; skipped until 9 (x); fallback ollama: GENERATION_INCOMPLETE_EXAMPLE".into(),
    ));
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    for _ in 0..3 {
        ctx.tick(600).unwrap();
    }
    assert_eq!(phases(&root, id), ["needs_agent"]);
    let generates = cli
        .world
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|c| c.starts_with("plans generate"))
        .count();
    assert_eq!(generates, 3, "three drafts per note, then the agent");
    let status =
        campaign::engine::status(&linguist_store::Store::read_only(&root).unwrap(), id, 0).unwrap();
    assert_eq!(status["status"], "running", "a refused draft never pauses");
    let needs = &status["agent"][0]["needs"];
    for note in ["101", "102"] {
        assert!(
            needs[note][0]
                .as_str()
                .unwrap()
                .starts_with("GENERATION_FAILED: 3 drafts failed (AGENT_QUOTA_COOLDOWN")
        );
    }
}

#[test]
fn an_uncertified_local_engine_pauses_the_campaign() {
    let root = std::env::temp_dir().join(format!("lab-campaign-engine-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    cli.world.lock().unwrap().draft_fail = Some((
        99,
        "AGENT_QUOTA_COOLDOWN: codex reported its usage limit; skipped until 9 (x); fallback ollama: GENERATION_ENGINE_UNVERIFIED: no passed certification covers the local engine as it is now".into(),
    ));
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    let report = ctx.tick(600).unwrap();
    assert_eq!(report.stopped, "GENERATION_ENGINE_UNVERIFIED");
    let status =
        campaign::engine::status(&linguist_store::Store::read_only(&root).unwrap(), id, 0).unwrap();
    assert_eq!(status["status"], "paused");
    assert_eq!(status["code"], "GENERATION_ENGINE_UNVERIFIED");
    let generates = cli
        .world
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|c| c.starts_with("plans generate") && c.contains("--certified-engine-only"))
        .count();
    assert_eq!(generates, 1, "one refused generate, then the pause");
}

/// WP-24: Anki closing and reopening its collection leaves the companion
/// without a session for a while; the review waits, it never fails.
#[test]
fn a_review_without_the_companion_session_waits_and_never_pauses() {
    let root = std::env::temp_dir().join(format!("lab-campaign-engine-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    cli.world.lock().unwrap().no_session = 40;
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    for _ in 0..4 {
        let ctx = context(&root, id, &cli, &anki, &clock);
        ctx.tick(600).unwrap();
    }
    let store = linguist_store::Store::read_only(&root).unwrap();
    assert_eq!(store.campaign(id).unwrap().status, "running");
    for b in store.campaign_batches(id).unwrap() {
        assert_eq!(BatchData::from_value(&b.data).unwrap().step_failures, 0);
    }
    cli.world.lock().unwrap().no_session = 0;
    assert_eq!(drive(&root, id, &cli, None), 0);
}

#[test]
fn an_unresolved_reconcile_is_tried_again_before_it_pauses() {
    let run = |unresolved: u32| {
        let root = std::env::temp_dir().join(format!("lab-campaign-reconcile-{}", Uuid::new_v4()));
        let mut spec = spec(false);
        spec.limits.max_step_failures = 3;
        let id = create(&root, spec, world_notes()[..2].to_vec());
        let cli = FakeCli {
            world: Mutex::default(),
            kill_after: AtomicU64::new(0),
            clock: AtomicU64::new(1000),
        };
        cli.world.lock().unwrap().recovery = Some(unresolved);
        let anki = FakeAnki(&cli);
        let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
        let ctx = context(&root, id, &cli, &anki, &clock);
        ctx.tick(600).unwrap();
        ctx.approve(1, "claude-agent").unwrap();
        for _ in 0..8 {
            ctx.tick(600).unwrap();
        }
        let status =
            campaign::engine::status(&linguist_store::Store::read_only(&root).unwrap(), id, 0)
                .unwrap();
        (phases(&root, id), status)
    };
    // Anki was briefly away: the next attempts resolve it, no pause.
    let (phases, status) = run(2);
    assert_eq!(status["status"], "running", "{status}");
    assert_eq!(phases, ["done"]);
    // Never resolved: the step-failure guardrail pauses, naming the outcome.
    let (_, status) = run(999);
    assert_eq!(status["status"], "paused", "{status}");
    assert!(
        status["detail"]
            .as_str()
            .unwrap()
            .contains("needs_recovery"),
        "{status}"
    );
}

#[test]
fn an_agent_edit_during_a_review_is_reviewed_again() {
    let root = std::env::temp_dir().join(format!("lab-campaign-midreview-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    {
        let mut w = cli.world.lock().unwrap();
        w.hard.insert("101".into());
        w.agent_edits_mid_review = true;
    }
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    for _ in 0..3 {
        ctx.tick(600).unwrap();
    }
    // The needs were computed before the agent's decision: the batch is
    // reviewed again on the newer revision instead of waiting for nothing.
    assert_eq!(phases(&root, id), ["ready"]);
}

#[test]
fn needs_already_met_are_reviewed_again_not_waited_for() {
    let root = std::env::temp_dir().join(format!("lab-campaign-met-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    cli.world.lock().unwrap().hard.insert("101".into());
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    ctx.tick(600).unwrap();
    assert_eq!(phases(&root, id), ["needs_agent"]);
    // The issue is settled without a newer revision being seen (a review
    // that raced the agent): the batch is reviewed again.
    {
        let mut w = cli.world.lock().unwrap();
        w.hard.clear();
        let plan = w.plans.values_mut().next().unwrap();
        plan.decided.insert("101".into());
    }
    ctx.tick(600).unwrap();
    assert_eq!(phases(&root, id), ["ready"]);
}

#[test]
fn a_batch_over_the_size_limit_is_split_until_it_fits() {
    let root = std::env::temp_dir().join(format!("lab-campaign-split-{}", Uuid::new_v4()));
    let mut spec = spec(false);
    spec.batch_size = 4;
    let notes: Vec<NoteInfo> = (0..4)
        .map(|i| note(&format!("20{i}"), &format!("語{i}"), &format!("ご{i}"), 1))
        .collect();
    let id = create(&root, spec, notes);
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    cli.world.lock().unwrap().max_notes = Some(1);
    drive(&root, id, &cli, None);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let batches = store.campaign_batches(id).unwrap();
    assert_eq!(batches.len(), 4, "4 notes, at most 1 per publishable plan");
    // Once a split showed the size that publishes, later batches of the
    // source are split before preparing: no further failed preparations.
    let failed = cli
        .world
        .lock()
        .unwrap()
        .prepare_jobs
        .values()
        .filter(|notes| notes.len() > 1)
        .count();
    assert!(failed <= 2, "{failed} oversized prepare jobs");
    assert!(batches.iter().all(|b| b.phase == "done"));
    let w = cli.world.lock().unwrap();
    assert_eq!(w.applied.len(), 4);
    assert!(w.applied.values().all(|n| *n == 1));
}

#[test]
fn audio_lost_to_a_service_outage_is_enriched_again_then_left_to_the_agent() {
    let root = std::env::temp_dir().join(format!("lab-campaign-outage-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    {
        let mut w = cli.world.lock().unwrap();
        w.outage.insert("101".into(), 1); // back after one more run
        w.outage.insert("102".into(), 9); // stays down
    }
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    for _ in 0..3 {
        ctx.tick(600).unwrap();
    }
    assert_eq!(phases(&root, id), ["needs_agent"]);
    let w = cli.world.lock().unwrap();
    assert_eq!(w.reenriched.iter().filter(|n| *n == "101").count(), 1);
    assert_eq!(
        w.reenriched.iter().filter(|n| *n == "102").count(),
        3,
        "three runs, then the agent"
    );
    drop(w);
    let status =
        campaign::engine::status(&linguist_store::Store::read_only(&root).unwrap(), id, 0).unwrap();
    assert!(
        status["agent"][0]["needs"]["102"][0]
            .as_str()
            .unwrap()
            .starts_with("AUDIO_SYNTHESIS_UNAVAILABLE")
    );
}

#[test]
fn a_dictionary_entry_lost_to_a_usage_limit_is_written_again() {
    let root = std::env::temp_dir().join(format!("lab-campaign-dictionary-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..3].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    let limit = "AGENT_QUOTA_COOLDOWN: codex reported its usage limit; skipped until 1".to_owned();
    {
        let mut w = cli.world.lock().unwrap();
        // Still at the limit for one more run: the batch waits, no try spent.
        w.dictionary_outage.insert("101".into(), (2, limit));
        w.dictionary_outage
            .insert("102".into(), (9, "AGENT_TIMEOUT: no answer".into()));
        // A refused entry is no outage: it stays with the agent.
        w.dictionary_outage
            .insert("103".into(), (9, "DICTIONARY_ENTRY_INVALID: x".into()));
    }
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    for _ in 0..6 {
        ctx.tick(600).unwrap();
    }
    let w = cli.world.lock().unwrap();
    assert_eq!(w.redictionaried.iter().filter(|n| *n == "101").count(), 2);
    assert_eq!(w.dictionary_outage["101"].0, 0);
    assert_eq!(
        w.redictionaried.iter().filter(|n| *n == "102").count(),
        3,
        "three runs, then the agent"
    );
    assert!(!w.redictionaried.contains(&"103".to_owned()));
}

#[test]
fn an_agent_step_on_a_waiting_batch_writes_a_lost_dictionary_entry_again_except_for_deferred_notes()
{
    let root =
        std::env::temp_dir().join(format!("lab-campaign-dictionary-wait-{}", Uuid::new_v4()));
    let mut spec = spec(false);
    spec.batch_size = 3;
    let id = create(&root, spec, world_notes()[..3].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    cli.world.lock().unwrap().hard.insert("101".into());
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    for _ in 0..3 {
        ctx.tick(600).unwrap();
    }
    assert_eq!(phases(&root, id), ["needs_agent"]);
    // Prepared while the provider was at its limit (before the retry existed).
    let limit = "AGENT_QUOTA_COOLDOWN: codex reported its usage limit; skipped until 1".to_owned();
    {
        let mut w = cli.world.lock().unwrap();
        w.dictionary_outage.insert("102".into(), (1, limit.clone()));
        w.dictionary_outage.insert("103".into(), (1, limit));
    }
    let batch = linguist_store::Store::read_only(&root)
        .unwrap()
        .campaign_batches(id)
        .unwrap()[0]
        .batch;
    // The agent's step reviews the batch again; the deferred note is left
    // as it is.
    ctx.item(batch, "103", ItemAction::Defer("AGENT: x".into()))
        .unwrap();
    for _ in 0..3 {
        ctx.tick(600).unwrap();
    }
    let w = cli.world.lock().unwrap();
    assert_eq!(w.redictionaried, ["102"]);
}

#[test]
fn tuned_prep_workers_prepare_batches_at_once_and_the_agent_holds_its_batches() {
    let root = std::env::temp_dir().join(format!("lab-campaign-parallel-{}", Uuid::new_v4()));
    let mut spec = spec(false);
    spec.batch_size = 1;
    let id = create(&root, spec, world_notes());
    // Parallel prep stays within prepare_ahead; the agent share is the
    // only tunable guardrail (the user's step, checked by the CLI).
    let mut limits = Limits::default();
    let refused = campaign::Tuning {
        prepare_parallel: Some(5),
        ..Default::default()
    }
    .apply(&mut limits)
    .unwrap_err();
    assert!(refused.starts_with("CAMPAIGN_TUNING_INVALID"), "{refused}");
    let tuning = campaign::Tuning {
        prepare_ahead: Some(4),
        prepare_parallel: Some(3),
        agent_runs_per_day: Some(288),
        agent_share_max: None,
    };
    let mut limits = spec_limits(&root, id);
    tuning.apply(&mut limits).unwrap();
    linguist_store::Store::open_existing(&root)
        .unwrap()
        .append_campaign_event(id, None, "tuned", &json!({"limits": limits}), 2)
        .unwrap();
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    cli.world.lock().unwrap().prepare_delay_ms = 50;
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    assert_eq!(ctx.definition.spec.limits.prepare_parallel, 3);
    let store = linguist_store::Store::read_only(&root).unwrap();
    let status = campaign::engine::status(&store, id, 0).unwrap();
    assert_eq!(status["limits"]["prepare_ahead"], 4);
    assert_eq!(status["agent_runs_per_day"], 288);
    // An agent run in progress holds batches waiting for it.
    let mut writer = linguist_store::Store::open_existing(&root).unwrap();
    writer
        .append_campaign_event(id, None, "agent_run", &json!({}), 1000)
        .unwrap();
    ctx.tick(600).unwrap();
    // Three prep workers and the write lane each sent commands.
    assert!(
        cli.world.lock().unwrap().threads.len() >= 3,
        "{:?}",
        cli.world.lock().unwrap().threads
    );
    // Four batches in flight (prepare_ahead), none past preparation.
    let p = phases(&root, id);
    assert_eq!(
        p.iter()
            .filter(|p| matches!(p.as_str(), "needs_agent" | "ready"))
            .count(),
        4,
        "{p:?}"
    );
    assert_eq!(
        campaign::engine::status(&store, id, 2000).unwrap()["agent_active"],
        true
    );
    // A batch waiting for the agent is not rechecked while the agent runs,
    // even when its plan changed.
    let waiting: Vec<u32> = store
        .campaign_batches(id)
        .unwrap()
        .into_iter()
        .filter(|b| b.phase == "needs_agent")
        .map(|b| b.batch)
        .collect();
    assert!(!waiting.is_empty(), "{p:?}");
    {
        let mut w = cli.world.lock().unwrap();
        for plan in w.plans.values_mut() {
            plan.revision += 1;
        }
    }
    let calls = cli.world.lock().unwrap().calls.len();
    ctx.tick(600).unwrap();
    assert_eq!(cli.world.lock().unwrap().calls.len(), calls, "nothing sent");
    writer
        .append_campaign_event(id, None, "agent_done", &json!({}), 3000)
        .unwrap();
    assert_eq!(
        campaign::engine::status(&store, id, 3000).unwrap()["agent_active"],
        false
    );
    ctx.tick(600).unwrap();
    assert!(cli.world.lock().unwrap().calls.len() > calls);
    drop((store, writer));
    std::fs::remove_dir_all(root).unwrap();
}

fn spec_limits(root: &std::path::Path, id: Uuid) -> Limits {
    let store = linguist_store::Store::read_only(root).unwrap();
    Definition::effective(&store, id).unwrap().spec.limits
}

#[test]
fn deferred_notes_of_finished_batches_are_requeued_once() {
    let root = std::env::temp_dir().join(format!("lab-campaign-requeue-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    drive(&root, id, &cli, None);
    let mut store = linguist_store::Store::open_existing(&root).unwrap();
    let done = store.campaign_batch(id, 1).unwrap();
    let mut data = BatchData::from_value(&done.data).unwrap();
    let note = data.notes[0].clone();
    data.deferred.insert(
        note.clone(),
        "AGENT: spot-check: invented collocation".into(),
    );
    data.deferred
        .insert(data.notes[1].clone(), "VALIDATION: kept".into());
    store
        .update_campaign_batch(
            id,
            1,
            done.version,
            "done",
            &data.to_value(),
            "batch",
            &json!("test"),
            5,
        )
        .unwrap();
    let before = store.campaign_batches(id).unwrap().len() as u32;
    let reasons = ["AGENT:".to_owned()];
    let out = campaign::engine::requeue(&mut store, id, &reasons, 6).unwrap();
    assert_eq!(out["requeued"][0]["from"], 1, "{out}");
    let new = store.campaign_batch(id, before + 1).unwrap();
    assert_eq!(new.phase, "selected");
    assert_eq!(
        BatchData::from_value(&new.data).unwrap().notes,
        std::slice::from_ref(&note)
    );
    let old = BatchData::from_value(&store.campaign_batch(id, 1).unwrap().data).unwrap();
    assert!(old.deferred[&note].starts_with(&format!("REQUEUED to batch {}", before + 1)));
    // Only the matching reason, and only once.
    let again = campaign::engine::requeue(&mut store, id, &reasons, 7).unwrap();
    assert_eq!(again["requeued"], json!([]));
    drop(store);
    // The requeued note is prepared and applied again.
    drive(&root, id, &cli, None);
    assert_eq!(cli.world.lock().unwrap().applied[&note], 2);
    std::fs::remove_dir_all(root).unwrap();
}

/// WP-24: a batch whose every note the agent deferred is finished by its
/// approval (nothing to write), so requeue can send the notes round again.
#[test]
fn approving_a_batch_whose_every_note_is_deferred_finishes_it() {
    let root = std::env::temp_dir().join(format!("lab-campaign-alldeferred-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ready = |root: &std::path::Path| {
        linguist_store::Store::read_only(root)
            .unwrap()
            .campaign_batches(id)
            .unwrap()
            .into_iter()
            .find(|b| b.phase == "ready")
    };
    for _ in 0..10 {
        if ready(&root).is_some() {
            break;
        }
        context(&root, id, &cli, &anki, &clock).tick(600).unwrap();
    }
    let batch = ready(&root).expect("a ready batch");
    let ctx = context(&root, id, &cli, &anki, &clock);
    for note in BatchData::from_value(&batch.data).unwrap().notes {
        ctx.item(batch.batch, &note, ItemAction::Defer("AGENT: test".into()))
            .unwrap();
    }
    for _ in 0..10 {
        if ready(&root).is_some() {
            break;
        }
        context(&root, id, &cli, &anki, &clock).tick(600).unwrap();
    }
    let out = context(&root, id, &cli, &anki, &clock)
        .approve(batch.batch, "claude-agent")
        .unwrap();
    assert_eq!(out["phase"], "done", "{out}");
    let mut store = linguist_store::Store::open_existing(&root).unwrap();
    let requeued = campaign::engine::requeue(&mut store, id, &["AGENT:".to_owned()], 9).unwrap();
    assert_eq!(requeued["requeued"][0]["from"], batch.batch, "{requeued}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_ready_batch_reopened_fresh_is_prepared_again_from_scratch() {
    let root = std::env::temp_dir().join(format!("lab-campaign-fresh-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    ctx.tick(600).unwrap();
    assert_eq!(phases(&root, id), ["ready"]);
    let old = BatchData::from_value(
        &linguist_store::Store::read_only(&root)
            .unwrap()
            .campaign_batch(id, 1)
            .unwrap()
            .data,
    )
    .unwrap();
    let out = ctx.reopen(1, "stronger model", true).unwrap();
    assert_eq!(out["phase"], "selected");
    let data = BatchData::from_value(
        &linguist_store::Store::read_only(&root)
            .unwrap()
            .campaign_batch(id, 1)
            .unwrap()
            .data,
    )
    .unwrap();
    assert_eq!((data.plan, data.prepare_job), (None, None));
    assert_eq!(data.notes, old.notes);
    // A plain reopen still needs an approved batch.
    ctx.tick(600).unwrap();
    assert!(ctx.reopen(1, "again", false).is_err());
    let new = BatchData::from_value(
        &linguist_store::Store::read_only(&root)
            .unwrap()
            .campaign_batch(id, 1)
            .unwrap()
            .data,
    )
    .unwrap();
    assert_ne!(new.plan, old.plan, "a new plan");
    assert_eq!(phases(&root, id), ["ready"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn notes_anki_was_too_busy_to_read_are_read_again_before_any_is_left_out() {
    let root = std::env::temp_dir().join(format!("lab-campaign-busy-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    {
        let mut w = cli.world.lock().unwrap();
        w.read_fail.insert("101".into(), 2); // readable after two retries
        w.read_fail.insert("102".into(), 9); // never
    }
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    for _ in 0..12 {
        ctx.tick(600).unwrap();
        // Five minutes pass between ticks.
        cli.clock.fetch_add(5 * 60 * 1000, Ordering::SeqCst);
    }
    let w = cli.world.lock().unwrap();
    assert_eq!(
        w.read_retries, 3,
        "three retries, then the rest is published"
    );
    drop(w);
    let data = BatchData::from_value(
        &linguist_store::Store::read_only(&root)
            .unwrap()
            .campaign_batch(id, 1)
            .unwrap()
            .data,
    )
    .unwrap();
    assert!(data.plan.is_some());
    assert_eq!(
        data.deferred.keys().collect::<Vec<_>>(),
        ["102"],
        "{:?}",
        data.deferred
    );
    assert!(data.deferred["102"].starts_with("PREPARE_FAILED: SOURCE_READ_CONNECTION_FAILED"));
    std::fs::remove_dir_all(root).unwrap();

    // A job whose attempt budget is spent is published without the notes
    // at once (exit 4, nothing eligible), never a step failure.
    let root = std::env::temp_dir().join(format!("lab-campaign-budget-{}", Uuid::new_v4()));
    let id = create(&root, spec(false), world_notes()[..2].to_vec());
    let cli = FakeCli {
        world: Mutex::default(),
        kill_after: AtomicU64::new(0),
        clock: AtomicU64::new(1000),
    };
    {
        let mut w = cli.world.lock().unwrap();
        w.read_fail.insert("102".into(), 9);
        w.retry_budget = Some(0);
    }
    let anki = FakeAnki(&cli);
    let clock = || cli.clock.fetch_add(10, Ordering::SeqCst);
    let ctx = context(&root, id, &cli, &anki, &clock);
    ctx.tick(600).unwrap();
    let record = linguist_store::Store::read_only(&root)
        .unwrap()
        .campaign_batch(id, 1)
        .unwrap();
    let data = BatchData::from_value(&record.data).unwrap();
    assert_eq!(
        (data.step_failures, data.plan.is_some()),
        (0, true),
        "{data:?}"
    );
    assert!(data.deferred.contains_key("102"));
    std::fs::remove_dir_all(root).unwrap();
}

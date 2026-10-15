//! WP-24 `campaign` commands: create a frozen collection revamp campaign,
//! advance it one tick at a time (`campaign run`), and the few decisions the
//! agent or the user takes between ticks. Every effect goes through this
//! binary's own commands, run as child processes with the campaign's frozen
//! purpose and settings; the campaign itself only records progress.
use crate::{emit, native_writes, state_root};
use clap::Subcommand;
use linguist_application::campaign::{
    self, Definition, Spec,
    engine::{self, Collection, Context, ItemAction, Output, Runner},
    select::{self, NoteInfo},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
};
use uuid::Uuid;

#[derive(Subcommand)]
pub(crate) enum CampaignCommand {
    /// Read the spec's sources from Anki, freeze cohorts and batches, and
    /// store the campaign; --dry-run prints the selection only.
    Create {
        /// JSON campaign spec (contracts/v2/campaign-spec.schema.json).
        #[arg(long)]
        spec: PathBuf,
        /// Print the selection report without storing a campaign.
        #[arg(long)]
        dry_run: bool,
    },
    /// List campaigns, newest first.
    List,
    /// Status for the agent and the user: phases, open items, next commands.
    Status {
        /// Campaign ID; defaults to the newest.
        campaign: Option<Uuid>,
    },
    /// One batch with its working data.
    Batch {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Batch number from `campaign status`.
        batch: u32,
    },
    /// Campaign events, oldest first.
    Events {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Continue after this event sequence.
        #[arg(long, default_value_t = 0)]
        after: u64,
        /// Maximum events (1-1000).
        #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
    },
    /// Advance every batch as far as it goes without judgment, then exit.
    Run {
        /// Campaign ID; defaults to the newest.
        campaign: Option<Uuid>,
        /// Stop starting new steps after this many seconds.
        #[arg(long, default_value_t = 3000)]
        max_seconds: u64,
    },
    /// Bind, validate and approve a ready batch (the agent's step).
    Approve {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Batch number from `campaign status`.
        batch: u32,
        /// Approver recorded with the plan approval.
        #[arg(long)]
        actor: String,
    },
    /// Settle, defer or restore one note of a batch under review.
    Item {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Batch number from `campaign status`.
        batch: u32,
        /// Anki note ID in that batch.
        note: String,
        /// The note's open question is answered (split done, or not needed).
        #[arg(long, conflicts_with_all = ["defer", "undefer"])]
        settled: bool,
        /// Leave the note out of this batch, with a reason.
        #[arg(long, conflicts_with = "undefer")]
        defer: Option<String>,
        /// Put a deferred note back into the batch.
        #[arg(long)]
        undefer: bool,
    },
    /// Send an approved batch back to review before its apply job exists.
    Reopen {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Batch number from `campaign status`.
        batch: u32,
        /// Why, recorded in the campaign events.
        #[arg(long)]
        reason: String,
        /// Prepare the batch again from scratch (ready, needs_agent or
        /// approved, no apply job) with the current settings.
        #[arg(long)]
        fresh: bool,
    },
    /// Leave a whole batch out before approval.
    Skip {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Batch number from `campaign status`.
        batch: u32,
        /// Why, recorded in the campaign events.
        #[arg(long)]
        reason: String,
    },
    /// Stop the campaign; only the user resumes it.
    Pause {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Why, recorded in the campaign events.
        #[arg(long, default_value = "paused by request")]
        reason: String,
    },
    /// Resume a paused campaign (the user's step: needs a terminal or --by-user).
    Resume {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Confirm that the user, not an agent, resumes it.
        #[arg(long)]
        by_user: bool,
    },
    /// Stop for good; batches keep their state and history.
    Cancel {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
    },
    /// Close a finished campaign (then sync to AnkiWeb with Upload).
    Close {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
    },
    /// Copy one stored asset (a source picture, a candidate) to a new file
    /// so the agent can look at it.
    Asset {
        /// Asset digest from a plan's media or archives.
        digest: String,
        /// Create-new output file (give it the image's extension).
        #[arg(long)]
        output: PathBuf,
    },
    /// Record one agent run if today's limit allows it (exit 5 when not).
    AgentSlot {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
    },
    /// Send notes that finished batches deferred back round in new batches
    /// (for example after a generation or policy fix).
    Requeue {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Deferral reason prefixes to requeue (repeatable).
        #[arg(long = "reason", default_values_t = ["AGENT:".to_owned(), "APPLY_FAILED:".to_owned()])]
        reasons: Vec<String>,
    },
    /// Record that the agent run started by `agent-slot` ended; batches
    /// waiting for the agent are reviewed again.
    AgentDone {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
    },
    /// Change throughput limits of a running campaign (recorded as a
    /// `tuned` event; guardrails are not tunable).
    Tune {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Batches prepared ahead of the oldest unapplied one (1-32).
        #[arg(long)]
        prepare_ahead: Option<u32>,
        /// Batches prepared and reviewed at once (1-8, at most prepare-ahead).
        #[arg(long)]
        prepare_parallel: Option<u32>,
        /// Agent runs per day (at most 288, one every 5 minutes).
        #[arg(long)]
        agent_runs_per_day: Option<u32>,
    },
    /// Write the systemd user timers that run scripts/campaign-tick.sh and,
    /// with --agent-script, scripts/campaign-agent.sh on its own timer.
    InstallTimer {
        /// Campaign ID from `campaign list`.
        campaign: Uuid,
        /// Minutes between ticks.
        #[arg(long, default_value_t = 15)]
        interval_minutes: u32,
        /// The tick wrapper script.
        #[arg(long)]
        script: PathBuf,
        /// The agent wrapper script (its own timer).
        #[arg(long)]
        agent_script: Option<PathBuf>,
        /// Minutes between agent runs.
        #[arg(long, default_value_t = 5)]
        agent_interval_minutes: u32,
        /// Working directory for the agent (the repository with the skill).
        #[arg(long)]
        workdir: PathBuf,
        /// Extra environment for the tick (repeatable KEY=VALUE), for
        /// example LAB_START_ANKI=1, LAB_START_VOICEVOX=1, LAB_CLAUDE=PATH.
        #[arg(long = "env")]
        env: Vec<String>,
        /// Write the units; without it they are only printed.
        #[arg(long)]
        execute: bool,
    },
}

/// The global options a child command repeats.
pub(crate) struct Globals {
    pub config: Option<PathBuf>,
    pub profile: Option<String>,
    /// The creating command's own `--set KEY=VALUE` overrides.
    pub settings: Vec<String>,
    pub offline: bool,
}

fn now() -> u64 {
    campaign::now_ms()
}

struct ChildRunner {
    exe: PathBuf,
    global_args: Vec<String>,
}
impl Runner for ChildRunner {
    fn run(&self, source: &campaign::SourceSpec, args: &[String]) -> Output {
        let mut command = std::process::Command::new(&self.exe);
        command
            .args(&self.global_args)
            .args(["--output", "json", "--purpose", &source.purpose]);
        for setting in &source.settings {
            command.args(["--set", setting]);
        }
        command.args(args);
        match command.output() {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let json = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
                Output {
                    code: out.status.code().unwrap_or(-1),
                    json,
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                }
            }
            Err(e) => Output {
                code: -1,
                json: Value::Null,
                stderr: json!({"error": format!("CAMPAIGN_CHILD_SPAWN_FAILED: {e}")}).to_string(),
            },
        }
    }
}

struct Companion<'a> {
    port: Mutex<Option<linguist_application::native_port::NativePort<'a>>>,
    client: &'a linguist_anki::Client,
    settings: &'a linguist_config::Effective,
}
impl Collection for Companion<'_> {
    fn note_evidence(&self, note_id: &str) -> Result<Option<Value>, String> {
        let id: i64 = note_id.parse().map_err(|_| "CAMPAIGN_NOTE_ID_INVALID")?;
        let mut guard = self
            .port
            .lock()
            .map_err(|_| "CAMPAIGN_COMPANION_POISONED")?;
        if guard.is_none() {
            let mut port = native_writes::connect(self.client, self.settings)?;
            port.refresh()?;
            *guard = Some(port);
        }
        match guard.as_ref().unwrap().note_evidence(id) {
            Ok(evidence) => Ok(Some(evidence)),
            Err(e) if e.contains("NOTE_NOT_FOUND") => Ok(None),
            Err(e) => {
                // A restarted Anki needs a new session.
                *guard = None;
                Err(e)
            }
        }
    }
}

fn store_root(settings: &linguist_config::Effective) -> Result<PathBuf, String> {
    state_root(settings)
}

fn newest(root: &Path) -> Result<Uuid, String> {
    linguist_store::Store::read_only(root)?
        .campaign_ids()?
        .first()
        .copied()
        .ok_or_else(|| "CAMPAIGN_NOT_FOUND: create one with `campaign create --spec FILE`".into())
}

/// The frozen definition with the newest `campaign tune` applied.
fn definition(root: &Path, id: Uuid) -> Result<Definition, String> {
    Definition::effective(&linguist_store::Store::read_only(root)?, id)
}

fn global_args(globals: &Globals) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    if let Some(config) = &globals.config {
        let absolute = std::fs::canonicalize(config).map_err(|_| "CAMPAIGN_CONFIG_PATH_INVALID")?;
        args.extend(["--config".into(), absolute.to_string_lossy().into_owned()]);
    }
    if let Some(profile) = &globals.profile {
        args.extend(["--profile".into(), profile.clone()]);
    }
    for setting in &globals.settings {
        args.extend(["--set".into(), setting.clone()]);
    }
    if globals.offline {
        args.push("--offline".into());
    }
    Ok(args)
}

/// A key set both on the creating command and by a source would make every
/// child command fail with DUPLICATE_OVERRIDE.
fn check_overrides(globals: &Globals, spec: &Spec) -> Result<(), String> {
    let key = |s: &String| {
        s.split_once('=')
            .map(|(k, _)| k.to_owned())
            .unwrap_or_default()
    };
    let global: BTreeSet<String> = globals.settings.iter().map(key).collect();
    for source in &spec.sources {
        let mut seen = BTreeSet::new();
        for setting in &source.settings {
            let k = key(setting);
            if global.contains(&k) || !seen.insert(k.clone()) {
                return Err(format!(
                    "CAMPAIGN_SETTING_DUPLICATE: {k} is set twice for source {}",
                    source.id
                ));
            }
        }
    }
    Ok(())
}

/// Read every source's notes; repetitions only for possible merge members.
fn read_sources(
    client: &linguist_anki::Client,
    spec: &Spec,
) -> Result<BTreeMap<String, Vec<NoteInfo>>, String> {
    client.check_profile()?;
    let mut matched = BTreeMap::new();
    for source in &spec.sources {
        let ids = client.find_notes(&source.query)?;
        let mut notes = Vec::with_capacity(ids.len());
        let mut cards: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for chunk in ids.chunks(500) {
            for info in client.notes_info(chunk)? {
                let id = linguist_anki::wire_id(&info["noteId"])?;
                let fields = info["fields"]
                    .as_object()
                    .map(|f| {
                        f.iter()
                            .map(|(k, v)| {
                                (
                                    k.clone(),
                                    v["value"].as_str().unwrap_or_default().to_owned(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                cards.insert(
                    id.clone(),
                    info["cards"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|c| linguist_anki::wire_id(c).ok())
                        .collect(),
                );
                notes.push(NoteInfo {
                    id,
                    model: info["modelName"].as_str().unwrap_or_default().to_owned(),
                    fields,
                    reps: 0,
                });
            }
        }
        // Repetitions decide a merge's survivor: read them for members only.
        let mut members: BTreeSet<String> = select::duplicate_groups(source, &notes)
            .into_iter()
            .flatten()
            .collect();
        for extra in spec.merge_extra.iter().filter(|m| m.source == source.id) {
            members.extend(extra.notes.iter().cloned());
        }
        let card_ids: Vec<String> = members
            .iter()
            .flat_map(|n| cards.get(n).cloned().unwrap_or_default())
            .collect();
        let mut reps: BTreeMap<String, u32> = BTreeMap::new();
        for chunk in card_ids.chunks(500) {
            for card in client.cards_info(chunk)? {
                let note = linguist_anki::wire_id(&card["note"])?;
                *reps.entry(note).or_default() += card["reps"].as_u64().unwrap_or(0) as u32;
            }
        }
        for note in &mut notes {
            note.reps = reps.get(&note.id).copied().unwrap_or(0);
        }
        matched.insert(source.id.clone(), notes);
    }
    Ok(matched)
}

fn create(
    globals: &Globals,
    settings: &linguist_config::Effective,
    spec_path: &Path,
    dry_run: bool,
) -> Result<u8, String> {
    let bytes = std::fs::read(spec_path).map_err(|_| "CAMPAIGN_SPEC_READ_IO")?;
    let spec: Spec =
        serde_json::from_slice(&bytes).map_err(|e| format!("CAMPAIGN_SPEC_INVALID: {e}"))?;
    spec.validate()?;
    check_overrides(globals, &spec)?;
    let client = crate::anki_client(settings)?;
    let matched = read_sources(&client, &spec)?;
    let (batches, selection) = select::build(&spec, &matched)?;
    if dry_run {
        let preview: Vec<Value> = batches
            .iter()
            .take(40)
            .map(|(cohort, data)| json!({"cohort": cohort, "notes": data.notes.len(), "merge": data.merge, "pilot": data.pilot}))
            .collect();
        emit(
            &json!({"schema_version": 1, "dry_run": true, "selection": selection, "first_batches": preview}),
        )?;
        return Ok(0);
    }
    if batches.is_empty() {
        return Err("CAMPAIGN_EMPTY: no note matched an ordered cohort".into());
    }
    let root = store_root(settings)?;
    // Opening the store creates the private state directory first.
    let mut store = linguist_store::Store::open(&root)?;
    let id = Uuid::new_v4();
    let directory = root.join("campaigns").join(id.to_string());
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)
            .map_err(|_| "CAMPAIGN_DIRECTORY_IO")?;
    }
    let policy_path = directory.join("policy.json");
    std::fs::write(
        &policy_path,
        serde_json::to_vec_pretty(&spec.policy()).map_err(|e| e.to_string())?,
    )
    .map_err(|_| "CAMPAIGN_POLICY_WRITE_IO")?;
    for source in spec
        .sources
        .iter()
        .filter(|s| !s.extra_agent_codes.is_empty())
    {
        let path = campaign::source_policy_path(&policy_path.to_string_lossy(), &source.id);
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&spec.source_policy(source)).map_err(|e| e.to_string())?,
        )
        .map_err(|_| "CAMPAIGN_POLICY_WRITE_IO")?;
    }
    let definition = Definition {
        schema_version: campaign::SCHEMA_VERSION,
        spec,
        created_ms: now(),
        global_args: global_args(globals)?,
        policy_path: policy_path.to_string_lossy().into_owned(),
        selection: selection.clone(),
    };
    let new: Vec<linguist_store::campaign::NewBatch> = batches
        .into_iter()
        .enumerate()
        .map(|(i, (cohort, data))| linguist_store::campaign::NewBatch {
            batch: i as u32 + 1,
            cohort,
            phase: "selected".into(),
            data: data.to_value(),
        })
        .collect();
    let digest = store.create_campaign(
        id,
        &serde_json::to_value(&definition).map_err(|e| e.to_string())?,
        &new,
        now(),
    )?;
    emit(
        &json!({"schema_version": 1, "campaign": id, "definition_digest": digest,
        "selection": selection, "policy_path": definition.policy_path,
        "next_command": format!("linguist-anki-bridge campaign run {id}")}),
    )?;
    Ok(0)
}

fn context<'a>(
    root: &'a Path,
    id: Uuid,
    runner: &'a ChildRunner,
    collection: &'a Companion<'a>,
    clock: &'a (dyn Fn() -> u64 + Sync),
) -> Result<Context<'a>, String> {
    Ok(Context {
        root,
        id,
        definition: definition(root, id)?,
        runner,
        collection,
        clock,
        poll: std::time::Duration::from_secs(20),
    })
}

fn set_status(
    root: &Path,
    id: Uuid,
    status: &str,
    code: Option<&str>,
    detail: Option<&str>,
    kind: &str,
) -> Result<Value, String> {
    let mut store = linguist_store::Store::open_existing(root)?;
    let record = store.campaign(id)?;
    if matches!(record.status.as_str(), "cancelled" | "closed") {
        return Err(format!(
            "CAMPAIGN_TERMINAL: the campaign is {}",
            record.status
        ));
    }
    store.set_campaign_status(
        id,
        record.version,
        status,
        code,
        detail,
        kind,
        &json!({"code": code, "detail": detail}),
        now(),
    )?;
    Ok(json!({"campaign": id, "status": status}))
}

pub(crate) fn handle(
    globals: &Globals,
    settings: &linguist_config::Effective,
    command: CampaignCommand,
) -> Result<u8, String> {
    if let CampaignCommand::Create { spec, dry_run } = &command {
        return create(globals, settings, spec, *dry_run);
    }
    let root = store_root(settings)?;
    let exe = std::env::current_exe().map_err(|_| "CAMPAIGN_EXECUTABLE_UNKNOWN")?;
    match command {
        CampaignCommand::Create { .. } => unreachable!(),
        CampaignCommand::List => {
            let store = linguist_store::Store::read_only(&root)?;
            let mut list = Vec::new();
            for id in store.campaign_ids()? {
                let record = store.campaign(id)?;
                list.push(
                    json!({"campaign": id, "name": record.definition["spec"]["name"],
                    "status": record.status, "code": record.code, "created_ms": record.created_ms}),
                );
            }
            emit(&json!({"schema_version": 1, "campaigns": list}))?;
        }
        CampaignCommand::Status { campaign } => {
            let id = match campaign {
                Some(id) => id,
                None => newest(&root)?,
            };
            let store = linguist_store::Store::read_only(&root)?;
            emit(&engine::status(&store, id, now())?)?;
        }
        CampaignCommand::Batch { campaign, batch } => {
            let store = linguist_store::Store::read_only(&root)?;
            emit(
                &serde_json::to_value(store.campaign_batch(campaign, batch)?)
                    .map_err(|e| e.to_string())?,
            )?;
        }
        CampaignCommand::Events {
            campaign,
            after,
            limit,
        } => {
            let store = linguist_store::Store::read_only(&root)?;
            emit(&json!({"events": store.campaign_events(campaign, after, limit)?}))?;
        }
        CampaignCommand::Run {
            campaign,
            max_seconds,
        } => {
            let id = match campaign {
                Some(id) => id,
                None => newest(&root)?,
            };
            let def = definition(&root, id)?;
            let runner = ChildRunner {
                exe,
                global_args: def.global_args.clone(),
            };
            let client = crate::anki_client(settings)?;
            let companion = Companion {
                port: Mutex::new(None),
                client: &client,
                settings,
            };
            let clock = || now();
            let ctx = context(&root, id, &runner, &companion, &clock)?;
            let report = ctx.tick(max_seconds)?;
            let stopped = report.stopped.clone();
            emit(&serde_json::to_value(report).map_err(|e| e.to_string())?)?;
            return Ok(
                if stopped.starts_with("CAMPAIGN_PAUSED") || stopped == "CAMPAIGN_PILOT_REVIEW" {
                    5
                } else {
                    0
                },
            );
        }
        CampaignCommand::Approve {
            campaign,
            batch,
            actor,
        } => {
            let def = definition(&root, campaign)?;
            if linguist_store::Store::read_only(&root)?
                .campaign(campaign)?
                .status
                != "running"
            {
                return Err(
                    "CAMPAIGN_NOT_RUNNING: approvals wait while the campaign is paused".into(),
                );
            }
            let runner = ChildRunner {
                exe,
                global_args: def.global_args.clone(),
            };
            let client = crate::anki_client(settings)?;
            let companion = Companion {
                port: Mutex::new(None),
                client: &client,
                settings,
            };
            let clock = || now();
            let ctx = context(&root, campaign, &runner, &companion, &clock)?;
            emit(&ctx.approve(batch, &actor)?)?;
        }
        CampaignCommand::Item {
            campaign,
            batch,
            note,
            settled,
            defer,
            undefer,
        } => {
            let action = match (settled, defer, undefer) {
                (true, None, false) => ItemAction::Settled,
                (false, Some(reason), false) => ItemAction::Defer(reason),
                (false, None, true) => ItemAction::Undefer,
                _ => return Err(
                    "CAMPAIGN_ITEM_ACTION_REQUIRED: pass --settled, --defer REASON or --undefer"
                        .into(),
                ),
            };
            let def = definition(&root, campaign)?;
            let runner = ChildRunner {
                exe,
                global_args: def.global_args.clone(),
            };
            let client = crate::anki_client(settings)?;
            let companion = Companion {
                port: Mutex::new(None),
                client: &client,
                settings,
            };
            let clock = || now();
            emit(
                &context(&root, campaign, &runner, &companion, &clock)?
                    .item(batch, &note, action)?,
            )?;
        }
        CampaignCommand::Reopen {
            campaign,
            batch,
            reason,
            fresh,
        } => {
            let def = definition(&root, campaign)?;
            let runner = ChildRunner {
                exe,
                global_args: def.global_args.clone(),
            };
            let client = crate::anki_client(settings)?;
            let companion = Companion {
                port: Mutex::new(None),
                client: &client,
                settings,
            };
            let clock = now;
            emit(
                &context(&root, campaign, &runner, &companion, &clock)?
                    .reopen(batch, &reason, fresh)?,
            )?;
        }
        CampaignCommand::Skip {
            campaign,
            batch,
            reason,
        } => {
            let def = definition(&root, campaign)?;
            let runner = ChildRunner {
                exe,
                global_args: def.global_args.clone(),
            };
            let client = crate::anki_client(settings)?;
            let companion = Companion {
                port: Mutex::new(None),
                client: &client,
                settings,
            };
            let clock = || now();
            emit(&context(&root, campaign, &runner, &companion, &clock)?.skip(batch, &reason)?)?;
        }
        CampaignCommand::Pause { campaign, reason } => {
            emit(&set_status(
                &root,
                campaign,
                "paused",
                Some("CAMPAIGN_PAUSED_BY_REQUEST"),
                Some(&reason),
                "paused",
            )?)?;
        }
        CampaignCommand::Resume { campaign, by_user } => {
            use std::io::IsTerminal;
            if !by_user && !std::io::stdin().is_terminal() {
                return Err("CAMPAIGN_RESUME_BY_USER_REQUIRED: only the user resumes a paused campaign; run it in a terminal or pass --by-user".into());
            }
            emit(&set_status(
                &root, campaign, "running", None, None, "resumed",
            )?)?;
        }
        CampaignCommand::Cancel { campaign } => {
            emit(&set_status(
                &root,
                campaign,
                "cancelled",
                None,
                None,
                "cancelled",
            )?)?;
        }
        CampaignCommand::Close { campaign } => {
            emit(&set_status(
                &root, campaign, "closed", None, None, "closed",
            )?)?;
            eprintln!("Campaign closed. Sync to AnkiWeb now and choose Upload.");
        }
        CampaignCommand::Asset { digest, output } => {
            let bytes =
                linguist_store::Store::read_only(&root)?.asset(&digest, 256 * 1024 * 1024)?;
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .map_err(|_| "CAMPAIGN_ASSET_OUTPUT_EXISTS_OR_IO")?;
            file.write_all(&bytes)
                .map_err(|_| "CAMPAIGN_ASSET_OUTPUT_IO")?;
            emit(&json!({"digest": digest, "output": output, "size_bytes": bytes.len()}))?;
        }
        CampaignCommand::AgentSlot { campaign } => {
            let def = definition(&root, campaign)?;
            let mut store = linguist_store::Store::open_existing(&root)?;
            let day = now() - now() % 86_400_000;
            let used = store.count_campaign_events(campaign, "agent_run", day)?;
            if used >= u64::from(def.spec.limits.agent_runs_per_day) {
                emit(&json!({"granted": false, "used_today": used}))?;
                return Ok(5);
            }
            store.append_campaign_event(campaign, None, "agent_run", &json!({}), now())?;
            emit(&json!({"granted": true, "used_today": used + 1}))?;
        }
        CampaignCommand::Requeue { campaign, reasons } => {
            definition(&root, campaign)?;
            if reasons.iter().any(|r| r.is_empty()) {
                return Err("CAMPAIGN_REASON_EMPTY".into());
            }
            let mut store = linguist_store::Store::open_existing(&root)?;
            emit(&engine::requeue(&mut store, campaign, &reasons, now())?)?;
        }
        CampaignCommand::AgentDone { campaign } => {
            definition(&root, campaign)?;
            let mut store = linguist_store::Store::open_existing(&root)?;
            store.append_campaign_event(campaign, None, "agent_done", &json!({}), now())?;
            emit(&json!({"campaign": campaign, "agent_done": true}))?;
        }
        CampaignCommand::Tune {
            campaign,
            prepare_ahead,
            prepare_parallel,
            agent_runs_per_day,
        } => {
            let tuning = campaign::Tuning {
                prepare_ahead,
                prepare_parallel,
                agent_runs_per_day,
            };
            if tuning.is_empty() {
                return Err("CAMPAIGN_TUNING_EMPTY: give at least one limit".into());
            }
            let mut limits = definition(&root, campaign)?.spec.limits;
            tuning.apply(&mut limits)?;
            let mut store = linguist_store::Store::open_existing(&root)?;
            if matches!(
                store.campaign(campaign)?.status.as_str(),
                "cancelled" | "closed"
            ) {
                return Err("CAMPAIGN_FINISHED".into());
            }
            store.append_campaign_event(
                campaign,
                None,
                "tuned",
                &json!({"tuning": tuning, "limits": limits}),
                now(),
            )?;
            emit(&json!({"campaign": campaign, "limits": limits,
                "note": "a running tick keeps its limits; the next tick uses these"}))?;
        }
        CampaignCommand::InstallTimer {
            campaign,
            interval_minutes,
            script,
            agent_script,
            agent_interval_minutes,
            workdir,
            env,
            execute,
        } => {
            if let Some(bad) = env.iter().find(|e| {
                e.contains('\n')
                    || !e.split_once('=').is_some_and(|(k, _)| {
                        !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    })
            }) {
                return Err(format!("CAMPAIGN_ENV_INVALID: {bad}"));
            }
            definition(&root, campaign)?;
            let script = std::fs::canonicalize(&script).map_err(|_| "CAMPAIGN_SCRIPT_NOT_FOUND")?;
            let workdir =
                std::fs::canonicalize(&workdir).map_err(|_| "CAMPAIGN_WORKDIR_NOT_FOUND")?;
            let agent_script = agent_script
                .map(|p| std::fs::canonicalize(p).map_err(|_| "CAMPAIGN_SCRIPT_NOT_FOUND"))
                .transpose()?;
            if interval_minutes == 0 || agent_interval_minutes == 0 {
                return Err("CAMPAIGN_INTERVAL_INVALID".into());
            }
            let units = timer_units(
                campaign,
                interval_minutes,
                &exe,
                &script,
                agent_script
                    .as_deref()
                    .map(|script| (script, agent_interval_minutes)),
                &workdir,
                &env,
            );
            let home = std::env::var("HOME").map_err(|_| "HOME_UNSET")?;
            let directory = PathBuf::from(home).join(".config/systemd/user");
            let mut written = Vec::new();
            if execute {
                std::fs::create_dir_all(&directory).map_err(|_| "CAMPAIGN_UNIT_IO")?;
                for (name, body) in &units {
                    let path = directory.join(name);
                    std::fs::write(&path, body).map_err(|_| "CAMPAIGN_UNIT_IO")?;
                    written.push(path.to_string_lossy().into_owned());
                }
            }
            emit(
                &json!({"units": units.iter().map(|(n, b)| json!({"name": n, "body": b})).collect::<Vec<_>>(),
                "written": written,
                "next_commands": if agent_script.is_some() {
                    json!(["systemctl --user daemon-reload", "systemctl --user enable --now linguist-campaign.timer linguist-campaign-agent.timer"])
                } else {
                    json!(["systemctl --user daemon-reload", "systemctl --user enable --now linguist-campaign.timer"])
                }}),
            )?;
        }
    }
    Ok(0)
}

fn timer_units(
    campaign: Uuid,
    interval: u32,
    exe: &Path,
    script: &Path,
    agent: Option<(&Path, u32)>,
    workdir: &Path,
    env: &[String],
) -> Vec<(String, String)> {
    let extra: String = env
        .iter()
        .map(|e| format!("Environment=\"{e}\"\n"))
        .collect();
    let service = format!(
        "[Unit]\nDescription=Linguist collection revamp campaign tick\nAfter=network-online.target\n\n[Service]\nType=oneshot\nWorkingDirectory={wd}\nEnvironment=LAB_CAMPAIGN={campaign}\nEnvironment=LAB_BIN={exe}\n{extra}ExecStart={script}\n# Child commands finish their own durable step even if the tick is stopped.\nKillMode=mixed\nTimeoutStopSec=600\n",
        wd = workdir.display(),
        exe = exe.display(),
        script = script.display(),
    );
    let timer = format!(
        "[Unit]\nDescription=Run the Linguist campaign tick every {interval} minutes\n\n[Timer]\nOnBootSec=5min\nOnUnitInactiveSec={interval}min\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
    );
    let mut units = vec![
        ("linguist-campaign.service".into(), service),
        ("linguist-campaign.timer".into(), timer),
    ];
    if let Some((agent, minutes)) = agent {
        units.push((
            "linguist-campaign-agent.service".into(),
            format!(
                "[Unit]\nDescription=Linguist collection revamp campaign agent\nAfter=network-online.target\n\n[Service]\nType=oneshot\nWorkingDirectory={wd}\nEnvironment=LAB_CAMPAIGN={campaign}\nEnvironment=LAB_BIN={exe}\n{extra}ExecStart={script}\nTimeoutStartSec=40min\n",
                wd = workdir.display(),
                exe = exe.display(),
                script = agent.display(),
            ),
        ));
        units.push((
            "linguist-campaign-agent.timer".into(),
            format!(
                "[Unit]\nDescription=Run the Linguist campaign agent every {minutes} minutes\n\n[Timer]\nOnBootSec=7min\nOnUnitInactiveSec={minutes}min\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
            ),
        ));
    }
    units
}

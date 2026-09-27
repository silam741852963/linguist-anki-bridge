//! Durable existing-note preparation. Collection mutation is never performed here.
use linguist_core::records::*;
use std::collections::BTreeMap;

/// Run one bounded source-capture worker and publish a complete review-required draft.
pub fn run(
    root: &std::path::Path,
    job: uuid::Uuid,
    environment: &BTreeMap<String, String>,
) -> Result<serde_json::Value, String> {
    use linguist_store::preparation::PreparationStage;
    // Read-only preflight: unavailable capabilities cannot create checkpoints or leases.
    let definition = linguist_store::Store::read_only(root)?.preparation_job(job)?;
    let frozen = &definition.job.settings;
    if std::path::Path::new(frozen.values["storage.state_dir"].as_str().unwrap()) != root {
        return Err("PREPARATION_STORAGE_CONFLICT".into());
    }
    let settings = linguist_config::Effective {
        version: frozen.version,
        values: frozen.values.clone(),
        provenance: frozen.provenance.clone(),
        fingerprint: frozen.fingerprint.clone(),
    };
    crate::revamp::validate_source_revamp(&settings, &definition.selection.purpose, environment)?;
    let registry = linguist_config::Registry::builtin();
    for key in [
        "jobs.lease_seconds",
        "jobs.heartbeat_seconds",
        "jobs.on_item_error",
        "jobs.prepare_workers",
    ] {
        registry.validate_value(key, settings.values.get(key).ok_or("JOB_SETTING_MISSING")?)?;
    }
    let seconds = settings.values["jobs.lease_seconds"].as_u64().unwrap();
    let heartbeat = settings.values["jobs.heartbeat_seconds"].as_u64().unwrap();
    if heartbeat * 3 >= seconds {
        return Err("JOB_HEARTBEAT_INTERVAL_INVALID".into());
    }
    let client = linguist_anki::Client::from_settings(&settings, environment)?;
    let mut store = linguist_store::Store::open(root)?;
    let lease = store.acquire_lease(&linguist_store::lease::Resource::JobWorker(job), seconds)?;
    let result = (|| {
        let mut head = None;
        let mut sequence = 0;
        loop {
            let events = store.preparation_events(job, sequence, 1000)?;
            if events.is_empty() {
                break;
            }
            let last = events.last().unwrap();
            sequence = last.event.sequence;
            head = Some(last.digest.clone());
        }
        let mut captured = 0u32;
        let mut failed = 0u32;
        for index in 0..definition.job.item_ids.len() {
            if store.preparation_items(job, index as u32, 1)?[0].state == "started" {
                return Err("PREPARATION_ACTIVE_ITEM_REQUIRES_RECOVERY".into());
            }
        }
        for index in 0..definition.job.item_ids.len() {
            let item = store.preparation_items(job, index as u32, 1)?.remove(0);
            if item.state == "captured" {
                continue;
            }
            // An interrupted dispatch requires explicit recovery, never implicit replay.
            if item.state == "started" {
                return Err("PREPARATION_ACTIVE_ITEM_REQUIRES_RECOVERY".into());
            }
            if item.state == "failed" && !item.retry_eligible {
                continue;
            }
            store.renew_lease(&lease, seconds)?;
            store.validate_lease(&lease)?;
            let attempt = item.attempt + 1;
            let started = store.append_preparation_event(
                job,
                item.item_id,
                attempt,
                PreparationStage::Started,
                head.as_deref(),
            )?;
            head = Some(started.digest);
            let id = &definition.selection.selected_note_ids[index];
            let capture = std::thread::scope(|scope| {
                let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                let client = &client;
                let settings = &settings;
                let purpose = &definition.selection.purpose;
                scope.spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::source_archive::capture_for_revamp(client, settings, purpose, id)
                    }))
                    .unwrap_or_else(|_| Err("SOURCE_CAPTURE_WORKER_PANIC".into()));
                    let _ = sender.send(result);
                });
                loop {
                    match receiver.recv_timeout(std::time::Duration::from_secs(heartbeat)) {
                        Ok(result) => break result,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            store.renew_lease(&lease, seconds)?
                        }
                        Err(_) => break Err("SOURCE_CAPTURE_WORKER_DISCONNECTED".into()),
                    }
                }
            });
            store.renew_lease(&lease, seconds)?;
            store.validate_lease(&lease)?;
            let prepared = capture.and_then(|capture| {
                let document = crate::revamp::stage_document(
                    &capture,
                    &settings,
                    &definition.selection.purpose,
                )?;
                for (digest, bytes) in &capture.captured.assets {
                    if store.publish_asset(
                        bytes,
                        settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024,
                    )? != *digest
                    {
                        return Err("SOURCE_CAPTURE_ASSET_DIGEST_CONFLICT".into());
                    }
                }
                Ok(document)
            });
            let (stage, stop) = match prepared {
                Ok(document) => {
                    captured += 1;
                    (
                        PreparationStage::Captured {
                            document: Box::new(document),
                        },
                        false,
                    )
                }
                Err(error) => {
                    failed += 1;
                    let code = match error.as_str() {
                        "ANKI_READ_TIMEOUT" => "SOURCE_READ_TIMEOUT",
                        "ANKI_DEPENDENCY_UNAVAILABLE" => "SOURCE_READ_CONNECTION_FAILED",
                        "ANKI_HTTP_FAILURE: 429" => "SOURCE_READ_RATE_LIMITED",
                        "ANKI_HTTP_FAILURE: 503" => "SOURCE_READ_UNAVAILABLE",
                        _ => "SOURCE_CAPTURE_REVIEW_REQUIRED",
                    };
                    let retry_eligible = code != "SOURCE_CAPTURE_REVIEW_REQUIRED";
                    (
                        PreparationStage::Failed {
                            code: code.into(),
                            retry_eligible,
                        },
                        !retry_eligible || settings.values["jobs.on_item_error"] == "stop",
                    )
                }
            };
            store.validate_lease(&lease)?;
            let receipt = store.append_preparation_event(
                job,
                item.item_id,
                attempt,
                stage,
                head.as_deref(),
            )?;
            head = Some(receipt.digest);
            if stop {
                break;
            }
        }
        store.renew_lease(&lease, seconds)?;
        let plan = store.publish_preparation_plan(job, head.as_deref(), &lease)?;
        let mut counts = BTreeMap::<String, u32>::new();
        let mut errors = BTreeMap::<String, u32>::new();
        for offset in (0..definition.job.item_ids.len()).step_by(1000) {
            for item in store.preparation_items(job, offset as u32, 1000)? {
                *counts.entry(item.state).or_default() += 1;
                if let Some(code) = item.error_code {
                    *errors.entry(code).or_default() += 1;
                }
            }
        }
        // Earlier failures remain non-success when this invocation dispatches nothing.
        let exit_code = if plan.is_some() || errors.contains_key("SOURCE_CAPTURE_REVIEW_REQUIRED") {
            4
        } else if errors.keys().any(|code| {
            !matches!(
                code.as_str(),
                "SOURCE_READ_CONNECTION_FAILED" | "SOURCE_READ_UNAVAILABLE"
            )
        }) {
            6
        } else if !errors.is_empty() {
            3
        } else {
            0
        };
        Ok(
            serde_json::json!({"schema_version":2,"job_id":job,"stage":"source_draft","captured_this_run":captured,"failed_this_run":failed,"item_counts":counts,"error_counts":errors,"exit_code":exit_code,"checkpoint_digest":head,"plan_published":plan.is_some(),"plan":plan,"ready":false,"writes_enabled":false}),
        )
    })();
    let released = store.release_lease(&lease);
    match result {
        Ok(value) => {
            released?;
            Ok(value)
        }
        Err(error) => Err(error),
    }
}

pub fn create(
    purpose: &str,
    note_ids: Vec<String>,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
) -> Result<serde_json::Value, String> {
    if note_ids.is_empty() || note_ids.len() > 100000 {
        return Err("JOB_INPUT_REQUIRED_OR_LIMIT".into());
    }
    for key in [
        "selection.max_notes",
        "selection.order",
        "jobs.max_item_attempts",
        "input.max_file_mb",
    ] {
        linguist_config::Registry::builtin()
            .validate_value(key, settings.values.get(key).ok_or("JOB_SETTING_MISSING")?)?;
    }
    for id in &note_ids {
        linguist_anki::wire_id(&serde_json::json!(id))?;
    }
    let mut selected = note_ids.clone();
    if settings.values["selection.order"] == "note_id" {
        selected.sort_by_key(|id| id.parse::<u64>().unwrap());
    }
    let frozen = crate::freeze_settings(settings, environment)?;
    let selection = SelectionReceipt {
        schema_version: 1,
        purpose: purpose.into(),
        selector: SelectionInput::NoteIds(note_ids.clone()),
        matched_note_ids: note_ids,
        selected_note_ids: selected.clone(),
        order: settings.values["selection.order"].as_str().unwrap().into(),
        max_notes: settings.values["selection.max_notes"].as_u64().unwrap(),
        command_limit: None,
    };
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "CLOCK_UNAVAILABLE")?
        .as_secs();
    let definition = linguist_store::preparation::PreparationDefinition {
        schema_version: 1,
        selection,
        created_at: format!("unix-seconds:{seconds}"),
        job: Job {
            id: uuid::Uuid::new_v4(),
            mode: JobMode::Prepare,
            settings: frozen,
            plan_refs: selected
                .iter()
                .map(|id| format!("anki-note:{id}"))
                .collect(),
            item_ids: selected.iter().map(|_| uuid::Uuid::new_v4()).collect(),
            pause_requested: false,
            cancel_requested: false,
        },
    };
    definition.validate()?;
    let root = std::path::PathBuf::from(
        definition.job.settings.values["storage.state_dir"]
            .as_str()
            .ok_or("JOB_STORAGE_SETTING_MISSING")?,
    );
    let digest = linguist_store::Store::open(&root)?.create_preparation_job(&definition)?;
    Ok(
        serde_json::json!({"schema_version":2,"job_id":definition.job.id,"mode":"prepare","digest":digest,"input_count":selected.len(),"state":"queued","worker_started":false,"execution_available":false,"writes_enabled":false}),
    )
}

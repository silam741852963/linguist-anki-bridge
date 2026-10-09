//! Durable existing-note preparation. Collection mutation is never performed here.
use linguist_core::records::*;
use std::collections::BTreeMap;

/// Reconcile a bounded page of interrupted read captures; never dispatches work.
pub fn recover(
    root: &std::path::Path,
    job: uuid::Uuid,
    after_index: u32,
    limit: u32,
    actor: Option<&str>,
    execute: bool,
) -> Result<serde_json::Value, String> {
    if !(1..=1000).contains(&limit) {
        return Err("JOB_RECOVERY_LIMIT_INVALID".into());
    }
    if execute
        && actor.is_none_or(|actor| {
            actor.trim().is_empty()
                || actor.chars().count() > 200
                || actor.chars().any(char::is_control)
        })
    {
        return Err("JOB_RECOVERY_ACTOR_REQUIRED".into());
    }
    let reader = linguist_store::Store::read_only(root)?;
    let definition = reader.preparation_job(job)?;
    if std::path::Path::new(
        definition.job.settings.values["storage.state_dir"]
            .as_str()
            .unwrap(),
    ) != root
    {
        return Err("PREPARATION_STORAGE_CONFLICT".into());
    }
    let preview = reader.preparation_items(job, after_index, limit)?;
    let next = preview.last().map(|item| item.index + 1);
    let started: Vec<_> = preview
        .into_iter()
        .filter(|item| item.state == "started")
        .collect();
    if !execute {
        return Ok(
            serde_json::json!({"schema_version":2,"job_id":job,"scope":"item_page","started":started,"next_index":next,"executed":false,"worker_liveness":"unverified","writes_enabled":false}),
        );
    }
    let seconds = definition
        .job
        .settings
        .values
        .get("jobs.lease_seconds")
        .ok_or("JOB_SETTING_MISSING")?;
    linguist_config::Registry::builtin().validate_value("jobs.lease_seconds", seconds)?;
    let seconds = seconds.as_u64().unwrap();
    drop(reader);
    let mut store = linguist_store::Store::open_existing(root)?;
    let lease = store.acquire_lease(&linguist_store::lease::Resource::JobWorker(job), seconds)?;
    let result = (|| -> Result<serde_json::Value, String> {
        // Re-read after ownership acquisition: the preview is not a mutation authority.
        let page = store.preparation_items(job, after_index, limit)?;
        let next = page.last().map(|item| item.index + 1);
        let mut head = store.preparation_head(job)?.map(|receipt| receipt.digest);
        let mut reconciled = Vec::new();
        for item in page.into_iter().filter(|item| item.state == "started") {
            store.renew_lease(&lease, seconds)?;
            let receipt = store.append_preparation_event_with_lease(
                job,
                item.item_id,
                item.attempt,
                linguist_store::preparation::PreparationStage::Interrupted {
                    actor: actor.unwrap().into(),
                },
                head.as_deref(),
                &lease,
            )?;
            head = Some(receipt.digest.clone());
            reconciled.push(receipt);
        }
        Ok(
            serde_json::json!({"schema_version":2,"job_id":job,"scope":"item_page","executed":true,"reconciled":reconciled,"next_index":next,"checkpoint_digest":head,"control":store.preparation_control(job)?,"work_dispatched":false,"attempts_reset":false,"writes_enabled":false}),
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

/// Read-only history page: summaries retain evidence references without document bodies.
pub fn audit(
    store: &linguist_store::Store,
    job: uuid::Uuid,
    after_checkpoint: u32,
    after_control: u32,
    limit: u32,
) -> Result<serde_json::Value, String> {
    use linguist_store::preparation::PreparationStage;
    let definition = store.preparation_job(job)?;
    let inputs: BTreeMap<_, _> = definition
        .job
        .item_ids
        .iter()
        .zip(&definition.job.plan_refs)
        .collect();
    let mut checkpoints = Vec::new();
    // Stream one captured document at a time; output never accumulates their full bodies.
    if !(1..=1000).contains(&limit) {
        return Err("JOB_AUDIT_PAGE_LIMIT: select --limit between 1 and 1000".into());
    }
    store.visit_preparation_events(job, after_checkpoint, limit, |receipt| {
        let cursor = receipt.event.sequence;
        let item = receipt.event.item_id;
        let input = inputs.get(&item).ok_or("PREPARATION_EVENT_ITEM_CONFLICT")?;
        let detail = match &receipt.event.stage {
            PreparationStage::Interrupted { actor } => serde_json::json!({"state":"interrupted","actor":actor,"code":"SOURCE_READ_INTERRUPTED","retry_classified":true}),
            PreparationStage::Started => serde_json::json!({"state":"started"}),
            PreparationStage::Failed {
                code,
                retry_eligible,
            } => {
                serde_json::json!({"state":"failed","error_code":code,"retry_classified":retry_eligible})
            }
            PreparationStage::Captured { document } => {
                let assets: std::collections::BTreeSet<_> = document
                    .archives
                    .iter()
                    .flat_map(|archive| &archive.asset_digests)
                    .chain(document.media.iter().map(|media| &media.digest))
                    .collect();
                serde_json::json!({"state":"captured","document_id":document.id,"semantic_digest":document.semantic_digest().map_err(|e| e.to_string())?,"asset_digests":assets,"source_digests":document.sources.iter().map(|source| &source.digest).collect::<Vec<_>>()})
            }
        };
        checkpoints.push(serde_json::json!({"sequence":cursor,"digest":receipt.digest,"parent_digest":receipt.event.parent_digest,"item_id":item,"input_ref":input,"attempt":receipt.event.attempt,"detail":detail}));
        Ok(())
    })?;
    let controls = store.preparation_controls(job, after_control, limit)?;
    let next_checkpoint = checkpoints.last().map(|value| value["sequence"].clone());
    let next_control = controls.last().map(|receipt| receipt.event.sequence);
    let source_plan = match store.revision(job, 1) {
        Err(error) if error == "PLAN_NOT_FOUND" => None,
        Err(error) => return Err(error),
        Ok(plan) => {
            if plan.settings != definition.job.settings
                || plan.selection.as_ref() != Some(&definition.selection)
            {
                return Err("PREPARATION_PLAN_CONFLICT".into());
            }
            Some(
                serde_json::json!({"id":job,"revision":1,"digest":plan.approval_digest().map_err(|e| e.to_string())?,"retained_assets_verified":true,"checkpoint_binding_verified":false}),
            )
        }
    };
    Ok(
        serde_json::json!({"schema_version":2,"job_id":job,"scope":"local_history_page","checkpoints":checkpoints,"controls":controls,"next_checkpoint_sequence":next_checkpoint,"next_control_sequence":next_control,"source_plan":source_plan,"page_hashes_and_links_verified":true,"full_history_checked":false,"native_verified":false,"worker_liveness":"unverified","writes_enabled":false}),
    )
}

/// Run bounded source-capture workers and publish a complete review-required draft.
pub fn run(
    root: &std::path::Path,
    job: uuid::Uuid,
    environment: &BTreeMap<String, String>,
) -> Result<serde_json::Value, String> {
    use linguist_store::preparation::PreparationStage;
    // Read-only preflight: unavailable capabilities cannot create checkpoints or leases.
    let reader = linguist_store::Store::read_only(root)?;
    let definition = reader.preparation_job(job)?;
    if reader.job_tombstone(job)?.is_some() {
        return Err("JOB_TOMBSTONED: a deleted job cannot run".into());
    }
    drop(reader);
    let frozen = &definition.job.settings;
    if std::path::Path::new(frozen.values["storage.state_dir"].as_str().unwrap()) != root {
        return Err("PREPARATION_STORAGE_CONFLICT".into());
    }
    let settings = linguist_config::Effective {
        version: frozen.version,
        values: frozen.values.clone(),
        provenance: frozen.provenance.clone(),
        fingerprint: frozen.fingerprint.clone(),
        semantic_fingerprint: frozen.semantic_fingerprint.clone(),
        execution_fingerprint: frozen.execution_fingerprint.clone(),
    };
    crate::revamp::validate_source_revamp(&settings, &definition.selection.purpose, environment)?;
    let registry = linguist_config::Registry::builtin();
    for key in [
        "jobs.lease_seconds",
        "jobs.heartbeat_seconds",
        "jobs.on_item_error",
        "jobs.prepare_workers",
        "jobs.pause_poll_ms",
    ] {
        registry.validate_value(key, settings.values.get(key).ok_or("JOB_SETTING_MISSING")?)?;
    }
    let seconds = settings.values["jobs.lease_seconds"].as_u64().unwrap();
    let heartbeat = settings.values["jobs.heartbeat_seconds"].as_u64().unwrap();
    if heartbeat * 3 >= seconds {
        return Err("JOB_HEARTBEAT_INTERVAL_INVALID".into());
    }
    let client = linguist_anki::Client::from_settings(&settings, environment)?;
    let mut store = linguist_store::Store::open_existing(root)?;
    let lease = store.acquire_lease(&linguist_store::lease::Resource::JobWorker(job), seconds)?;
    let result = (|| {
        let mut head = store.preparation_head(job)?.map(|receipt| receipt.digest);
        let mut captured = 0u32;
        let mut failed = 0u32;
        let mut eligible = Vec::new();
        // Load bounded pages once rather than rereading the whole definition per input.
        // Check every interrupted dispatch before permitting any new source reads.
        for offset in (0..definition.job.item_ids.len()).step_by(1000) {
            store.renew_lease(&lease, seconds)?;
            for item in store.preparation_items(job, offset as u32, 1000)? {
                if item.state == "started" {
                    return Err("PREPARATION_ACTIVE_ITEM_REQUIRES_RECOVERY".into());
                }
                if item.state == "pending" || item.retry_eligible {
                    eligible.push(item);
                }
            }
        }
        let workers = settings.values["jobs.prepare_workers"].as_u64().unwrap() as usize;
        let poll = std::time::Duration::from_millis(
            settings.values["jobs.pause_poll_ms"].as_u64().unwrap(),
        );
        let heartbeat_interval = std::time::Duration::from_secs(heartbeat);
        for wave in eligible.chunks(workers) {
            if store.preparation_control(job)?.is_some_and(|receipt| {
                receipt.event.action != linguist_store::preparation_control::ControlAction::Resume
            }) {
                break;
            }
            let stop = std::thread::scope(|scope| -> Result<bool, String> {
                // Capacity covers the entire wave, so fencing errors cannot strand senders.
                let (sender, receiver) = std::sync::mpsc::sync_channel(wave.len());
                let mut dispatched = 0;
                for item in wave {
                    store.renew_lease(&lease, seconds)?;
                    let attempt = item.attempt + 1;
                    let started = match store.append_preparation_event_with_lease(
                        job,
                        item.item_id,
                        attempt,
                        PreparationStage::Started,
                        head.as_deref(),
                        &lease,
                    ) {
                        Ok(started) => started,
                        Err(error) if error == "PREPARATION_CONTROL_BLOCKS_DISPATCH" => break,
                        Err(error) => return Err(error),
                    };
                    head = Some(started.digest);
                    dispatched += 1;
                    let client = &client;
                    let settings = &settings;
                    let purpose = &definition.selection.purpose;
                    let id = &definition.selection.selected_note_ids[item.index as usize];
                    let sender = sender.clone();
                    scope.spawn(move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let capture = crate::source_archive::capture_for_revamp(
                                client, settings, purpose, id,
                            )?;
                            // Decode, stage and enrich in the worker so the
                            // coordinator can heartbeat; the checkpoint holds
                            // every stage's result, so a resumed job never
                            // fetches a captured item again.
                            let document =
                                crate::revamp::stage_document(&capture, settings, purpose)?;
                            let (document, extra) = prepare_document(
                                document,
                                &capture.captured.assets,
                                settings,
                                purpose,
                                environment,
                                crate::vocab::Providers::default(),
                            )
                            .map_err(|error| {
                                format!("{}: {error}", enrichment_code(&error))
                            })?;
                            Ok::<_, String>((document, capture.captured.assets, extra))
                        }))
                        .unwrap_or_else(|_| Err("SOURCE_CAPTURE_WORKER_PANIC".into()));
                        let _ = sender.send((item.item_id, attempt, result));
                    });
                }
                drop(sender);
                let mut stop = dispatched < wave.len();
                let mut renewed = std::time::Instant::now();
                for _ in 0..dispatched {
                    let (item_id, attempt, prepared) = loop {
                        match receiver.recv_timeout(poll.min(heartbeat_interval)) {
                            Ok(result) => break result,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                if renewed.elapsed() >= heartbeat_interval {
                                    store.renew_lease(&lease, seconds)?;
                                    renewed = std::time::Instant::now();
                                }
                                stop |= store.preparation_control(job)?.is_some_and(|receipt| receipt.event.action != linguist_store::preparation_control::ControlAction::Resume);
                            }
                            Err(_) => return Err("SOURCE_CAPTURE_WORKER_DISCONNECTED".into()),
                        }
                    };
                    store.renew_lease(&lease, seconds)?;
                    renewed = std::time::Instant::now();
                    let prepared = prepared.and_then(|(document, assets, extra)| {
                        for (digest, bytes) in assets {
                            if store.publish_asset(
                                &bytes,
                                settings.values["input.max_file_mb"].as_u64().unwrap()
                                    * 1024
                                    * 1024,
                            )? != digest
                            {
                                return Err("SOURCE_CAPTURE_ASSET_DIGEST_CONFLICT".into());
                            }
                        }
                        for (bytes, cap) in extra {
                            store.publish_asset(&bytes, cap)?;
                        }
                        Ok(document)
                    });
                    let (stage, succeeded) = match prepared {
                        Ok(document) => (
                            PreparationStage::Captured {
                                document: Box::new(document),
                            },
                            true,
                        ),
                        Err(error) => {
                            let code = match error.as_str() {
                                "ANKI_READ_TIMEOUT" => "SOURCE_READ_TIMEOUT",
                                "ANKI_DEPENDENCY_UNAVAILABLE" => "SOURCE_READ_CONNECTION_FAILED",
                                "ANKI_HTTP_FAILURE: 429" => "SOURCE_READ_RATE_LIMITED",
                                "ANKI_HTTP_FAILURE: 503" => "SOURCE_READ_UNAVAILABLE",
                                error if error.starts_with("ENRICHMENT_PROVIDER_TRANSIENT:") => {
                                    "ENRICHMENT_PROVIDER_TRANSIENT"
                                }
                                error if error.starts_with("ENRICHMENT_FAILED:") => {
                                    "ENRICHMENT_FAILED"
                                }
                                _ => "SOURCE_CAPTURE_REVIEW_REQUIRED",
                            };
                            let retry_eligible = matches!(
                                code,
                                "SOURCE_READ_TIMEOUT"
                                    | "SOURCE_READ_CONNECTION_FAILED"
                                    | "SOURCE_READ_RATE_LIMITED"
                                    | "SOURCE_READ_UNAVAILABLE"
                                    | "ENRICHMENT_PROVIDER_TRANSIENT"
                            );
                            stop |=
                                !retry_eligible || settings.values["jobs.on_item_error"] == "stop";
                            (
                                PreparationStage::Failed {
                                    code: code.into(),
                                    retry_eligible,
                                },
                                false,
                            )
                        }
                    };
                    let receipt = store.append_preparation_event_with_lease(
                        job,
                        item_id,
                        attempt,
                        stage,
                        head.as_deref(),
                        &lease,
                    )?;
                    head = Some(receipt.digest);
                    if succeeded {
                        captured += 1;
                    } else {
                        failed += 1;
                    }
                }
                // Stop prevents the next wave; already-dispatched reads retain their outcomes.
                Ok(stop)
            })?;
            if stop {
                break;
            }
        }
        store.renew_lease(&lease, seconds)?;
        let control = store.preparation_control(job)?;
        let halted = control.as_ref().is_some_and(|receipt| {
            receipt.event.action != linguist_store::preparation_control::ControlAction::Resume
        });
        // Every dispatched read has its durable result, so the stop is confirmed.
        let stop_ack = if halted {
            store.acknowledge_preparation_stop(job, &lease)?
        } else {
            None
        };
        let plan = if halted {
            None
        } else {
            match store.publish_preparation_plan(job, head.as_deref(), &lease) {
                Err(error) if error == "PREPARATION_CONTROL_BLOCKS_PUBLICATION" => None,
                other => other?,
            }
        };
        let control = store.preparation_control(job)?;
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
            serde_json::json!({"schema_version":2,"job_id":job,"stage":"source_draft","captured_this_run":captured,"failed_this_run":failed,"item_counts":counts,"error_counts":errors,"control":control,"stop_acknowledgement":stop_ack,"worker_stopped_confirmed":stop_ack.is_some(),"exit_code":exit_code,"checkpoint_digest":head,"plan_published":plan.is_some(),"plan":plan,"ready":false,"writes_enabled":false}),
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

/// A prepared document and every new asset with its publication cap.
type PreparedDocument = (linguist_core::LearningDocument, Vec<(Vec<u8>, u64)>);

/// OCR, dictionary lookup and kanji/picture/audio enrichment for one staged
/// document, in the order `vocab revamp` runs them over a whole plan. Returns
/// the enriched document and every new asset with its publication cap.
/// `providers` are injected ports; `None` uses the configured live adapter.
pub fn prepare_document(
    document: linguist_core::LearningDocument,
    captured: &BTreeMap<String, Vec<u8>>,
    settings: &linguist_config::Effective,
    purpose: &str,
    environment: &BTreeMap<String, String>,
    providers: crate::vocab::Providers<'_>,
) -> Result<PreparedDocument, String> {
    let mut documents = vec![document];
    let mut extra: Vec<(Vec<u8>, u64)> =
        crate::ocr_inspection::inspect_documents(&mut documents, captured, settings, environment)?
            .into_values()
            .map(|bytes| (bytes, 100 * 1024 * 1024))
            .collect();
    let mut document = documents.remove(0);
    if settings.values["dictionary.provider"] != "authored" && !purpose.ends_with("_grammar") {
        let cap = settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024;
        let (enriched, responses) =
            crate::dictionary::enrich_document(
                &document,
                settings,
                &Default::default(),
                providers.dictionary,
            )?;
        document = enriched;
        extra.extend(responses.into_iter().map(|bytes| (bytes, cap)));
    }
    if crate::vocab::document_requested(settings, &document) {
        let cap = settings.values["network.max_response_mb"]
            .as_u64()
            .unwrap_or(20)
            * 1024
            * 1024;
        let (enriched, bytes) =
            crate::vocab::enrich_document(&document, settings, environment, providers)?;
        document = enriched;
        extra.extend(bytes.into_iter().map(|bytes| (bytes, cap)));
    }
    Ok((document, extra))
}

/// Transient provider reads (DNS, deadline, transport, rate limits and 5xx)
/// may be retried; any other enrichment failure needs review.
fn enrichment_code(error: &str) -> &'static str {
    let transient = [
        "PROVIDER_READ_Dns",
        "PROVIDER_READ_Deadline",
        "PROVIDER_READ_Transport",
        "PROVIDER_READ_RetryAfter",
        "PROVIDER_READ_Http(408)",
        "PROVIDER_READ_Http(425)",
        "PROVIDER_READ_Http(429)",
        "PROVIDER_READ_Http(5",
    ];
    if transient.iter().any(|code| error.contains(code)) {
        "ENRICHMENT_PROVIDER_TRANSIENT"
    } else {
        "ENRICHMENT_FAILED"
    }
}

pub fn create(
    purpose: &str,
    note_ids: Vec<String>,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
) -> Result<serde_json::Value, String> {
    create_selected(
        purpose,
        crate::revamp::SourceSelector::NoteIds(note_ids),
        None,
        settings,
        environment,
    )
}

/// Freeze a one-time query/deck match set; creation never starts capture workers.
pub fn create_selected(
    purpose: &str,
    selector: crate::revamp::SourceSelector,
    limit: Option<u64>,
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
) -> Result<serde_json::Value, String> {
    use crate::revamp::SourceSelector;
    if !matches!(
        purpose,
        "japanese_vocab" | "english_vocab" | "japanese_grammar" | "english_grammar"
    ) {
        return Err("JOB_PURPOSE_UNSUPPORTED".into());
    }
    if limit.is_some_and(|limit| !(1..=100000).contains(&limit)) {
        return Err("JOB_SELECTION_LIMIT_INVALID".into());
    }
    for key in [
        "selection.max_notes",
        "selection.order",
        "jobs.max_item_attempts",
        "input.max_file_mb",
        "input.max_record_chars",
    ] {
        linguist_config::Registry::builtin()
            .validate_value(key, settings.values.get(key).ok_or("JOB_SETTING_MISSING")?)?;
    }
    let frozen = crate::freeze_settings(settings, environment)?;
    let (note_ids, selector) = match selector {
        SourceSelector::NoteIds(ids) => {
            if limit.is_some() {
                return Err("JOB_EXPLICIT_IDS_LIMIT_CONFLICT".into());
            }
            if ids.is_empty() || ids.len() > 100000 {
                return Err("JOB_INPUT_REQUIRED_OR_LIMIT".into());
            }
            let selector = SelectionInput::NoteIds(ids.clone());
            (ids, selector)
        }
        selector => {
            let (query, input) = match selector {
                SourceSelector::Query(query) => (query.clone(), SelectionInput::Query(query)),
                SourceSelector::Deck(name) => {
                    let query = linguist_anki::deck_query(&name)?;
                    (query.clone(), SelectionInput::Deck { name, query })
                }
                SourceSelector::NoteIds(_) => unreachable!(),
            };
            if query.trim().is_empty() {
                return Err("JOB_QUERY_EMPTY".into());
            }
            if query.chars().count() as u64
                > settings.values["input.max_record_chars"].as_u64().unwrap()
            {
                return Err("JOB_QUERY_LIMIT".into());
            }
            let client = linguist_anki::Client::from_settings(settings, environment)?;
            let ids = client.find_notes(&query)?;
            client.check_profile()?;
            if ids.is_empty() {
                return Ok(
                    serde_json::json!({"schema_version":2,"job_id":null,"mode":"prepare","input_count":0,"matched_count":0,"state":"empty","worker_started":false,"execution_available":false,"writes_enabled":false}),
                );
            }
            if ids.len() > 100000 {
                return Err("JOB_INPUT_REQUIRED_OR_LIMIT".into());
            }
            (ids, input)
        }
    };
    if limit.is_none()
        && note_ids.len() as u64 > settings.values["selection.max_notes"].as_u64().unwrap()
    {
        return Err("JOB_INPUT_LIMIT_EXCEEDED: choose fewer inputs, raise selection.max_notes, or use --limit with a query/deck".into());
    }
    for id in &note_ids {
        linguist_anki::wire_id(&serde_json::json!(id))?;
    }
    let mut selected = note_ids.clone();
    if settings.values["selection.order"] == "note_id" {
        selected.sort_by_key(|id| id.parse::<u64>().unwrap());
    }
    if let Some(limit) = limit {
        selected.truncate(limit as usize);
    }
    let matched_count = note_ids.len();
    let selection = SelectionReceipt {
        schema_version: 1,
        purpose: purpose.into(),
        selector,
        matched_note_ids: note_ids,
        selected_note_ids: selected.clone(),
        order: settings.values["selection.order"].as_str().unwrap().into(),
        max_notes: settings.values["selection.max_notes"].as_u64().unwrap(),
        command_limit: limit,
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
        serde_json::json!({"schema_version":2,"job_id":definition.job.id,"mode":"prepare","digest":digest,"input_count":selected.len(),"matched_count":matched_count,"state":"queued","worker_started":false,"execution_available":false,"writes_enabled":false}),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_transient_provider_reads_are_retried() {
        for error in [
            "DICTIONARY_LOOKUP_FAILED: PROVIDER_READ_Dns",
            "PROVIDER_READ_Deadline",
            "PROVIDER_READ_Http(429)",
            "PROVIDER_READ_Http(503)",
        ] {
            assert_eq!(super::enrichment_code(error), "ENRICHMENT_PROVIDER_TRANSIENT");
        }
        for error in [
            "PROVIDER_READ_Policy",
            "PROVIDER_READ_Offline",
            "PROVIDER_READ_Http(404)",
            "PROVIDER_READ_Schema",
            "AUDIO_SETTING_MISSING",
        ] {
            assert_eq!(super::enrichment_code(error), "ENRICHMENT_FAILED");
        }
    }
}

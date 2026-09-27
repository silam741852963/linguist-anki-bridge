//! Queue existing-note preparation inputs without reading Anki or starting a worker.
use linguist_core::records::*;
use std::collections::BTreeMap;

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

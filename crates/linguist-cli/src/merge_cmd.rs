//! WP-24 duplicate merge commands: `plans merge-notes`, `notes delete-merged`
//! and `notes restore-merged`, over the verified companion add-on.
use crate::{Command, NoteCommand, PlanCommand, emit, native_writes, state_root};
use linguist_application::{apply::ApplyPort, merge};
use serde_json::json;

fn now_ms() -> u64 {
    linguist_application::campaign::now_ms()
}

fn max_asset(settings: &linguist_config::Effective) -> u64 {
    settings.values["media.max_asset_mb"]
        .as_u64()
        .unwrap_or(10)
        .max(1)
        * 1024
        * 1024
}

/// Handles the merge commands; `None` for every other command.
pub(crate) fn handle(
    settings: &linguist_config::Effective,
    command: &Command,
) -> Result<Option<u8>, String> {
    match command {
        Command::Plans {
            command: PlanCommand::MergeNotes { plan, item, notes },
        } => merge_notes(settings, *plan, *item, notes).map(Some),
        Command::Notes {
            command:
                NoteCommand::DeleteMerged {
                    note_id,
                    survivor,
                    plan,
                    apply,
                },
        } => delete(settings, note_id, survivor, *plan, *apply).map(Some),
        Command::Notes {
            command: NoteCommand::RestoreMerged { note_id, apply },
        } => restore(settings, note_id, *apply).map(Some),
        _ => Ok(None),
    }
}

fn merge_notes(
    settings: &linguist_config::Effective,
    plan: uuid::Uuid,
    item: uuid::Uuid,
    notes: &[String],
) -> Result<u8, String> {
    let root = state_root(settings)?;
    let base = {
        let store = linguist_store::Store::read_only(&root)?;
        store.revision(plan, store.latest_revision(plan)?)?
    };
    let digest = base.approval_digest().map_err(|e| e.to_string())?;
    let client = crate::anki_client(settings)?;
    let mut port = native_writes::connect(&client, settings)?;
    port.refresh()?;
    let max = max_asset(settings);
    let mut merged = Vec::new();
    for note in notes {
        let id: i64 = note.parse().map_err(|_| "MERGE_NOTE_ID_INVALID")?;
        let evidence = port.note_evidence(id)?;
        let mut media = Vec::new();
        for file in evidence["media"].as_array().into_iter().flatten() {
            if file["missing"] == true {
                continue;
            }
            let Some(name) = file["name"].as_str() else {
                continue;
            };
            // Unreadable media stays referenced by name in the archived fields.
            if let Ok(Some(bytes)) = port.media_bytes(name, max) {
                media.push((name.to_owned(), bytes));
            }
        }
        merged.push(merge::MergedNote { evidence, media });
    }
    let mut store = linguist_store::Store::open_existing(&root)?;
    let child = merge::add_merged_notes(
        &mut store,
        &base,
        &digest,
        item,
        &merged,
        max,
        now_ms() / 1000,
    )?;
    emit(
        &json!({"schema_version": 2, "plan_id": plan, "revision": child.revision,
        "digest": child.approval_digest().map_err(|e| e.to_string())?, "item": item,
        "merged": notes, "writes_enabled": false}),
    )?;
    Ok(4)
}

fn delete(
    settings: &linguist_config::Effective,
    note_id: &str,
    survivor: &str,
    plan: uuid::Uuid,
    apply: bool,
) -> Result<u8, String> {
    let root = state_root(settings)?;
    let base = {
        let store = linguist_store::Store::read_only(&root)?;
        store.revision(plan, store.latest_revision(plan)?)?
    };
    if !merge::merged_in(&base).iter().any(|(_, n)| n == note_id) {
        return Err("MERGE_NOT_RECORDED: run `plans merge-notes` first".into());
    }
    if !apply {
        let record = linguist_store::Store::read_only(&root)?.merge_deletion(note_id)?;
        emit(
            &json!({"note_id": note_id, "survivor": survivor, "plan": plan, "record": record,
            "writes_enabled": false, "next_command": format!(
                "linguist-anki-bridge notes delete-merged {note_id} --survivor {survivor} --plan {plan} --apply")}),
        )?;
        return Ok(0);
    }
    let client = crate::anki_client(settings)?;
    let mut port = native_writes::connect(&client, settings)?;
    port.refresh()?;
    let id: i64 = note_id.parse().map_err(|_| "MERGE_NOTE_ID_INVALID")?;
    let evidence = match port.note_evidence(id) {
        Ok(evidence) => Some(evidence),
        Err(e) if e.contains("NOTE_NOT_FOUND") => None,
        Err(e) => return Err(e),
    };
    let mut store = linguist_store::Store::open_existing(&root)?;
    let result = merge::delete_merged(
        &mut store,
        &mut port as &mut dyn ApplyPort,
        &base,
        note_id,
        survivor,
        evidence.as_ref(),
        now_ms(),
    )?;
    emit(&result)?;
    Ok(0)
}

fn restore(
    settings: &linguist_config::Effective,
    note_id: &str,
    apply: bool,
) -> Result<u8, String> {
    let root = state_root(settings)?;
    if !apply {
        let record = linguist_store::Store::read_only(&root)?
            .merge_deletion(note_id)?
            .ok_or("MERGE_DELETION_NOT_FOUND")?;
        emit(&json!({"record": record, "writes_enabled": false,
            "next_command": format!("linguist-anki-bridge notes restore-merged {note_id} --apply")}))?;
        return Ok(0);
    }
    let client = crate::anki_client(settings)?;
    let mut port = native_writes::connect(&client, settings)?;
    port.refresh()?;
    let mut store = linguist_store::Store::open_existing(&root)?;
    let result = merge::restore_merged(
        &mut store,
        &mut port as &mut dyn ApplyPort,
        note_id,
        64 * 1024 * 1024,
        now_ms(),
    )?;
    emit(&result)?;
    Ok(0)
}

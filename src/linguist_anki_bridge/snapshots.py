"""Persistent, word-oriented snapshots for reversible Anki writes."""

from __future__ import annotations

import datetime as dt
import json
import threading
import uuid
from pathlib import Path
from typing import Any


DEFAULT_SNAPSHOT_PATH = Path.home() / ".config" / "linguist-anki-bridge" / "card_snapshots.json"


def _compact(value: Any, key: str = "") -> Any:
    """Keep diagnostic structure while excluding duplicated media payloads."""
    if isinstance(value, dict):
        return {str(k): _compact(v, str(k)) for k, v in value.items()}
    if isinstance(value, list):
        return [_compact(item, key) for item in value]
    if isinstance(value, str) and ("b64" in key.lower() or value.startswith("data:image")):
        return f"<media payload: {len(value)} chars>"
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    return str(value)


def _field_values(note: dict | None) -> dict[str, str]:
    values: dict[str, str] = {}
    for name, field in (note or {}).get("fields", {}).items():
        values[str(name)] = str(field.get("value", "") if isinstance(field, dict) else field)
    return values


class SnapshotManager:
    """Store snapshots atomically and restore them through AnkiConnect."""

    def __init__(self, path: Path | str = DEFAULT_SNAPSHOT_PATH):
        self.path = Path(path)
        self._lock = threading.RLock()

    def _load(self) -> list[dict]:
        if not self.path.exists():
            return []
        try:
            value = json.loads(self.path.read_text(encoding="utf-8"))
            return value if isinstance(value, list) else []
        except (OSError, ValueError):
            return []

    def _save(self, snapshots: list[dict]) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self.path.with_suffix(self.path.suffix + ".tmp")
        temporary.write_text(json.dumps(snapshots, ensure_ascii=False, indent=2), encoding="utf-8")
        temporary.replace(self.path)

    def create(
        self,
        *,
        word: str,
        mode: str,
        deck_key: str,
        note: dict | None,
        processed: dict,
        dry_run: bool,
        media_before: dict[str, str | None],
    ) -> str:
        now = dt.datetime.now(dt.timezone.utc).astimezone().isoformat(timespec="seconds")
        snapshot_id = f"{dt.datetime.now().strftime('%Y%m%d-%H%M%S')}-{uuid.uuid4().hex[:8]}"
        record = {
            "id": snapshot_id,
            "created_at": now,
            "word": str(word),
            "mode": str(mode),
            "deck_key": str(deck_key),
            "dry_run": bool(dry_run),
            "status": "dry-run" if dry_run else "captured",
            "original_note": {
                "note_id": (note or {}).get("noteId"),
                "model_name": (note or {}).get("modelName", ""),
                "deck_name": (note or {}).get("deckName", ""),
                "tags": list((note or {}).get("tags") or []),
                "fields": _field_values(note),
            } if note else None,
            "processed": _compact(processed),
            "media_before": dict(media_before),
            "result_note_id": None,
            "error": "",
        }
        with self._lock:
            snapshots = self._load()
            snapshots.append(record)
            self._save(snapshots)
        return snapshot_id

    def finalize(self, snapshot_id: str, *, result_note_id: int | None = None, error: str = "") -> None:
        with self._lock:
            snapshots = self._load()
            for record in snapshots:
                if record.get("id") == snapshot_id:
                    record["result_note_id"] = result_note_id
                    record["error"] = str(error or "")
                    if error:
                        record["status"] = "failed"
                    elif not record.get("dry_run"):
                        record["status"] = "committed"
                    break
            self._save(snapshots)

    def list_for_word(self, word: str) -> list[dict]:
        with self._lock:
            matches = [item for item in self._load() if item.get("word") == str(word)]
        return sorted(matches, key=lambda item: item.get("created_at", ""), reverse=True)

    def get(self, snapshot_id: str) -> dict | None:
        with self._lock:
            return next((item for item in self._load() if item.get("id") == snapshot_id), None)

    def revert(self, snapshot_id: str, anki_client) -> str:
        with self._lock:
            snapshots = self._load()
            record = next((item for item in snapshots if item.get("id") == snapshot_id), None)
            if not record:
                raise KeyError(f"Unknown snapshot: {snapshot_id}")
            if record.get("dry_run"):
                return "Dry-run snapshot contains no Anki changes to revert."
            if record.get("status") == "reverted":
                return "Snapshot was already reverted."

            original = record.get("original_note")
            if not original and record.get("result_note_id"):
                # Remove the reference before deleting media created for it.
                anki_client.delete_notes([int(record["result_note_id"])])

            for filename, previous in (record.get("media_before") or {}).items():
                if previous:
                    anki_client.store_media_file(filename, previous)
                else:
                    anki_client.delete_media_file(filename)

            if original and original.get("note_id"):
                note_id = int(original["note_id"])
                model_name = str(original.get("model_name") or "")
                if model_name:
                    current = anki_client.get_notes_info([note_id])
                    current_model = str(current[0].get("modelName") or "") if current else ""
                else:
                    current_model = ""
                if model_name and current_model and current_model != model_name:
                    if not anki_client.supports_action("updateNoteModel"):
                        raise RuntimeError(
                            "Restoring the original note type requires a current AnkiConnect release."
                        )
                    anki_client.update_note_model(
                        note_id, model_name, original.get("fields") or {},
                        original.get("tags") or [],
                    )
                else:
                    anki_client.update_note_fields(note_id, original.get("fields") or {})
                message = f"Restored note {original['note_id']} and its tracked media."
            elif record.get("result_note_id"):
                message = f"Deleted injected note {record['result_note_id']} and restored its tracked media."
            else:
                raise RuntimeError("This snapshot has no committed note to revert.")

            record["status"] = "reverted"
            record["reverted_at"] = dt.datetime.now(dt.timezone.utc).astimezone().isoformat(timespec="seconds")
            self._save(snapshots)
            return message

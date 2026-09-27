"""Durable, resumable modernization jobs.

SQLite is the source of truth for orchestration state.  Large processing
results (notably base64 media) live in atomically replaced JSON artifacts so
the database stays quick to inspect and recover.
"""

from __future__ import annotations

import asyncio
import datetime as dt
import json
import os
import shutil
import sqlite3
import threading
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


DEFAULT_BATCH_DB = Path.home() / ".config" / "linguist-anki-bridge" / "batch_jobs.sqlite3"
TERMINAL_ITEM_STATES = {"completed", "reverted", "skipped"}


def _now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="microseconds")


@dataclass(frozen=True)
class BatchItemSeed:
    note_id: int
    word: str


class ServiceRateLimiter:
    """Async start-rate limiter shared by every card in a batch.

    Limits are expressed as minimum seconds between starts.  A monotonic clock
    makes wall-clock changes harmless, while one lock per service lets unrelated
    services progress independently.
    """

    def __init__(self, intervals: dict[str, float] | None = None):
        self.intervals = {str(k): max(0.0, float(v)) for k, v in (intervals or {}).items()}
        self._locks: dict[str, asyncio.Lock] = {}
        self._next: dict[str, float] = {}

    async def wait(self, service: str) -> None:
        interval = self.intervals.get(service, 0.0)
        if interval <= 0:
            return
        lock = self._locks.setdefault(service, asyncio.Lock())
        async with lock:
            loop = asyncio.get_running_loop()
            delay = self._next.get(service, 0.0) - loop.time()
            if delay > 0:
                await asyncio.sleep(delay)
            self._next[service] = loop.time() + interval


class BatchJobStore:
    """Transactional persistence and recovery for batch modernization."""

    def __init__(self, path: Path | str = DEFAULT_BATCH_DB):
        self.path = Path(path)
        self.artifact_root = self.path.with_suffix("").with_name(self.path.stem + "_artifacts")
        self._lock = threading.RLock()
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.artifact_root.mkdir(parents=True, exist_ok=True)
        self._runner_lock_file = None
        self.is_primary_runner = self._acquire_runner_lease()
        self._initialize()
        if self.is_primary_runner:
            self.recover_interrupted()

    def _acquire_runner_lease(self) -> bool:
        """Hold a kernel-released lease so two app processes cannot run jobs."""
        lock_path = self.path.with_suffix(self.path.suffix + ".runner.lock")
        handle = open(lock_path, "a+b")
        try:
            import fcntl
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            handle.seek(0)
            handle.truncate()
            handle.write(str(os.getpid()).encode("ascii"))
            handle.flush()
            self._runner_lock_file = handle
            return True
        except ImportError:
            # Non-POSIX fallback: retain the handle and rely on the application's
            # existing single-process runner guard.
            self._runner_lock_file = handle
            return True
        except (BlockingIOError, OSError):
            handle.close()
            return False

    def close(self) -> None:
        handle = self._runner_lock_file
        self._runner_lock_file = None
        if handle is None:
            return
        try:
            try:
                import fcntl
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
            except ImportError:
                pass
        finally:
            handle.close()

    def _connect(self) -> sqlite3.Connection:
        db = sqlite3.connect(self.path, timeout=30)
        db.row_factory = sqlite3.Row
        db.execute("PRAGMA foreign_keys=ON")
        db.execute("PRAGMA journal_mode=WAL")
        db.execute("PRAGMA synchronous=FULL")
        return db

    def _initialize(self) -> None:
        with self._lock, self._connect() as db:
            db.executescript(
                """
                CREATE TABLE IF NOT EXISTS batch_jobs (
                    id TEXT PRIMARY KEY,
                    deck_key TEXT NOT NULL,
                    deck_name TEXT NOT NULL,
                    status TEXT NOT NULL,
                    dry_run INTEGER NOT NULL DEFAULT 0,
                    settings_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    started_at TEXT,
                    finished_at TEXT,
                    last_error TEXT NOT NULL DEFAULT ''
                );
                CREATE TABLE IF NOT EXISTS batch_items (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    job_id TEXT NOT NULL REFERENCES batch_jobs(id) ON DELETE CASCADE,
                    ordinal INTEGER NOT NULL,
                    note_id INTEGER NOT NULL,
                    word TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    attempts INTEGER NOT NULL DEFAULT 0,
                    next_attempt_at TEXT,
                    artifact_path TEXT,
                    snapshot_id TEXT,
                    result_note_id INTEGER,
                    last_error TEXT NOT NULL DEFAULT '',
                    started_at TEXT,
                    finished_at TEXT,
                    updated_at TEXT NOT NULL,
                    UNIQUE(job_id, note_id)
                );
                CREATE INDEX IF NOT EXISTS idx_batch_items_work
                    ON batch_items(job_id, status, ordinal);
                CREATE INDEX IF NOT EXISTS idx_batch_items_note
                    ON batch_items(note_id, status);
                """
            )

    def create_job(
        self, *, deck_key: str, deck_name: str, items: Iterable[BatchItemSeed | dict],
        dry_run: bool, settings: dict[str, Any] | None = None,
    ) -> str:
        seeds = [
            item if isinstance(item, BatchItemSeed) else BatchItemSeed(int(item["note_id"]), str(item.get("word", "")))
            for item in items
        ]
        if not seeds:
            raise ValueError("A batch job requires at least one card.")
        job_id = f"batch-{dt.datetime.now().strftime('%Y%m%d-%H%M%S')}-{uuid.uuid4().hex[:8]}"
        now = _now()
        with self._lock, self._connect() as db:
            db.execute(
                "INSERT INTO batch_jobs VALUES (?, ?, ?, 'queued', ?, ?, ?, ?, NULL, NULL, '')",
                (job_id, deck_key, deck_name, int(dry_run), json.dumps(settings or {}), now, now),
            )
            db.executemany(
                """INSERT OR IGNORE INTO batch_items
                   (job_id, ordinal, note_id, word, status, updated_at)
                   VALUES (?, ?, ?, ?, 'pending', ?)""",
                [(job_id, index, seed.note_id, seed.word, now) for index, seed in enumerate(seeds)],
            )
        return job_id

    def recover_interrupted(self) -> None:
        """Pause jobs left running and return in-flight work to safe checkpoints."""
        now = _now()
        with self._lock, self._connect() as db:
            db.execute(
                "UPDATE batch_jobs SET status='paused', updated_at=?, last_error=? WHERE status IN ('running','pausing')",
                (now, "Application stopped while this job was running; resume is safe."),
            )
            db.execute(
                "UPDATE batch_items SET status='pending', updated_at=? WHERE status='processing'",
                (now,),
            )
            # A commit may already have reached Anki. Replaying the same managed
            # field/media update is idempotent and reuses the original snapshot.
            db.execute(
                "UPDATE batch_items SET status='processed', updated_at=? WHERE status='committing'",
                (now,),
            )
            db.execute(
                "UPDATE batch_jobs SET status='rollback_paused', updated_at=? WHERE status='rolling_back'",
                (now,),
            )

    @staticmethod
    def _row(row: sqlite3.Row | None) -> dict | None:
        if row is None:
            return None
        value = dict(row)
        if "settings_json" in value:
            try:
                value["settings"] = json.loads(value.pop("settings_json") or "{}")
            except ValueError:
                value["settings"] = {}
        if "dry_run" in value:
            value["dry_run"] = bool(value["dry_run"])
        return value

    def list_jobs(self) -> list[dict]:
        with self._lock, self._connect() as db:
            rows = db.execute(
                """SELECT j.*,
                    COUNT(i.id) AS total,
                    SUM(CASE WHEN i.status='completed' THEN 1 ELSE 0 END) AS completed,
                    SUM(CASE WHEN i.status='failed' THEN 1 ELSE 0 END) AS failed,
                    SUM(CASE WHEN i.status='reverted' THEN 1 ELSE 0 END) AS reverted
                   FROM batch_jobs j LEFT JOIN batch_items i ON i.job_id=j.id
                   GROUP BY j.id ORDER BY j.created_at DESC"""
            ).fetchall()
        return [self._row(row) for row in rows]

    def get_job(self, job_id: str) -> dict | None:
        with self._lock, self._connect() as db:
            return self._row(db.execute("SELECT * FROM batch_jobs WHERE id=?", (job_id,)).fetchone())

    def list_items(self, job_id: str, *, limit: int | None = None, offset: int = 0) -> list[dict]:
        """Return a stable page of job items.

        ``limit=None`` retains the original all-items API for the runner and
        tests.  The management UI uses bounded pages so a 50,000-note job does
        not freeze Textual while constructing rows.
        """
        sql = "SELECT * FROM batch_items WHERE job_id=? ORDER BY ordinal"
        params: tuple[Any, ...] = (job_id,)
        if limit is not None:
            sql += " LIMIT ? OFFSET ?"
            params = (job_id, max(1, int(limit)), max(0, int(offset)))
        with self._lock, self._connect() as db:
            rows = db.execute(sql, params).fetchall()
        return [self._row(row) for row in rows]

    def item_counts(self, job_id: str) -> dict[str, int]:
        with self._lock, self._connect() as db:
            rows = db.execute(
                "SELECT status, COUNT(*) AS count FROM batch_items WHERE job_id=? GROUP BY status",
                (job_id,),
            ).fetchall()
        return {str(row["status"]): int(row["count"]) for row in rows}

    def get_item(self, item_id: int) -> dict | None:
        with self._lock, self._connect() as db:
            return self._row(db.execute("SELECT * FROM batch_items WHERE id=?", (item_id,)).fetchone())

    def set_job_status(self, job_id: str, status: str, error: str = "") -> None:
        now = _now()
        started = now if status == "running" else None
        finished = now if status in {"completed", "failed", "cancelled", "rolled_back", "rollback_partial"} else None
        with self._lock, self._connect() as db:
            db.execute(
                """UPDATE batch_jobs SET status=?, updated_at=?, last_error=?,
                   started_at=COALESCE(started_at, ?), finished_at=COALESCE(?, finished_at)
                   WHERE id=?""",
                (status, now, error, started, finished, job_id),
            )
            if status == "running":
                db.execute("UPDATE batch_jobs SET finished_at=NULL WHERE id=?", (job_id,))

    def claim_next(self, job_id: str) -> dict | None:
        now = _now()
        with self._lock, self._connect() as db:
            db.execute("BEGIN IMMEDIATE")
            row = db.execute(
                """SELECT * FROM batch_items WHERE job_id=? AND
                   (status IN ('processed','pending') AND (next_attempt_at IS NULL OR next_attempt_at<=?))
                   ORDER BY CASE status WHEN 'processed' THEN 0 ELSE 1 END, ordinal LIMIT 1""",
                (job_id, now),
            ).fetchone()
            if not row:
                db.commit()
                return None
            new_status = "committing" if row["status"] == "processed" else "processing"
            db.execute(
                "UPDATE batch_items SET status=?, started_at=COALESCE(started_at, ?), updated_at=? WHERE id=?",
                (new_status, now, now, row["id"]),
            )
            db.commit()
        result = dict(row)
        result["status"] = new_status
        return result

    def set_item(self, item_id: int, **values: Any) -> None:
        allowed = {
            "status", "attempts", "next_attempt_at", "artifact_path", "snapshot_id",
            "result_note_id", "last_error", "finished_at", "started_at",
        }
        values = {key: value for key, value in values.items() if key in allowed}
        values["updated_at"] = _now()
        columns = ", ".join(f"{key}=?" for key in values)
        with self._lock, self._connect() as db:
            db.execute(f"UPDATE batch_items SET {columns} WHERE id=?", (*values.values(), item_id))
            db.execute(
                "UPDATE batch_jobs SET updated_at=? WHERE id=(SELECT job_id FROM batch_items WHERE id=?)",
                (values["updated_at"], item_id),
            )

    def delete_job(self, job_id: str) -> dict[str, int]:
        """Delete orchestration state and artifacts without touching Anki.

        Snapshot records intentionally remain in the snapshot manager, but
        deleting the job removes the convenient mass-rollback association.
        The UI therefore requires an explicit warning confirmation first.
        """
        job = self.get_job(job_id)
        if not job:
            raise KeyError(f"Batch job not found: {job_id}")
        if job.get("status") in {"running", "pausing", "rolling_back"}:
            raise RuntimeError("Pause or cancel the active batch before deleting it.")
        counts = self.item_counts(job_id)
        if counts.get("processing", 0) or counts.get("committing", 0):
            raise RuntimeError("The current card is still reaching a safe boundary; try delete again shortly.")
        with self._lock, self._connect() as db:
            db.execute("DELETE FROM batch_jobs WHERE id=?", (job_id,))
        directory = self.artifact_root / job_id
        if directory.parent == self.artifact_root and directory.exists():
            shutil.rmtree(directory)
        return {"items": sum(counts.values()), "committed": counts.get("completed", 0)}

    def save_artifact(self, job_id: str, item_id: int, data: dict) -> str:
        directory = self.artifact_root / job_id
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / f"{item_id}.json"
        temporary = path.with_suffix(".json.tmp")
        temporary.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
        temporary.replace(path)
        self.set_item(item_id, artifact_path=str(path), status="processed", last_error="")
        return str(path)

    def load_artifact(self, item: dict) -> dict | None:
        path = item.get("artifact_path")
        if not path:
            return None
        try:
            value = json.loads(Path(path).read_text(encoding="utf-8"))
            return value if isinstance(value, dict) else None
        except (OSError, ValueError):
            return None

    def retry_or_fail(self, item: dict, error: str, max_attempts: int, backoff: float) -> bool:
        attempts = int(item.get("attempts", 0)) + 1
        retry = attempts < max(1, max_attempts)
        next_at = None
        if retry:
            next_at = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(seconds=backoff * (2 ** (attempts - 1)))).isoformat(timespec="seconds")
        resume_status = "processed" if item.get("artifact_path") else "pending"
        self.set_item(
            int(item["id"]), status=resume_status if retry else "failed", attempts=attempts,
            next_attempt_at=next_at, last_error=str(error), finished_at=None if retry else _now(),
        )
        return retry

    def retry_failed(self, job_id: str) -> int:
        now = _now()
        with self._lock, self._connect() as db:
            cursor = db.execute(
                """UPDATE batch_items SET status='pending', attempts=0, next_attempt_at=NULL,
                   last_error='', finished_at=NULL, updated_at=? WHERE job_id=? AND status='failed'""",
                (now, job_id),
            )
        self.set_job_status(job_id, "paused")
        return cursor.rowcount

    def cancel(self, job_id: str) -> None:
        now = _now()
        with self._lock, self._connect() as db:
            db.execute("UPDATE batch_jobs SET status='cancelled', updated_at=?, finished_at=? WHERE id=?", (now, now, job_id))
            db.execute(
                "UPDATE batch_items SET status='skipped', finished_at=?, updated_at=? WHERE job_id=? AND status='pending'",
                (now, now, job_id),
            )

    def remaining(self, job_id: str) -> int:
        with self._lock, self._connect() as db:
            return int(db.execute(
                "SELECT COUNT(*) FROM batch_items WHERE job_id=? AND status NOT IN ('completed','reverted','skipped','failed')",
                (job_id,),
            ).fetchone()[0])

    def later_change_exists(self, job_id: str, note_id: int) -> bool:
        with self._lock, self._connect() as db:
            return bool(db.execute(
                """SELECT 1 FROM batch_items newer JOIN batch_jobs nj ON nj.id=newer.job_id
                   JOIN batch_jobs current ON current.id=?
                   WHERE newer.note_id=? AND nj.created_at>current.created_at
                   AND newer.status='completed' LIMIT 1""",
                (job_id, note_id),
            ).fetchone())

import asyncio
import sqlite3

from linguist_anki_bridge.batch_jobs import BatchItemSeed, BatchJobStore, ServiceRateLimiter


def make_store(tmp_path):
    return BatchJobStore(tmp_path / "jobs.sqlite3")


def test_job_checkpoint_and_artifact_are_resumable(tmp_path):
    store = make_store(tmp_path)
    job_id = store.create_job(
        deck_key="japanese_vocab", deck_name="Japanese", dry_run=False,
        items=[BatchItemSeed(11, "俳優")], settings={"max_attempts": 3},
    )
    item = store.claim_next(job_id)
    assert item["status"] == "processing"
    path = store.save_artifact(job_id, item["id"], {"word": "俳優", "audio_b64": "abc"})
    committing = store.claim_next(job_id)
    assert committing["status"] == "committing"
    assert store.load_artifact({"artifact_path": path})["word"] == "俳優"


def test_restart_pauses_job_and_rewinds_only_to_safe_checkpoint(tmp_path):
    path = tmp_path / "jobs.sqlite3"
    store = BatchJobStore(path)
    job_id = store.create_job(
        deck_key="japanese_vocab", deck_name="Japanese", dry_run=False,
        items=[BatchItemSeed(1, "一"), BatchItemSeed(2, "二")],
    )
    store.set_job_status(job_id, "running")
    first = store.claim_next(job_id)
    store.save_artifact(job_id, first["id"], {"word": "一"})
    store.claim_next(job_id)  # durable artifact is now in the committing checkpoint
    second = store.list_items(job_id)[1]
    store.set_item(second["id"], status="processing")

    store.close()  # Simulate process termination; the kernel lease is released.
    recovered = BatchJobStore(path)
    assert recovered.get_job(job_id)["status"] == "paused"
    statuses = [item["status"] for item in recovered.list_items(job_id)]
    assert statuses == ["processed", "pending"]


def test_commit_retry_reuses_expensive_processing_artifact(tmp_path):
    store = make_store(tmp_path)
    job_id = store.create_job(
        deck_key="japanese_vocab", deck_name="Japanese", dry_run=False,
        items=[BatchItemSeed(7, "七")],
    )
    item = store.claim_next(job_id)
    store.save_artifact(job_id, item["id"], {"word": "七"})
    committing = store.claim_next(job_id)
    assert store.retry_or_fail(committing, "temporary Anki error", 3, 0) is True
    assert store.get_item(item["id"])["status"] == "processed"


def test_newer_completed_batch_blocks_old_rollback(tmp_path):
    store = make_store(tmp_path)
    old = store.create_job(
        deck_key="japanese_vocab", deck_name="Japanese", dry_run=False,
        items=[BatchItemSeed(9, "九")],
    )
    new = store.create_job(
        deck_key="japanese_vocab", deck_name="Japanese", dry_run=False,
        items=[BatchItemSeed(9, "九")],
    )
    with sqlite3.connect(store.path) as db:
        db.execute("UPDATE batch_jobs SET created_at='2026-01-01T00:00:00+00:00' WHERE id=?", (old,))
        db.execute("UPDATE batch_jobs SET created_at='2026-01-02T00:00:00+00:00' WHERE id=?", (new,))
        db.execute("UPDATE batch_items SET status='completed' WHERE job_id=?", (new,))
    assert store.later_change_exists(old, 9) is True
    assert store.later_change_exists(new, 9) is False


def test_service_rate_limiter_spaces_same_service_starts():
    async def run():
        limiter = ServiceRateLimiter({"dictionary": 0.02})
        loop = asyncio.get_running_loop()
        starts = []
        for _ in range(2):
            await limiter.wait("dictionary")
            starts.append(loop.time())
        return starts

    starts = asyncio.run(run())
    assert starts[1] - starts[0] >= 0.018


def test_only_one_process_store_owns_runner_lease(tmp_path):
    path = tmp_path / "jobs.sqlite3"
    first = BatchJobStore(path)
    second = BatchJobStore(path)
    assert first.is_primary_runner is True
    assert second.is_primary_runner is False
    first.close()
    third = BatchJobStore(path)
    assert third.is_primary_runner is True
    third.close()


def test_items_can_be_paged_without_changing_default_all_items_api(tmp_path):
    store = make_store(tmp_path)
    job_id = store.create_job(
        deck_key="japanese_vocab", deck_name="Japanese", dry_run=True,
        items=[BatchItemSeed(index, str(index)) for index in range(7)],
    )
    assert len(store.list_items(job_id)) == 7
    assert [item["note_id"] for item in store.list_items(job_id, limit=3, offset=3)] == [3, 4, 5]


def test_item_update_touches_job_revision_and_delete_never_mutates_anki(tmp_path):
    store = make_store(tmp_path)
    job_id = store.create_job(
        deck_key="japanese_vocab", deck_name="Japanese", dry_run=False,
        items=[BatchItemSeed(11, "十一")],
    )
    before = store.get_job(job_id)["updated_at"]
    item = store.list_items(job_id)[0]
    store.set_item(item["id"], status="completed")
    assert store.get_job(job_id)["updated_at"] > before
    result = store.delete_job(job_id)
    assert result == {"items": 1, "committed": 1}
    assert store.get_job(job_id) is None
    assert store.list_items(job_id) == []


def test_delete_rejects_in_flight_job(tmp_path):
    store = make_store(tmp_path)
    job_id = store.create_job(
        deck_key="japanese_vocab", deck_name="Japanese", dry_run=False,
        items=[BatchItemSeed(12, "十二")],
    )
    store.claim_next(job_id)
    try:
        store.delete_job(job_id)
    except RuntimeError as exc:
        assert "safe boundary" in str(exc)
    else:
        raise AssertionError("in-flight job deletion should have been rejected")

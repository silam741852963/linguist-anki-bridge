# SPDX-License-Identifier: GPL-3.0-or-later
from pathlib import Path
import fcntl
import multiprocessing
import os
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge.lineage import LineageError, LineageStore


def initialize_lineage(root, results):
    try:
        store = LineageStore(root)
        results.put((True, store.lineage('a' * 64, initialize=True)))
        store.close()
    except Exception as error:
        results.put((False, str(error)))


class LineageTest(unittest.TestCase):
    def test_concurrent_initializers_share_one_durable_lineage(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'private'
            context = multiprocessing.get_context('spawn')
            results = context.Queue()
            processes = [context.Process(target=initialize_lineage, args=(root, results)) for _ in range(4)]
            try:
                for process in processes:
                    process.start()
                values = [results.get(timeout=10) for _ in processes]
                for process in processes:
                    process.join(timeout=10)
                    self.assertEqual(process.exitcode, 0)
                self.assertTrue(all(success for success, _ in values), values)
                self.assertEqual(len({value for _, value in values}), 1)
                store = LineageStore(root)
                self.assertEqual(store.lineage('a' * 64), values[0][1])
                store.close()
            finally:
                for process in processes:
                    if process.is_alive():
                        process.terminate()
                        process.join(timeout=10)
                results.close()
                results.join_thread()

    def test_startup_lock_timeout_preserves_existing_metadata(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'private'
            store = LineageStore(root)
            lineage = store.lineage('a' * 64, initialize=True)
            store.close()
            lock = os.open(root / '.native-sidecar-init.lock', os.O_RDWR)
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                with self.assertRaisesRegex(LineageError, 'BRIDGE_SIDECAR_LOCK_TIMEOUT'):
                    LineageStore(root, busy_timeout_ms=20)
            finally:
                os.close(lock)
            store = LineageStore(root)
            self.assertEqual(store.lineage('a' * 64), lineage)
            store.close()

    def test_explicit_lineage_creation_survives_reopen_and_distinguishes_paths(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'private'
            store = LineageStore(root)
            with self.assertRaisesRegex(LineageError, 'BRIDGE_LINEAGE_MISSING'):
                store.lineage('a' * 64)
            first = store.lineage('a' * 64, initialize=True)
            second = store.lineage('b' * 64, initialize=True)
            self.assertNotEqual(first, second)
            self.assertEqual(store._db.execute('PRAGMA synchronous').fetchone()[0], 2)
            self.assertEqual(store._db.execute('PRAGMA journal_mode').fetchone()[0], 'wal')
            store.close()
            reopened = LineageStore(root)
            self.assertEqual(reopened.lineage('a' * 64), first)
            self.assertEqual(reopened.lineage('b' * 64), second)
            reopened.close()

    def test_damaged_lineage_future_schema_and_symlink_sidecar_fail_closed(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'private'
            store = LineageStore(root)
            store.lineage('a' * 64, initialize=True)
            store._db.execute('UPDATE lineages SET lineage_id=?', ('damaged',))
            with self.assertRaisesRegex(LineageError, 'BRIDGE_LINEAGE_CORRUPT'):
                store.lineage('a' * 64, initialize=True)
            self.assertEqual(store._db.execute('SELECT lineage_id FROM lineages').fetchone()[0], 'damaged')
            store._db.execute('PRAGMA user_version=99')
            store.close()
            with self.assertRaisesRegex(LineageError, 'BRIDGE_SIDECAR_SCHEMA_UNSUPPORTED'):
                LineageStore(root)
            database = root / 'native-sidecar.sqlite3'
            database.unlink()
            external = Path(parent) / 'external'
            external.write_bytes(b'untouched')
            database.symlink_to(external)
            with self.assertRaisesRegex(LineageError, 'BRIDGE_SIDECAR_FILE_INVALID'):
                LineageStore(root)
            self.assertEqual(external.read_bytes(), b'untouched')

    def test_blank_existing_database_and_changed_installation_are_not_adopted(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'private'
            store = LineageStore(root)
            store._db.execute('UPDATE installation SET bridge_id=?', ('different',))
            store.close()
            with self.assertRaisesRegex(LineageError, 'BRIDGE_SIDECAR_INSTALLATION_CONFLICT'):
                LineageStore(root)
            database = root / 'native-sidecar.sqlite3'
            database.write_bytes(b'')
            with self.assertRaisesRegex(LineageError, 'BRIDGE_SIDECAR_SCHEMA_UNSUPPORTED'):
                LineageStore(root)

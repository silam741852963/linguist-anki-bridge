# SPDX-License-Identifier: GPL-3.0-or-later
from pathlib import Path
import sys
import tempfile
import threading
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge.session import SessionError, SessionTracker

LINEAGE = 'c17625b0-7a88-4aab-a8a5-c1d993c72a00'


class SessionTest(unittest.TestCase):
    def test_repeated_loads_create_epochs_but_ordinary_file_writes_do_not(self):
        with tempfile.TemporaryDirectory() as parent:
            path = Path(parent) / 'collection.anki2'
            path.write_bytes(b'fixture')
            handle = object()
            tracker = SessionTracker()
            first = tracker.opened(profile='Fixture', collection_path=path,
                                   lineage_id=LINEAGE, collection_handle=handle)
            path.write_bytes(b'ordinary scheduler change')
            observed = tracker.observed(profile='Fixture', collection_path=path, collection_handle=handle)
            self.assertEqual(first, observed)
            observed['session_epoch'] = 'forged'
            self.assertEqual(first, tracker.observed(profile='Fixture', collection_path=path, collection_handle=handle))
            second = tracker.opened(profile='Fixture', collection_path=path,
                                    lineage_id=LINEAGE, collection_handle=handle)
            self.assertNotEqual(first['session_epoch'], second['session_epoch'])
            self.assertEqual(first['path_fingerprint'], second['path_fingerprint'])
            self.assertEqual(first['lineage_id'], second['lineage_id'])
            self.assertNotIn(str(path), str(second))

    def test_profile_backend_and_file_replacement_invalidate_session(self):
        for change in ['profile', 'handle', 'file']:
            with self.subTest(change=change), tempfile.TemporaryDirectory() as parent:
                path = Path(parent) / 'collection.anki2'
                path.write_bytes(b'fixture')
                handle = object()
                tracker = SessionTracker()
                tracker.opened(profile='Fixture', collection_path=path, lineage_id=LINEAGE, collection_handle=handle)
                if change == 'file':
                    replacement = Path(parent) / 'replacement'
                    replacement.write_bytes(b'imported')
                    replacement.replace(path)
                with self.assertRaises(SessionError):
                    tracker.observed(profile='Other' if change == 'profile' else 'Fixture',
                                     collection_path=path, collection_handle=object() if change == 'handle' else handle)
                with self.assertRaisesRegex(SessionError, 'BRIDGE_SESSION_UNAVAILABLE'):
                    tracker.observed(profile='Fixture', collection_path=path, collection_handle=handle)

    def test_failed_open_clears_old_session_and_other_threads_cannot_access_it(self):
        with tempfile.TemporaryDirectory() as parent:
            path = Path(parent) / 'collection.anki2'
            path.write_bytes(b'fixture')
            handle = object()
            tracker = SessionTracker()
            tracker.opened(profile='Fixture', collection_path=path, lineage_id=LINEAGE, collection_handle=handle)
            errors = []
            def wrong_thread():
                try:
                    tracker.invalidate()
                except SessionError as error:
                    errors.append(str(error))
            worker = threading.Thread(target=wrong_thread)
            worker.start()
            worker.join(timeout=5)
            self.assertEqual(errors, ['BRIDGE_SESSION_THREAD_INVALID'])
            self.assertEqual(tracker.observed(profile='Fixture', collection_path=path, collection_handle=handle)['lineage_id'], LINEAGE)
            with self.assertRaises(SessionError):
                tracker.opened(profile='Fixture', collection_path=path, lineage_id='corrupt', collection_handle=handle)
            with self.assertRaises(SessionError):
                tracker.observed(profile='Fixture', collection_path=path, collection_handle=handle)

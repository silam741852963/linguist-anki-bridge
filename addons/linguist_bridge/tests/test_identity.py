# SPDX-License-Identifier: GPL-3.0-or-later
import json
import multiprocessing
from pathlib import Path
import sys
import tempfile
import unittest

PACKAGE = Path(__file__).resolve().parents[1]
ROOT = PACKAGE.parent
sys.path.insert(0, str(ROOT))
from linguist_bridge.identity import IdentityError, installation_identity


def initialize(root, results):
    try:
        results.put((True, installation_identity(root)))
    except Exception as error:
        results.put((False, str(error)))


def crash_after_publication(root):
    import os
    from linguist_bridge import identity
    original = identity.os.link
    def crash(*args, **kwargs):
        original(*args, **kwargs)
        os._exit(23)
    identity.os.link = crash
    identity.installation_identity(root)


class IdentityTest(unittest.TestCase):
    def test_process_crash_after_publication_adopts_original_identity(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'private'
            process = multiprocessing.get_context('spawn').Process(target=crash_after_publication, args=(str(root),))
            process.start()
            process.join(timeout=10)
            self.assertEqual(process.exitcode, 23)
            published = json.loads((root / 'installation-id.json').read_text())['bridge_id']
            self.assertEqual(installation_identity(root), published)
            self.assertTrue(any(path.name.endswith('.tmp') for path in root.iterdir()))

    def test_identity_survives_reopen_and_concurrent_initializers(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'private'
            context = multiprocessing.get_context('spawn')
            results = context.Queue()
            processes = [context.Process(target=initialize, args=(str(root), results)) for _ in range(4)]
            for process in processes:
                process.start()
            values = [results.get(timeout=10) for _ in processes]
            for process in processes:
                process.join(timeout=10)
                self.assertEqual(process.exitcode, 0)
            self.assertTrue(all(ok for ok, _ in values), values)
            self.assertEqual(len({value for _, value in values}), 1)
            identity = installation_identity(root)
            self.assertEqual(identity, values[0][1])
            self.assertEqual(root.stat().st_mode & 0o777, 0o700)
            self.assertEqual((root / 'installation-id.json').stat().st_mode & 0o777, 0o600)
            self.assertEqual(list(root.iterdir()), [root / 'installation-id.json'])

    def test_corrupt_duplicate_unknown_or_unsafe_identity_is_never_replaced(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'private'
            installation_identity(root)
            target = root / 'installation-id.json'
            for data in [b'corrupt', b'{"schema_version":1,"schema_version":1,"bridge_id":"bad"}',
                         b'{"schema_version":true,"bridge_id":"bad"}', b'{}', b' ' * 1025]:
                target.write_bytes(data)
                with self.assertRaises(IdentityError):
                    installation_identity(root)
                self.assertEqual(target.read_bytes(), data)
            target.unlink()
            external = Path(parent) / 'external'
            external.write_text('private original')
            target.symlink_to(external)
            with self.assertRaises(IdentityError):
                installation_identity(root)
            self.assertEqual(external.read_text(), 'private original')

    def test_shared_directory_and_relative_or_symlink_roots_fail_closed(self):
        with self.assertRaises(IdentityError):
            installation_identity('relative')
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / 'shared'
            root.mkdir(mode=0o755)
            with self.assertRaises(IdentityError):
                installation_identity(root)
            self.assertFalse((root / 'installation-id.json').exists())
            link = Path(parent) / 'link'
            link.symlink_to(root, target_is_directory=True)
            with self.assertRaises(IdentityError):
                installation_identity(link)

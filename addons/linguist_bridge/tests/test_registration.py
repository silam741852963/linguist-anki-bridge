# SPDX-License-Identifier: GPL-3.0-or-later
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import types
import unittest
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge.lineage import LineageStore
from linguist_bridge.operations import OperationLedger
from linguist_bridge.payloads import VOCAB_FIELDS
from linguist_bridge import registration

PACKAGE = Path(__file__).resolve().parents[1]

UTIL = '''def api(*versions):
    def decorate(function):
        function.api = True
        function.versions = versions
        return function
    return decorate
'''
MAIN = '''class AnkiConnect:
    def handler(self, request):
        return getattr(self, request['action'])(**request.get('params', {}))
    @util.api()
    def standard(self):
        return 'unchanged'
    @util.api()
    def apiReflect(self, scopes):
        return {'actions': [name for name in dir(type(self))
                if getattr(getattr(type(self), name), 'api', False)]}
ac = AnkiConnect()
'''


class RegistrationTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.name = 'fixture_' + uuid.uuid4().hex
        self.modules = []
        (self.root / 'util.py').write_text(UTIL)
        (self.root / '__init__.py').write_text(MAIN)
        self.util = self.load(self.name + '.util', self.root / 'util.py')
        self.module = self.load(self.name, self.root / '__init__.py', self.util)
        self.pins = {name: hashlib.sha256((self.root / name).read_bytes()).hexdigest()
                     for name in ['__init__.py', 'util.py']}

    def load(self, name, path, util=None):
        spec = importlib.util.spec_from_file_location(name, path)
        module = importlib.util.module_from_spec(spec)
        sys.modules[name] = module
        self.modules.append(name)
        if util is not None:
            module.util = util
        spec.loader.exec_module(module)
        return module

    def tearDown(self):
        for name in self.modules:
            sys.modules.pop(name, None)
        self.temp.cleanup()

    def test_registration_is_additive_and_dispatches_only_supplied_read_manifest(self):
        cls = self.module.AnkiConnect
        handler = cls.handler
        standard = cls.standard
        manifest = {"actions": ["labCapabilities"], "mutation_variants": [], "collection_session": None}
        registration.register_capabilities(self.module, self.pins, lambda: manifest)
        self.assertIs(cls.handler, handler)
        self.assertIs(cls.standard, standard)
        self.assertEqual(self.module.ac.handler({'action': 'standard'}), 'unchanged')
        self.assertEqual(self.module.ac.handler({'action': 'labCapabilities'}), manifest)
        self.assertFalse(hasattr(cls, 'labMutate'))
        with self.assertRaises(registration.RegistrationError):
            registration.register_capabilities(self.module, self.pins, lambda: {})

    def test_status_action_is_additive_read_only_and_uses_exact_supplied_ids(self):
        cls = self.module.AnkiConnect
        handler = cls.handler
        seen = []
        def status(lineage_id, operation_id):
            seen.append((lineage_id, operation_id))
            return {"state": "queued", "dispatch_newly_authorized": False}
        registration.register_read_actions(
            self.module, self.pins, lambda: {"actions": ["labCapabilities", "labOperationStatus"],
                                              "mutation_variants": [], "collection_session": None},
            status,
        )
        self.assertIs(cls.handler, handler)
        self.assertEqual(self.module.ac.handler({"action": "standard"}), "unchanged")
        self.assertEqual(self.module.ac.handler({"action": "labOperationStatus", "params": {
            "lineage_id": "lineage", "operation_id": "operation",
        }}), {"state": "queued", "dispatch_newly_authorized": False})
        self.assertEqual(seen, [("lineage", "operation")])
        self.assertFalse(hasattr(cls, "labMutate"))
        self.assertEqual(self.module.ac.handler({"action": "labCapabilities"})["actions"],
                         ["labCapabilities", "labOperationStatus"])
        with self.assertRaises(registration.RegistrationError):
            registration.register_read_actions(self.module, self.pins, lambda: {}, status)

    def test_status_action_reads_durable_ledger_without_dispatch(self):
        root = self.root / "private"
        lineage_store = LineageStore(root)
        lineage_id = lineage_store.lineage("b" * 64, initialize=True)
        lineage_store.close()
        ledger = OperationLedger(root, initialize=True)
        operation_id = str(uuid.uuid4())
        marker = "lab_op_" + operation_id.replace("-", "")
        fields = {name: "" for name in VOCAB_FIELDS}
        fields.update(Expression="cat", Meaning="<p>animal</p>", Language="en")
        body = {"model_name":"Linguist Vocabulary v2", "model_manifest_digest":"b" * 64,
                "deck_id":"123", "fields":fields, "tags":[marker], "marker_tag":marker,
                "source_plan_digest":"lab-jcs-v1:plan:" + "a" * 64,
                "checkpoint_digest":"c" * 64,
                "binding":{"profile_fingerprint":"d" * 64,"path_fingerprint":"e" * 64},
                "expected_absent":True}
        payload = json.dumps({"body":body,"schema_version":1,"variant":"create_note"},
                             sort_keys=True, separators=(",", ":")).encode()
        epoch = str(uuid.uuid4())
        owner = ledger.begin_owner(lineage_id=lineage_id, session_epoch=epoch,
                                   approved_digest="lab-jcs-v1:plan:" + "a" * 64,
                                   live_operations=set())
        ledger.queue(lineage_id=lineage_id, operation_id=operation_id,
                     payload=payload, payload_digest=hashlib.sha256(payload).hexdigest(),
                     approved_digest="lab-jcs-v1:plan:" + "a" * 64,
                     session_epoch=epoch, owner_token=owner["owner_token"],
                     fence_generation=owner["fence"], variant="create_note")
        registration.register_read_actions(self.module, self.pins, lambda: {
            "actions": ["labCapabilities", "labOperationStatus"],
            "mutation_variants": [], "collection_session": None,
        }, ledger.status)
        status = self.module.ac.handler({"action": "labOperationStatus", "params": {
            "lineage_id": lineage_id, "operation_id": operation_id,
        }})
        self.assertEqual(status["state"], "queued")
        self.assertFalse(status["dispatch_newly_authorized"])
        self.assertEqual(ledger._db.execute("SELECT count(*) FROM operation_events").fetchone()[0], 1)
        ledger.close()

    def test_manifest_cannot_advertise_unregistered_or_mutating_actions(self):
        advertised = {"actions": ["labCapabilities", "labOperationStatus"],
                      "mutation_variants": [], "collection_session": None}
        registration.register_capabilities(self.module, self.pins, lambda: advertised)
        with self.assertRaisesRegex(registration.RegistrationError, "BRIDGE_MANIFEST_INVALID"):
            self.module.ac.handler({"action": "labCapabilities"})
        advertised["actions"] = ["labCapabilities"]
        advertised["mutation_variants"] = ["create_note"]
        with self.assertRaisesRegex(registration.RegistrationError, "BRIDGE_MANIFEST_INVALID"):
            self.module.ac.handler({"action": "labCapabilities"})
        advertised["mutation_variants"] = []
        self.assertEqual(self.module.ac.handler({"action": "labCapabilities"}), advertised)
        advertised["collection_session"] = {"lineage_id": "forged"}
        with self.assertRaisesRegex(registration.RegistrationError, "BRIDGE_MANIFEST_INVALID"):
            self.module.ac.handler({"action": "labCapabilities"})

    def test_unknown_sources_collision_and_unexpected_decorator_are_rejected(self):
        (self.root / 'util.py').write_text(UTIL + '\n# changed source\n')
        with self.assertRaises(registration.RegistrationError):
            registration.register_capabilities(self.module, self.pins, lambda: {})
        self.assertFalse(hasattr(self.module.AnkiConnect, 'labCapabilities'))
        (self.root / 'util.py').write_text(UTIL)
        self.module.AnkiConnect.labCapabilities = lambda self: 'existing'
        with self.assertRaises(registration.RegistrationError):
            registration.register_capabilities(self.module, self.pins, lambda: {})
        self.assertEqual(self.module.ac.labCapabilities(), 'existing')
        del self.module.AnkiConnect.labCapabilities
        self.module.util.api = lambda: (lambda function: function)
        with self.assertRaises(registration.RegistrationError):
            registration.register_capabilities(self.module, self.pins, lambda: {})
        self.assertFalse(hasattr(self.module.AnkiConnect, 'labCapabilities'))

    def test_failed_post_registration_reflection_removes_only_new_action(self):
        original = self.module.AnkiConnect.apiReflect
        def broken(self, scopes):
            result = original(self, scopes)
            if hasattr(type(self), 'labCapabilities'):
                result['actions'].remove('standard')
            return result
        broken.api = True
        self.module.AnkiConnect.apiReflect = broken
        with self.assertRaises(registration.RegistrationError):
            registration.register_capabilities(self.module, self.pins, lambda: {})
        self.assertFalse(hasattr(self.module.AnkiConnect, 'labCapabilities'))
        self.assertEqual(self.module.ac.standard(), 'unchanged')

    def test_failed_status_registration_rolls_back_both_actions(self):
        original = self.module.AnkiConnect.apiReflect
        def broken(self, scopes):
            result = original(self, scopes)
            if hasattr(type(self), 'labOperationStatus'):
                result['actions'].remove('standard')
            return result
        broken.api = True
        self.module.AnkiConnect.apiReflect = broken
        with self.assertRaises(registration.RegistrationError):
            registration.register_read_actions(self.module, self.pins, lambda: {}, lambda a, b: {})
        self.assertFalse(hasattr(self.module.AnkiConnect, 'labCapabilities'))
        self.assertFalse(hasattr(self.module.AnkiConnect, 'labOperationStatus'))
        self.assertEqual(self.module.ac.standard(), 'unchanged')

# SPDX-License-Identifier: GPL-3.0-or-later
import hashlib
import importlib.util
from pathlib import Path
import sys
import tempfile
import types
import unittest
import uuid

PACKAGE = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("bridge_registration", PACKAGE / "registration.py")
registration = importlib.util.module_from_spec(spec)
spec.loader.exec_module(registration)

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
        registration.register_capabilities(self.module, self.pins, lambda: {'read_only': True})
        self.assertIs(cls.handler, handler)
        self.assertIs(cls.standard, standard)
        self.assertEqual(self.module.ac.handler({'action': 'standard'}), 'unchanged')
        self.assertEqual(self.module.ac.handler({'action': 'labCapabilities'}), {'read_only': True})
        self.assertFalse(hasattr(cls, 'labMutate'))
        with self.assertRaises(registration.RegistrationError):
            registration.register_capabilities(self.module, self.pins, lambda: {})

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

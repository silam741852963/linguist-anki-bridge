# SPDX-License-Identifier: GPL-3.0-or-later
"""Pinned, additive, read-only registration. Called only by a verified startup adapter."""
import hashlib
import inspect
import os
from pathlib import Path
import stat
from .protocol import validated_session


class RegistrationError(RuntimeError):
    pass


def _source_digest(path):
    descriptor = None
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_size > 2 * 1024 * 1024:
            raise RegistrationError("BRIDGE_SOURCE_INVALID")
        with os.fdopen(descriptor, "rb") as handle:
            descriptor = None
            data = handle.read(2 * 1024 * 1024 + 1)
        if len(data) > 2 * 1024 * 1024:
            raise RegistrationError("BRIDGE_SOURCE_INVALID")
        return hashlib.sha256(data).hexdigest()
    except OSError:
        raise RegistrationError("BRIDGE_SOURCE_UNAVAILABLE") from None
    finally:
        if descriptor is not None:
            os.close(descriptor)


def register_actions(module, expected_pins, actions):
    """Additively register named bound actions with AnkiConnect's own decorator.

    Pins must come from the verified build matrix, never endpoint/user claims.
    This starts no server, replaces no handler and never overwrites an
    existing action. Unknown sources/collisions fail closed.
    """
    if (type(expected_pins) is not dict or set(expected_pins) != {"__init__.py", "util.py"}
            or type(actions) is not dict or not actions
            or any(type(name) is not str or not name.startswith("lab") or not callable(function)
                   for name, function in actions.items())):
        raise RegistrationError("BRIDGE_REGISTRATION_INPUT_INVALID")
    names = list(actions)
    main_path = Path(getattr(module, "__file__", ""))
    if not main_path.is_absolute() or main_path.name != "__init__.py":
        raise RegistrationError("BRIDGE_SOURCE_INVALID")
    cls = getattr(module, "AnkiConnect", None)
    instance = getattr(module, "ac", None)
    util = getattr(module, "util", None)
    decorator = getattr(util, "api", None)
    if (not isinstance(cls, type) or type(instance) is not cls or not callable(decorator)
            or Path(getattr(util, "__file__", "")) != main_path.parent / "util.py"
            or inspect.getsourcefile(cls) != str(main_path)
            or inspect.getsourcefile(decorator) != str(main_path.parent / "util.py")):
        raise RegistrationError("BRIDGE_SYMBOLS_UNSUPPORTED")
    for name, expected in expected_pins.items():
        if (type(expected) is not str or len(expected) != 64
                or any(char not in "0123456789abcdef" for char in expected)
                or _source_digest(main_path.parent / name) != expected):
            raise RegistrationError("BRIDGE_SOURCE_PIN_MISMATCH")
    if any(hasattr(cls, name) or name in vars(instance) for name in names):
        raise RegistrationError("BRIDGE_ACTION_COLLISION")
    reflect = getattr(instance, "apiReflect", None)
    handler = getattr(instance, "handler", None)
    if not inspect.ismethod(handler) or not inspect.ismethod(reflect) or not getattr(reflect, "api", False):
        raise RegistrationError("BRIDGE_DISPATCHER_UNSUPPORTED")
    try:
        before = reflect(scopes=["actions"])
    except Exception:
        raise RegistrationError("BRIDGE_REFLECTION_FAILED") from None
    if (type(before) is not dict or type(before.get("actions")) is not list
            or any(name in before["actions"] for name in names)):
        raise RegistrationError("BRIDGE_DISPATCHER_UNSUPPORTED")
    for name, function in actions.items():
        action = decorator()(function)
        if (action is not function or getattr(action, "api", None) is not True
                or getattr(action, "versions", None) != ()):
            raise RegistrationError("BRIDGE_DECORATOR_UNSUPPORTED")
    for name, action in actions.items():
        setattr(cls, name, action)
    try:
        after = reflect(scopes=["actions"])
        if (type(after) is not dict or type(after.get("actions")) is not list
                or sorted(after["actions"]) != sorted(before["actions"] + names)
                or getattr(instance.handler, "__func__", None) is not handler.__func__):
            raise RegistrationError("BRIDGE_DISPATCHER_UNSUPPORTED")
    except Exception:
        for name, action in actions.items():
            if vars(cls).get(name) is action:
                delattr(cls, name)
        raise RegistrationError("BRIDGE_REGISTRATION_FAILED") from None
    return actions


def register_read_actions(module, expected_pins, manifest_supplier, status_supplier=None):
    """Read-only subset: `labCapabilities` and optionally `labOperationStatus`."""
    if (not callable(manifest_supplier)
            or (status_supplier is not None and not callable(status_supplier))):
        raise RegistrationError("BRIDGE_REGISTRATION_INPUT_INVALID")
    names = ["labCapabilities"] + (["labOperationStatus"] if status_supplier is not None else [])

    def labCapabilities(self):
        manifest = manifest_supplier()
        if (type(manifest) is not dict or manifest.get("actions") != names
                or manifest.get("mutation_variants") != []):
            raise RegistrationError("BRIDGE_MANIFEST_INVALID")
        try:
            validated_session(manifest.get("collection_session"))
        except ValueError:
            raise RegistrationError("BRIDGE_MANIFEST_INVALID") from None
        return manifest

    def labOperationStatus(self, lineage_id, operation_id):
        return status_supplier(lineage_id, operation_id)

    actions = {"labCapabilities": labCapabilities}
    if status_supplier is not None:
        actions["labOperationStatus"] = labOperationStatus
    if type(expected_pins) is not dict or set(expected_pins) != {"__init__.py", "util.py"}:
        raise RegistrationError("BRIDGE_REGISTRATION_INPUT_INVALID")
    return register_actions(module, expected_pins, actions)


def register_capabilities(module, expected_pins, manifest_supplier):
    return register_read_actions(module, expected_pins, manifest_supplier)["labCapabilities"]

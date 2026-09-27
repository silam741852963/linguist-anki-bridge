# SPDX-License-Identifier: GPL-3.0-or-later
"""Pinned, additive, read-only registration. Called only by a verified startup adapter."""
import hashlib
import inspect
import os
from pathlib import Path
import stat


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


def register_capabilities(module, expected_pins, manifest_supplier):
    """Pins must come from the verified build matrix, never endpoint/user claims.

    This function starts no server, replaces no handler and registers no controls.
    It is not called by package import. Unknown sources/collisions fail closed.
    """
    if set(expected_pins) != {"__init__.py", "util.py"} or not callable(manifest_supplier):
        raise RegistrationError("BRIDGE_REGISTRATION_INPUT_INVALID")
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
    if hasattr(cls, "labCapabilities") or "labCapabilities" in vars(instance):
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
            or "labCapabilities" in before["actions"]):
        raise RegistrationError("BRIDGE_DISPATCHER_UNSUPPORTED")

    def labCapabilities(self):
        return manifest_supplier()

    action = decorator()(labCapabilities)
    if action is not labCapabilities or getattr(action, "api", None) is not True or getattr(action, "versions", None) != ():
        raise RegistrationError("BRIDGE_DECORATOR_UNSUPPORTED")
    setattr(cls, "labCapabilities", action)
    try:
        after = reflect(scopes=["actions"])
        if (type(after) is not dict or type(after.get("actions")) is not list
                or sorted(after["actions"]) != sorted(before["actions"] + ["labCapabilities"])
                or getattr(instance.handler, "__func__", None) is not handler.__func__):
            raise RegistrationError("BRIDGE_DISPATCHER_UNSUPPORTED")
    except Exception:
        if vars(cls).get("labCapabilities") is action:
            delattr(cls, "labCapabilities")
        raise RegistrationError("BRIDGE_REGISTRATION_FAILED") from None
    return action

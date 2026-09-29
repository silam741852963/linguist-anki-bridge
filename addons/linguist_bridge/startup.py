# SPDX-License-Identifier: GPL-3.0-or-later
"""Explicit read-only activation after Anki's main window is initialized."""
import os
from pathlib import Path
import stat
import sys
import threading

from .compatibility import read_source_pins
from .identity import installation_identity
from .operations import OperationLedger
from .protocol import build_capabilities
from .registration import RegistrationError, _source_digest, register_read_actions


class StartupError(RuntimeError):
    pass


_runtime = None
last_startup_error = None


class ReadOnlyRuntime:
    def __init__(self, module, actions, ledger):
        self._module = module
        self._actions = actions
        self._ledger = ledger

    def close(self):
        for name, action in self._actions.items():
            if vars(self._module.AnkiConnect).get(name) is action:
                delattr(self._module.AnkiConnect, name)
        self._actions = {}
        self._ledger.close()


def _state_root(main_window):
    base = getattr(getattr(main_window, "pm", None), "base", None)
    try:
        base = Path(base)
        info = base.lstat()
        if (not base.is_absolute() or base.resolve() != base
                or not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid()):
            raise StartupError("BRIDGE_STATE_PARENT_INVALID")
    except (OSError, TypeError, ValueError, RuntimeError):
        raise StartupError("BRIDGE_STATE_PARENT_INVALID") from None
    return base / "linguist-anki-bridge-native"


def activate_read_only(*, main_window, anki_connect_module, anki_version, build_hash):
    """Register two read actions after exact build/source verification.

    The returned runtime must be kept alive while actions are registered. This
    function never examines or changes the collection and declares no session.
    """
    if threading.current_thread() is not threading.main_thread():
        raise StartupError("BRIDGE_STARTUP_THREAD_INVALID")
    pins = read_source_pins(anki_version, build_hash)
    source = Path(getattr(anki_connect_module, "__file__", ""))
    if not source.is_absolute() or source.name != "__init__.py":
        raise StartupError("BRIDGE_SOURCE_INVALID")
    try:
        if any(_source_digest(source.parent / name) != expected
               for name, expected in pins.items()):
            raise StartupError("BRIDGE_SOURCE_PIN_MISMATCH")
    except RegistrationError:
        raise StartupError("BRIDGE_SOURCE_PIN_MISMATCH") from None
    root = _state_root(main_window)
    bridge_id = installation_identity(root)
    ledger = OperationLedger(root, initialize=True)
    try:
        def manifest():
            key = anki_connect_module.util.setting("apiKey")
            return build_capabilities(
                bridge_id=bridge_id, anki_version=anki_version,
                anki_connect_source_digest=pins["__init__.py"],
                api_key_configured=type(key) is str and bool(key.strip()),
                operation_status_available=True,
            )

        actions = register_read_actions(anki_connect_module, pins, manifest, ledger.status)
        return ReadOnlyRuntime(anki_connect_module, actions, ledger)
    except BaseException:
        ledger.close()
        raise


def discover_anki_connect(anki_version, build_hash):
    """Find the one loaded AnkiConnect module whose bytes match the pinned build."""
    pins = read_source_pins(anki_version, build_hash)
    matches = []
    for module in tuple(sys.modules.values()):
        namespace = getattr(module, "__dict__", None)
        if type(namespace) is not dict or not {"AnkiConnect", "ac", "util"} <= namespace.keys():
            continue
        file_name = namespace.get("__file__")
        if type(file_name) is not str:
            continue
        source = Path(file_name)
        if not source.is_absolute() or source.name != "__init__.py":
            continue
        try:
            if all(_source_digest(source.parent / name) == expected
                   for name, expected in pins.items()):
                if module not in matches:
                    matches.append(module)
        except RegistrationError:
            continue
    if len(matches) != 1:
        raise StartupError("BRIDGE_ANKICONNECT_MODULE_UNAVAILABLE")
    return matches[0]


def install_read_only_hook(hook, main_window_provider, anki_version, build_hash):
    """Install one deferred callback; no sidecar or action exists until it runs."""
    if not callable(main_window_provider) or not hasattr(hook, "append"):
        raise StartupError("BRIDGE_HOOK_INVALID")
    read_source_pins(anki_version, build_hash)

    def on_main_window_ready():
        global _runtime, last_startup_error
        if _runtime is not None:
            return
        try:
            module = discover_anki_connect(anki_version, build_hash)
            _runtime = activate_read_only(
                main_window=main_window_provider(), anki_connect_module=module,
                anki_version=anki_version, build_hash=build_hash,
            )
            last_startup_error = None
        except Exception as error:
            last_startup_error = (str(error) if isinstance(error, (StartupError, RegistrationError))
                                  else "BRIDGE_STARTUP_FAILED")

    hook.append(on_main_window_ready)
    return on_main_window_ready


def install_anki_hooks():
    """Called only by the installable entrypoint, after Anki loads this add-on."""
    from anki.buildinfo import buildhash, version
    from aqt import gui_hooks
    import aqt
    return install_read_only_hook(gui_hooks.main_window_did_init, lambda: aqt.mw,
                                  version, buildhash)


def shutdown_read_only():
    global _runtime
    if _runtime is not None:
        _runtime.close()
        _runtime = None

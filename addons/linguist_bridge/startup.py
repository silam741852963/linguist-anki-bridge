# SPDX-License-Identifier: GPL-3.0-or-later
"""Explicit read-only activation after Anki's main window is initialized."""
import os
from pathlib import Path
import stat
import sys
import threading

from .compatibility import read_source_pins
from .identity import installation_identity
from .inspection import InspectionError, inspect_note_twice
from .lineage import LineageStore
from .operations import OperationLedger
from .protocol import _uuid, build_capabilities
from .registration import RegistrationError, _source_digest, register_read_actions
from .session import SessionError, SessionTracker, collection_path_fingerprint


class StartupError(RuntimeError):
    pass


_runtime = None
last_startup_error = None


class ReadOnlyRuntime:
    def __init__(self, module, actions, ledger, lineage, main_window):
        self._module = module
        self._actions = actions
        self._ledger = ledger
        self._lineage = lineage
        self._main_window = main_window
        self._session = SessionTracker()
        self._hooks = []
        self.last_session_error = None

    def bind_session_hooks(self, hooks):
        names = ("collection_did_load", "collection_will_temporarily_close",
                 "collection_did_temporarily_close", "profile_will_close")
        if self._hooks or any(not hasattr(getattr(hooks, name, None), "append")
                              or not hasattr(getattr(hooks, name, None), "remove")
                              for name in names):
            raise StartupError("BRIDGE_SESSION_HOOKS_UNSUPPORTED")

        def opened(col):
            self._session.invalidate()
            try:
                window = self._main_window
                if col is None or col is not getattr(window, "col", None):
                    raise StartupError("BRIDGE_COLLECTION_HANDLE_INVALID")
                manager = window.pm
                path = manager.collectionPath()
                lineage_id = self._lineage.lineage(
                    collection_path_fingerprint(path), initialize=True)
                self._session.opened(profile=manager.name, collection_path=path,
                                     lineage_id=lineage_id, collection_handle=col)
                self.last_session_error = None
            except Exception as error:
                self.last_session_error = (str(error) if isinstance(error, (StartupError, SessionError))
                                           else "BRIDGE_SESSION_UNAVAILABLE")

        def invalidated(*_):
            self._session.invalidate()

        callbacks = (opened, invalidated, opened, invalidated)
        try:
            for name, callback in zip(names, callbacks):
                hook = getattr(hooks, name)
                hook.append(callback)
                self._hooks.append((hook, callback))
        except Exception:
            for hook, callback in reversed(self._hooks):
                hook.remove(callback)
            self._hooks = []
            raise StartupError("BRIDGE_SESSION_HOOKS_UNSUPPORTED") from None

    def observed_session(self):
        window = self._main_window
        return self._session.observed(
            profile=window.pm.name, collection_path=window.pm.collectionPath(),
            collection_handle=window.col)

    def inspect_note(self, note_id, expected_session_epoch):
        """Internal bounded inspection tied to the current observed epoch."""
        try:
            _uuid(expected_session_epoch)
            before = self.observed_session()
        except ValueError:
            raise InspectionError("BRIDGE_INSPECT_SESSION_CONFLICT") from None
        except (SessionError, AttributeError):
            raise InspectionError("BRIDGE_INSPECT_SESSION_CONFLICT") from None
        if before["session_epoch"] != expected_session_epoch:
            raise InspectionError("BRIDGE_INSPECT_SESSION_CONFLICT")
        try:
            report = inspect_note_twice(self._main_window.col, note_id)
        finally:
            try:
                after = self.observed_session()
            except (SessionError, AttributeError):
                raise InspectionError("BRIDGE_INSPECT_SESSION_CONFLICT") from None
            if after != before:
                raise InspectionError("BRIDGE_INSPECT_SESSION_CONFLICT")
        report["collection_session"] = before
        return report

    def close(self):
        for hook, callback in reversed(self._hooks):
            hook.remove(callback)
        self._hooks = []
        self._session.invalidate()
        for name, action in self._actions.items():
            if vars(self._module.AnkiConnect).get(name) is action:
                delattr(self._module.AnkiConnect, name)
        self._actions = {}
        self._ledger.close()
        self._lineage.close()


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
    lineage = LineageStore(root)
    try:
        ledger = OperationLedger(root, initialize=True)
    except BaseException:
        lineage.close()
        raise
    try:
        runtime_ref = [None]
        def manifest():
            key = anki_connect_module.util.setting("apiKey")
            session = None
            if runtime_ref[0] is not None:
                try:
                    session = runtime_ref[0].observed_session()
                except (SessionError, AttributeError):
                    pass
            return build_capabilities(
                bridge_id=bridge_id, anki_version=anki_version,
                anki_connect_source_digest=pins["__init__.py"],
                api_key_configured=type(key) is str and bool(key.strip()),
                operation_status_available=True,
                collection_session=session,
            )

        actions = register_read_actions(anki_connect_module, pins, manifest, ledger.status)
        runtime = ReadOnlyRuntime(anki_connect_module, actions, ledger, lineage, main_window)
        runtime_ref[0] = runtime
        return runtime
    except BaseException:
        ledger.close()
        lineage.close()
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


def install_read_only_hook(hook, main_window_provider, anki_version, build_hash,
                           session_hooks=None):
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
            runtime = activate_read_only(
                main_window=main_window_provider(), anki_connect_module=module,
                anki_version=anki_version, build_hash=build_hash,
            )
            try:
                if session_hooks is not None:
                    runtime.bind_session_hooks(session_hooks)
            except BaseException:
                runtime.close()
                raise
            _runtime = runtime
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
                                  version, buildhash, session_hooks=gui_hooks)


def shutdown_read_only():
    global _runtime
    if _runtime is not None:
        _runtime.close()
        _runtime = None

# SPDX-License-Identifier: GPL-3.0-or-later
"""Main-thread session observations; no Anki/Qt hooks are activated by this module."""
import hashlib
import os
from pathlib import Path
import stat
import threading
from uuid import uuid4
from .protocol import _label, _uuid


class SessionError(RuntimeError):
    pass


def _file(path):
    try:
        path = Path(path)
        valid = path.is_absolute() and path.resolve() == path
    except (OSError, RuntimeError, TypeError, ValueError):
        raise SessionError("BRIDGE_COLLECTION_PATH_INVALID") from None
    if not valid:
        raise SessionError("BRIDGE_COLLECTION_PATH_INVALID")
    descriptor = None
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid():
            raise SessionError("BRIDGE_COLLECTION_FILE_INVALID")
        return path, (info.st_dev, info.st_ino)
    except OSError:
        raise SessionError("BRIDGE_COLLECTION_FILE_UNAVAILABLE") from None
    finally:
        if descriptor is not None:
            os.close(descriptor)


def _fingerprint(domain, text):
    try:
        return hashlib.sha256(domain.encode() + b"\0" + text.encode("utf-8")).hexdigest()
    except UnicodeError:
        raise SessionError("BRIDGE_SESSION_ENCODING_INVALID") from None


def collection_path_fingerprint(collection_path):
    path, _ = _file(collection_path)
    return _fingerprint("lab-path-v1", str(path))


class SessionTracker:
    def __init__(self):
        if threading.current_thread() is not threading.main_thread():
            raise SessionError("BRIDGE_SESSION_THREAD_INVALID")
        self._thread = threading.get_ident()
        self._record = None
        self._handle = None
        self._path = None
        self._identity = None
        self._profile = None

    def _owner(self):
        if threading.get_ident() != self._thread:
            raise SessionError("BRIDGE_SESSION_THREAD_INVALID")

    def invalidate(self):
        self._owner()
        self._record = self._handle = self._path = self._identity = self._profile = None

    def opened(self, *, profile, collection_path, lineage_id, collection_handle):
        """Each explicit load/import/restore hook creates a fresh execution epoch.

        Lineage is supplied by the future durable sidecar. Never create it here.
        A file/handle match alone is not proof against unobserved same-file restore.
        """
        self.invalidate()  # Failed re-open must not retain an old dispatch session.
        if collection_handle is None:
            raise SessionError("BRIDGE_COLLECTION_HANDLE_INVALID")
        try:
            profile = _label(profile, 128)
            lineage_id = _uuid(lineage_id)
        except ValueError:
            raise SessionError("BRIDGE_SESSION_METADATA_INVALID") from None
        path, identity = _file(collection_path)
        self._record = {
            "lineage_id": lineage_id,
            "session_epoch": str(uuid4()),
            "profile_fingerprint": _fingerprint("lab-profile-v1", profile),
            "path_fingerprint": _fingerprint("lab-path-v1", str(path)),
        }
        self._handle, self._path, self._identity, self._profile = collection_handle, path, identity, profile
        return dict(self._record)

    def observed(self, *, profile, collection_path, collection_handle):
        self._owner()
        if self._record is None:
            raise SessionError("BRIDGE_SESSION_UNAVAILABLE")
        try:
            path, identity = _file(collection_path)
        except SessionError:
            self.invalidate()
            raise
        if (profile != self._profile or path != self._path or identity != self._identity
                or collection_handle is not self._handle):
            self.invalidate()
            raise SessionError("BRIDGE_SESSION_CONFLICT")
        return dict(self._record)

# SPDX-License-Identifier: GPL-3.0-or-later
"""Private, create-new installation identity. No Anki/Qt imports or session claims."""
import json
import os
from pathlib import Path
import stat
from uuid import uuid4
from .protocol import _uuid


class IdentityError(RuntimeError):
    pass


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise IdentityError("BRIDGE_IDENTITY_INVALID")
        result[key] = value
    return result


def _read(directory):
    descriptor = None
    try:
        descriptor = os.open("installation-id.json", os.O_RDONLY | os.O_NOFOLLOW,
                             dir_fd=directory)
        info = os.fstat(descriptor)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or stat.S_IMODE(info.st_mode) != 0o600 or info.st_size > 1024):
            raise IdentityError("BRIDGE_IDENTITY_INVALID")
        with os.fdopen(descriptor, "rb") as handle:
            descriptor = None
            data = handle.read(1025)
        if len(data) > 1024:
            raise IdentityError("BRIDGE_IDENTITY_INVALID")
        record = json.loads(data, object_pairs_hook=_object)
        if (type(record) is not dict or set(record) != {"schema_version", "bridge_id"}
                or type(record["schema_version"]) is not int or record["schema_version"] != 1):
            raise IdentityError("BRIDGE_IDENTITY_INVALID")
        return _uuid(record["bridge_id"])
    except FileNotFoundError:
        raise
    except (OSError, ValueError, UnicodeError):
        raise IdentityError("BRIDGE_IDENTITY_INVALID") from None
    finally:
        if descriptor is not None:
            os.close(descriptor)


def installation_identity(root):
    """Initialize explicitly in a private existing-parent directory; never repair corrupt identity.

    UUID persistence identifies an installation only. It proves neither collection
    lineage nor session continuity. This function is not called during import/read.
    """
    root = Path(root)
    if not root.is_absolute() or root.resolve() != root:
        raise IdentityError("BRIDGE_IDENTITY_PATH_INVALID")
    directory = None
    temporary = None
    try:
        try:
            root.mkdir(mode=0o700)
        except FileExistsError:
            pass
        directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        info = os.fstat(directory)
        if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
            raise IdentityError("BRIDGE_IDENTITY_DIRECTORY_INVALID")
        parent = os.open(root.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
        try:
            existing = _read(directory)
            os.fsync(directory)
            return existing
        except FileNotFoundError:
            pass
        bridge_id = str(uuid4())
        record = json.dumps({"schema_version": 1, "bridge_id": bridge_id},
                            separators=(",", ":"), sort_keys=True).encode() + b"\n"
        temporary = ".installation-id-" + uuid4().hex + ".tmp"
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                             0o600, dir_fd=directory)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(record)
            handle.flush()
            os.fsync(handle.fileno())
        try:
            os.link(temporary, "installation-id.json", src_dir_fd=directory,
                    dst_dir_fd=directory, follow_symlinks=False)
        except FileExistsError:
            pass  # Adopt the validated winning identity, never overwrite it.
        os.unlink(temporary, dir_fd=directory)
        temporary = None
        os.fsync(directory)
        return _read(directory)
    except IdentityError:
        raise
    except OSError:
        raise IdentityError("BRIDGE_IDENTITY_IO_FAILED") from None
    finally:
        if directory is not None:
            if temporary is not None:
                try:
                    os.unlink(temporary, dir_fd=directory)
                except OSError:
                    pass
            os.close(directory)

# SPDX-License-Identifier: GPL-3.0-or-later
"""Durable sidecar lineage metadata. No collection mutation or operation dispatch."""
from contextlib import contextmanager
import fcntl
import os
from pathlib import Path
import sqlite3
import stat
import time
from uuid import uuid4
from .identity import installation_identity
from .protocol import _digest, _uuid


class LineageError(RuntimeError):
    pass


class LineageStore:
    def __init__(self, root, *, busy_timeout_ms=5000):
        self._db = None
        if type(busy_timeout_ms) is not int or not 1 <= busy_timeout_ms <= 60000:
            raise LineageError("BRIDGE_SIDECAR_TIMEOUT_INVALID")
        bridge_id = installation_identity(root)
        root = Path(root)
        path = root / 'native-sidecar.sqlite3'
        created = False
        lock = None
        try:
            lock = os.open(root / '.native-sidecar-init.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
            lock_info = os.fstat(lock)
            if not stat.S_ISREG(lock_info.st_mode) or lock_info.st_uid != os.getuid() or stat.S_IMODE(lock_info.st_mode) != 0o600:
                raise LineageError("BRIDGE_SIDECAR_LOCK_INVALID")
            deadline = time.monotonic() + busy_timeout_ms / 1000
            while True:
                try:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() >= deadline:
                        raise LineageError("BRIDGE_SIDECAR_LOCK_TIMEOUT") from None
                    time.sleep(min(0.01, max(0, deadline - time.monotonic())))
            try:
                descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
                created = True
                os.close(descriptor)
            except FileExistsError:
                pass
            for candidate in [path, Path(str(path) + '-wal'), Path(str(path) + '-shm')]:
                try:
                    info = candidate.lstat()
                except FileNotFoundError:
                    continue
                if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o600:
                    raise LineageError("BRIDGE_SIDECAR_FILE_INVALID")
            before = path.stat()
            self._db = sqlite3.connect(path, timeout=busy_timeout_ms / 1000, isolation_level=None)
            after = path.lstat()
            if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino) or not stat.S_ISREG(after.st_mode):
                raise LineageError("BRIDGE_SIDECAR_FILE_CONFLICT")
            self._db.execute('PRAGMA trusted_schema=OFF')
            self._db.execute('PRAGMA foreign_keys=ON')
            self._db.execute('PRAGMA synchronous=FULL')
            self._db.execute(f'PRAGMA busy_timeout={busy_timeout_ms}')
            version = self._db.execute('PRAGMA user_version').fetchone()[0]
            if version == 0 and created:
                with self._transaction():
                    self._db.execute('CREATE TABLE installation (singleton INTEGER PRIMARY KEY CHECK(singleton=1), bridge_id TEXT NOT NULL)')
                    self._db.execute('INSERT INTO installation VALUES (1,?)', (bridge_id,))
                    self._db.execute('CREATE TABLE lineages (path_fingerprint TEXT PRIMARY KEY, lineage_id TEXT NOT NULL UNIQUE)')
                    self._db.execute('PRAGMA user_version=1')
            elif version != 1:
                raise LineageError("BRIDGE_SIDECAR_SCHEMA_UNSUPPORTED")
            if self._db.execute('PRAGMA quick_check').fetchall() != [('ok',)]:
                raise LineageError("BRIDGE_SIDECAR_CORRUPT")
            if self._db.execute('SELECT bridge_id FROM installation WHERE singleton=1').fetchone() != (bridge_id,):
                raise LineageError("BRIDGE_SIDECAR_INSTALLATION_CONFLICT")
            if self._db.execute('PRAGMA journal_mode=WAL').fetchone()[0] != 'wal':
                raise LineageError("BRIDGE_SIDECAR_DURABILITY_UNAVAILABLE")
            directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        except Exception as error:
            self.close()
            if isinstance(error, LineageError):
                raise
            raise LineageError("BRIDGE_SIDECAR_OPEN_FAILED") from None
        finally:
            if lock is not None:
                os.close(lock)

    @contextmanager
    def _transaction(self):
        self._db.execute('BEGIN IMMEDIATE')
        try:
            yield
            self._db.execute('COMMIT')
        except BaseException:
            self._db.execute('ROLLBACK')
            raise

    def lineage(self, path_fingerprint, *, initialize=False):
        """Explicit initialization only; missing existing lineage is recovery, not auto-repair."""
        try:
            _digest(path_fingerprint)
        except ValueError:
            raise LineageError("BRIDGE_LINEAGE_PATH_INVALID") from None
        if type(initialize) is not bool:
            raise LineageError("BRIDGE_LINEAGE_INPUT_INVALID")
        try:
            with self._transaction():
                row = self._db.execute('SELECT lineage_id FROM lineages WHERE path_fingerprint=?', (path_fingerprint,)).fetchone()
                if row is None:
                    if not initialize:
                        raise LineageError("BRIDGE_LINEAGE_MISSING")
                    lineage_id = str(uuid4())
                    self._db.execute('INSERT INTO lineages VALUES (?,?)', (path_fingerprint, lineage_id))
                    return lineage_id
                try:
                    return _uuid(row[0])
                except ValueError:
                    raise LineageError("BRIDGE_LINEAGE_CORRUPT") from None
        except LineageError:
            raise
        except (sqlite3.Error, AttributeError):
            raise LineageError("BRIDGE_LINEAGE_IO_FAILED") from None

    def close(self):
        if self._db is not None:
            self._db.close()
            self._db = None

#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Build the deterministic, explicitly installable lab-native-v1 Anki companion."""
import argparse
import json
import os
from pathlib import Path
import stat
from uuid import uuid4
import zipfile


SOURCE = Path(__file__).resolve().parent / "linguist_bridge"
RUNTIME = (
    "compatibility.py", "effects.py", "identity.py", "inspection.py", "lineage.py",
    "manifest.py", "native.py", "operations.py", "payloads.py", "protocol.py",
    "registration.py", "session.py", "startup.py",
)


def build(output):
    output = Path(output)
    if not output.is_absolute() or output.suffix != ".ankiaddon":
        raise ValueError("ADDON_OUTPUT_INVALID")
    parent = output.parent
    if parent.resolve() != parent or not stat.S_ISDIR(parent.lstat().st_mode):
        raise ValueError("ADDON_OUTPUT_PARENT_INVALID")
    manifest = json.dumps({
        "package": "linguist_bridge",
        "name": "Linguist Anki Bridge companion",
    }, sort_keys=True, separators=(",", ":")).encode() + b"\n"
    files = {"__init__.py": (SOURCE / "addon_entrypoint.py").read_bytes(),
             "LICENSE": (SOURCE / "LICENSE").read_bytes(),
             "manifest.json": manifest}
    files.update({name: (SOURCE / name).read_bytes() for name in RUNTIME})
    temporary = parent / ("." + output.name + "." + uuid4().hex + ".tmp")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            descriptor = None
            with zipfile.ZipFile(handle, "w", compression=zipfile.ZIP_DEFLATED,
                                 compresslevel=9) as archive:
                for name in sorted(files):
                    info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                    info.compress_type = zipfile.ZIP_DEFLATED
                    info.external_attr = 0o644 << 16
                    archive.writestr(info, files[name], compress_type=zipfile.ZIP_DEFLATED,
                                     compresslevel=9)
            handle.flush()
            os.fsync(handle.fileno())
        os.link(temporary, output, follow_symlinks=False)
        parent_descriptor = os.open(parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.unlink(temporary)
            os.fsync(parent_descriptor)
        finally:
            os.close(parent_descriptor)
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if temporary.exists():
            os.unlink(temporary)
    return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="absolute create-new .ankiaddon path")
    arguments = parser.parse_args()
    print(build(arguments.output))

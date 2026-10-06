#!/usr/bin/env python3
"""Run real Anki desktop on a DISPOSABLE base folder for the native scenarios.

Run with the Python that ships Anki (on this machine `/usr/bin/python3.14`):

    disposable-anki-desktop.py --dir DIR --ankiconnect ZIP --addon ANKIADDON --api-key KEY

First run (empty DIR): creates a new base folder with one profile
(`Disposable`) and a new collection, installs AnkiConnect (whose
`__init__.py`/`util.py` must match the pinned SHA-256) and the Linguist
companion through Anki's own add-on installer (`AddonManager.install`), and
configures AnkiConnect with the API key and a free loopback port. A later run
on the same DIR (marker file present) reuses it, so a crash/restart can be
tested. It then starts `anki -b BASE -p Disposable` offscreen and prints one
JSON line `{"endpoint": ..., "pid": ...}` once `labCapabilities` reports a
collection session. Anki runs until stdin closes; then it is asked to exit
(`guiExitAnki`) and, failing that, terminated.

Safety: the user's profile is never opened. DIR must be empty or a previous
disposable base; the base folder is passed with `-b`; TMPDIR points inside
DIR so Anki's single-instance socket never reaches a running user Anki; port
8765 is refused; Qt runs offscreen.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import types
import urllib.request
import zipfile

PROFILE = "Disposable"
ANKICONNECT_PACKAGE = "2055492159"
PINS = {
    "__init__.py": "629566e8eea59f3d67abf1b2339d5c0c621b2d894139e8335db030d022582873",
    "util.py": "059b4c6170bdb40286a040e4a7950bccd69259ad5c8dd16e2f0fa2587766b18d",
}
MARKER = ".lab-disposable-anki"
USER_PORT = 8765


def fail(message):
    print(json.dumps({"error": message}), flush=True)
    raise SystemExit(2)


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def check_ankiconnect(path):
    with zipfile.ZipFile(path) as archive:
        for name, expected in PINS.items():
            if hashlib.sha256(archive.read(name)).hexdigest() != expected:
                fail(f"ANKICONNECT_PIN_MISMATCH: {name}")


def install_addons(base, ankiconnect, addon, api_key, port):
    """Anki's own installer, without a main window."""
    from aqt.addons import AddonManager

    folder = base / "addons21"
    folder.mkdir(exist_ok=True)
    manager = AddonManager.__new__(AddonManager)
    manager.mw = types.SimpleNamespace(pm=types.SimpleNamespace(addonFolder=lambda: str(folder)))
    manager.dirty = False
    result = manager.install(str(ankiconnect), manifest={"package": ANKICONNECT_PACKAGE,
                                                         "name": "AnkiConnect"})
    if type(result).__name__ != "InstallOk":
        fail(f"ANKICONNECT_INSTALL_FAILED: {result}")
    meta = manager.addonMeta(ANKICONNECT_PACKAGE)
    meta["config"] = {"apiKey": api_key, "webBindAddress": "127.0.0.1", "webBindPort": port,
                      "webCorsOriginList": [], "apiLogPath": None}
    manager.writeAddonMeta(ANKICONNECT_PACKAGE, meta)
    result = manager.install(str(addon))
    if type(result).__name__ != "InstallOk":
        fail(f"COMPANION_INSTALL_FAILED: {result}")
    return result.name


def create_base(base, fsrs):
    from aqt.profiles import ProfileManager
    from anki.collection import Collection

    manager = ProfileManager(ProfileManager.get_created_base_folder(str(base)))
    manager.setupMeta()
    manager.create(PROFILE)
    manager.load(PROFILE)
    manager.meta["firstRun"] = False
    manager.setLang("en_US")
    manager.save()
    profile = base / PROFILE
    profile.mkdir(exist_ok=True)
    col = Collection(str(profile / "collection.anki2"))
    if fsrs:
        col.set_config("fsrs", True)
    col.close()


def call(endpoint, key, action, params=None, timeout=10):
    body = json.dumps({"action": action, "version": 6, "key": key,
                       "params": params or {}}).encode()
    request = urllib.request.Request(endpoint, body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read())


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dir", required=True, type=Path)
    parser.add_argument("--ankiconnect", type=Path)
    parser.add_argument("--addon", type=Path)
    parser.add_argument("--api-key", required=True)
    parser.add_argument("--fsrs", action="store_true")
    parser.add_argument("--fault-file", type=Path)
    parser.add_argument("--anki", default="/usr/bin/anki")
    parser.add_argument("--startup-timeout", type=float, default=90)
    args = parser.parse_args()

    root = args.dir.resolve()
    home_anki = Path.home() / ".local/share/Anki2"
    if root == home_anki or home_anki in root.parents or root in home_anki.parents:
        fail("DISPOSABLE_DIR_INVALID: inside the user's Anki folder")
    base, tmp = root / "b", root / "t"
    if root.exists() and any(root.iterdir()):
        if not (root / MARKER).is_file():
            fail("DISPOSABLE_DIR_NOT_EMPTY")
        state = json.loads((root / MARKER).read_text())
        port = state["port"]
    else:
        if not args.ankiconnect or not args.addon:
            fail("FIRST_RUN_NEEDS_ADDONS")
        check_ankiconnect(args.ankiconnect)
        root.mkdir(mode=0o700, parents=True, exist_ok=True)
        base.mkdir(mode=0o700)
        tmp.mkdir(mode=0o700)
        port = free_port()
        create_base(base, args.fsrs)
        name = install_addons(base, args.ankiconnect, args.addon, args.api_key, port)
        (root / MARKER).write_text(json.dumps({"port": port, "companion": name}))
    if port == USER_PORT:
        fail("DISPOSABLE_PORT_REFUSED: 8765 belongs to the user's Anki")
    socket_path = tmp / ("anki" + "0" * 40)
    if len(str(socket_path)) > 100:
        fail("DISPOSABLE_DIR_TOO_LONG: the private Qt socket path would not fit")
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(("ANKI", "QT_", "LINGUIST_BRIDGE"))}
    environment.update(TMPDIR=str(tmp), QT_QPA_PLATFORM="offscreen",
                       ANKI_NOHIGHDPI="1", QTWEBENGINE_CHROMIUM_FLAGS="--disable-gpu")
    if args.fault_file:
        environment["LINGUIST_BRIDGE_FAULT_FILE"] = str(args.fault_file)
    with socket.socket() as probe:
        if probe.connect_ex(("127.0.0.1", port)) == 0:
            fail(f"DISPOSABLE_PORT_BUSY: {port} already answers; an earlier instance is still running")
    log = open(root / "anki.log", "ab")
    process = subprocess.Popen([sys.executable, args.anki, "-b", str(base), "-p", PROFILE],
                               env=environment, stdin=subprocess.DEVNULL,
                               stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    def stop(*_):
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
        raise SystemExit(3)
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    endpoint = f"http://127.0.0.1:{port}"
    deadline = time.monotonic() + args.startup_timeout
    manifest = None
    while time.monotonic() < deadline and process.poll() is None:
        try:
            reply = call(endpoint, args.api_key, "labCapabilities", timeout=2)
            if reply.get("error") is None and reply["result"]["collection_session"]:
                manifest = reply["result"]
                break
        except OSError:
            pass
        time.sleep(0.25)
    if manifest is None:
        process.terminate()
        fail("DISPOSABLE_ANKI_NOT_READY: see " + str(root / "anki.log"))
    print(json.dumps({"endpoint": endpoint, "pid": process.pid, "base": str(base),
                      "anki_version": manifest["integration"]["anki_version"],
                      "companion_version": manifest["companion_version"],
                      "mutation_variants": manifest["mutation_variants"]}), flush=True)

    def wait_stdin():
        sys.stdin.read()
    reader = threading.Thread(target=wait_stdin, daemon=True)
    reader.start()
    while reader.is_alive() and process.poll() is None:
        reader.join(0.25)
    if process.poll() is None:
        try:
            call(endpoint, args.api_key, "guiExitAnki", timeout=5)
            process.wait(30)
        except Exception:  # noqa: BLE001
            pass
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(15)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
    try:
        print(json.dumps({"exit_code": process.returncode}), flush=True)
    except BrokenPipeError:
        pass


if __name__ == "__main__":
    main()

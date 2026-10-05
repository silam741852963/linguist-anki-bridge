#!/usr/bin/env python3
"""Build the CLI release archive with locked dependencies and checksums.

The build uses `cargo build --locked --release -p linguist-cli`, a fixed
SOURCE_DATE_EPOCH (the HEAD commit time) and remapped source/target/registry
paths. It writes, below `dist/release/` by default:

- `linguist-anki-bridge-VERSION-TARGET/` with the binary, LICENSE and a short
  release README;
- `linguist-anki-bridge-VERSION-TARGET.tar.gz` with sorted entries, fixed
  owners, modes and mtimes;
- `build-manifest.json` with the toolchain, lockfile hash, git state, flags,
  dynamic library needs and the Qt check;
- `SHA256SUMS` for the binary, archive and manifest.

`--verify-reproducible` builds a second time in a separate target directory
and fails unless both binaries are byte-equal. The script never installs the
binary, edits shell profiles or copies anything outside the output directory.
"""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[1]
BINARY = "linguist-anki-bridge"
QT_PATTERN = re.compile(r"(^|[-_])(qt|qml|cxx-qt|qmetaobject)([-_]|$)", re.IGNORECASE)


def run(argv, env=None, capture=True):
    result = subprocess.run(
        argv, cwd=ROOT, env=env, check=False, text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None)
    if result.returncode != 0:
        detail = (result.stderr or "").strip()[-2000:]
        raise SystemExit(f"RELEASE_COMMAND_FAILED: {' '.join(argv)}\n{detail}")
    return result.stdout or ""


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def workspace_version():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^\[workspace\.package\][^\[]*?^version\s*=\s*"([^"]+)"', text, re.M | re.S)
    if not match:
        raise SystemExit("RELEASE_VERSION_MISSING: workspace.package.version")
    return match.group(1)


def host_target():
    for line in run(["rustc", "-vV"]).splitlines():
        if line.startswith("host: "):
            return line.split(": ", 1)[1].strip()
    raise SystemExit("RELEASE_TARGET_UNKNOWN")


def git_state():
    commit = run(["git", "rev-parse", "HEAD"]).strip()
    epoch = run(["git", "log", "-1", "--format=%ct", "HEAD"]).strip()
    dirty = bool(run(["git", "status", "--porcelain", "--untracked-files=no"]).strip())
    return commit, int(epoch), dirty


def qt_closure():
    """Every package in the CLI's normal and build dependency closure."""
    out = run(["cargo", "tree", "--locked", "-p", "linguist-cli", "-e", "normal,build",
               "--prefix", "none", "--format", "{p}"])
    packages = sorted({line.split(" ")[0] for line in out.splitlines() if line.strip()})
    return packages, [name for name in packages if QT_PATTERN.search(name)]


def needed_libraries(binary):
    try:
        out = subprocess.run(["readelf", "-d", str(binary)], check=True, text=True,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    return sorted(re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", out))


def build(target_dir, epoch):
    env = dict(os.environ)
    home = Path.home()
    cargo_home = Path(env.get("CARGO_HOME", home / ".cargo"))
    remaps = [
        f"--remap-path-prefix={ROOT}=/build/linguist-anki-bridge",
        f"--remap-path-prefix={target_dir}=/build/target",
        f"--remap-path-prefix={cargo_home}=/cargo",
    ]
    env["SOURCE_DATE_EPOCH"] = str(epoch)
    env["RUSTFLAGS"] = " ".join(remaps)
    env["CARGO_TARGET_DIR"] = str(target_dir)
    env["CARGO_INCREMENTAL"] = "0"
    env.pop("RUSTC_WRAPPER", None)
    run(["cargo", "build", "--locked", "--release", "-p", "linguist-cli"], env=env, capture=True)
    return target_dir / "release" / BINARY, env["RUSTFLAGS"]


def deterministic_tar(source_dir, archive, epoch):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.PAX_FORMAT) as tar:
        entries = [source_dir] + sorted(source_dir.rglob("*"))
        for path in entries:
            info = tar.gettarinfo(str(path), arcname=str(path.relative_to(source_dir.parent)))
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mtime = epoch
            info.pax_headers = {}
            if path.is_dir():
                info.mode = 0o755
                tar.addfile(info)
            else:
                info.mode = 0o755 if os.access(path, os.X_OK) else 0o644
                with open(path, "rb") as handle:
                    tar.addfile(info, handle)
    with open(archive, "wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as gz:
            gz.write(buffer.getvalue())


RELEASE_README = """linguist-anki-bridge {version} ({target})

Command-line tool for preparing and reviewing Anki vocabulary and grammar
cards. This archive contains one binary; it does not
need Qt, Python or a desktop session.

Install by copying `{binary}` to a directory on PATH yourself. Nothing in this
archive installs itself.

Start with:
  {binary} --help
  {binary} config init
  {binary} doctor --local

Collection writes are not available in this release: apply, restore, backup
creation and model installation stop with CAPABILITY_UNAVAILABLE until the
native Anki transport is verified. See docs/cli/implementation/status.md and
docs/cli/implementation/wp-15.md in the source repository for the release
evidence and its known limits.

Verify the archive with SHA256SUMS before use.
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=ROOT / "dist" / "release")
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target" / "release-build")
    parser.add_argument("--verify-reproducible", action="store_true",
                        help="build twice in separate target directories and compare")
    parser.add_argument("--clean", action="store_true",
                        help="remove both release target directories first (no cached artifacts)")
    parser.add_argument("--allow-dirty", action="store_true",
                        help="build from a working tree with tracked changes (recorded)")
    args = parser.parse_args()

    if not (ROOT / "Cargo.lock").is_file():
        raise SystemExit("RELEASE_LOCKFILE_MISSING")
    commit, epoch, dirty = git_state()
    if dirty and not args.allow_dirty:
        raise SystemExit("RELEASE_TREE_DIRTY: commit or pass --allow-dirty (recorded in the manifest)")
    version = workspace_version()
    target = host_target()
    packages, qt = qt_closure()
    if qt:
        raise SystemExit(f"RELEASE_QT_DEPENDENCY: {', '.join(qt)}")

    target_dir = args.target_dir.resolve()
    if args.clean:
        for directory in (target_dir, target_dir.with_name(target_dir.name + "-verify")):
            shutil.rmtree(directory, ignore_errors=True)
    binary, rustflags = build(target_dir, epoch)
    binary_sha = sha256(binary)
    reproducible = None
    if args.verify_reproducible:
        second_dir = target_dir.with_name(target_dir.name + "-verify")
        second, _ = build(second_dir, epoch)
        reproducible = sha256(second) == binary_sha
        if not reproducible:
            raise SystemExit(f"RELEASE_NOT_REPRODUCIBLE: {binary} and {second} differ")

    needed = needed_libraries(binary)
    if needed is not None and any(QT_PATTERN.search(name) or name.startswith("libQt") for name in needed):
        raise SystemExit(f"RELEASE_QT_LIBRARY: {needed}")

    name = f"{BINARY}-{version}-{target}"
    out = args.out.resolve()
    stage = out / name
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir(parents=True)
    shutil.copy2(binary, stage / BINARY)
    os.chmod(stage / BINARY, 0o755)
    shutil.copyfile(ROOT / "LICENSE", stage / "LICENSE")
    (stage / "README.txt").write_text(
        RELEASE_README.format(version=version, target=target, binary=BINARY), encoding="utf-8")
    archive = out / f"{name}.tar.gz"
    deterministic_tar(stage, archive, epoch)

    manifest = {
        "schema_version": 1,
        "name": BINARY,
        "version": version,
        "target": target,
        "git_commit": commit,
        "git_tracked_changes": dirty,
        "source_date_epoch": epoch,
        "rustc": run(["rustc", "-vV"]).strip().splitlines(),
        "cargo": run(["cargo", "-V"]).strip(),
        "cargo_lock_sha256": sha256(ROOT / "Cargo.lock"),
        "command": ["cargo", "build", "--locked", "--release", "-p", "linguist-cli"],
        "rustflags": rustflags.replace(str(Path.home()), "~"),
        "profile": "release (codegen-units=1, incremental=false, strip=symbols)",
        "dependency_count": len(packages),
        "qt_dependencies": qt,
        "needed_shared_libraries": needed,
        "binary_sha256": binary_sha,
        "archive_sha256": sha256(archive),
        "reproducible_rebuild_matched": reproducible,
        "clean_build": args.clean,
        "installed": False,
    }
    manifest_path = out / "build-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    sums = [
        (sha256(stage / BINARY), f"{name}/{BINARY}"),
        (sha256(archive), archive.name),
        (sha256(manifest_path), manifest_path.name),
    ]
    (out / "SHA256SUMS").write_text("".join(f"{d}  {p}\n" for d, p in sums), encoding="utf-8")
    json.dump({"archive": str(archive), "manifest": str(manifest_path),
               "binary_sha256": binary_sha, "reproducible": reproducible}, sys.stdout)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()

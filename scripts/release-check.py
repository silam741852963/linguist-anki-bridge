#!/usr/bin/env python3
"""Run the WP-15 release checks and write scoped gate evidence.

    python3 scripts/release-check.py [--msrv] [--release-build] [--benchmark]

Runs every check command, keeps redacted logs, and writes one GateEvidence
record per release gate (EV-01..EV-14, `contracts/v2/gate-evidence.schema.json`)
plus `versions.json` and a summary `README.md` into
`docs/cli/evidence/release-DATE/`. It then records each gate's status and
evidence path in `docs/cli/decisions/release-gates.json`.

A gate is `pass` only when every listed command exited 0 and every assertion
held, and only when the gate's evidence requirement is fully covered by what
ran. Gates whose requirement needs something this tree cannot exercise (the
native companion transport, crash injection at native boundaries, missing OCR
language packs) are `blocked` with an explicit failure code, even when the
partial checks listed with them passed. Nothing here installs software,
touches a user Anki profile or applies anything to a real collection; the
Anki checks use disposable collections.
"""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
HOME = str(Path.home())
ANKI_PYTHON = os.environ.get("LAB_ANKI_PYTHON", "/usr/bin/python3.14")
CLI = "target/debug/linguist-anki-bridge"
MSRV_BIN = Path.home() / ".rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin"


def jcs(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def redact(text):
    text = text.replace(str(ROOT), "<repo>").replace(HOME, "~")
    return re.sub(r"/tmp/[A-Za-z0-9._-]+", "/tmp/<disposable>", text)


class Runner:
    def __init__(self, out):
        self.out = out
        self.results = {}

    def run(self, key, argv, env=None, stdin_from=None, timeout=3600):
        environment = dict(os.environ)
        environment.update(env or {})
        started = time.monotonic()
        stdin = None
        if stdin_from:
            stdin = subprocess.run(stdin_from, cwd=ROOT, capture_output=True).stdout
        try:
            result = subprocess.run(argv, cwd=ROOT, env=environment, input=stdin,
                                    capture_output=True, timeout=timeout)
            code, stdout, stderr = result.returncode, result.stdout, result.stderr
        except FileNotFoundError as error:
            code, stdout, stderr = 127, b"", str(error).encode()
        except subprocess.TimeoutExpired:
            code, stdout, stderr = 124, b"", b"timeout"
        output = stdout + b"\n--- stderr ---\n" + stderr
        shown = [("<stdin: " + " ".join(stdin_from) + "> | ") if stdin_from else ""]
        redacted = [redact(part) for part in argv]
        log = self.out / "logs" / f"{key}.txt"
        log.parent.mkdir(parents=True, exist_ok=True)
        text = redact(output.decode("utf-8", "replace"))
        log.write_text(f"$ {shown[0]}{' '.join(redacted)}\nexit {code}\n\n{text[-60000:]}",
                       encoding="utf-8")
        self.results[key] = {
            "redacted_argv": redacted, "exit_code": code,
            "invocation_digest": sha256(jcs({"argv": redacted, "stdin": stdin_from})),
            "output_digest": sha256(output), "seconds": round(time.monotonic() - started, 1),
            "text": output.decode("utf-8", "replace"), "log": str(log.relative_to(ROOT)),
        }
        status = "ok" if code == 0 else f"exit {code}"
        print(f"[{status:>7}] {key} ({self.results[key]['seconds']}s)", file=sys.stderr)
        return self.results[key]

    def ok(self, key):
        return key in self.results and self.results[key]["exit_code"] == 0

    def text(self, key):
        return self.results.get(key, {}).get("text", "")


def test_counts(text):
    passed = sum(int(m) for m in re.findall(r"test result: \w+\. (\d+) passed", text))
    failed = sum(int(m) for m in re.findall(r"; (\d+) failed", text))
    return passed, failed


def version_of(argv, pattern=None):
    try:
        out = subprocess.run(argv, capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return "unavailable"
    text = (out.stdout or out.stderr).strip()
    if pattern:
        match = re.search(pattern, text)
        return match.group(1) if match else text.splitlines()[0] if text else "unavailable"
    return text.splitlines()[0] if text else "unavailable"


def lock_versions(names):
    lock = (ROOT / "Cargo.lock").read_text()
    found = {}
    for block in lock.split("[[package]]"):
        name = re.search(r'^name = "([^"]+)"', block, re.M)
        version = re.search(r'^version = "([^"]+)"', block, re.M)
        if name and version and name.group(1) in names:
            found[name.group(1)] = version.group(1)
    return found


def tesseract_languages():
    try:
        out = subprocess.run(["tesseract", "--list-langs"], capture_output=True, text=True,
                             timeout=30).stdout
    except (OSError, subprocess.TimeoutExpired):
        return []
    return [line.strip() for line in out.splitlines()[1:] if line.strip()]


def versions():
    anki = version_of([ANKI_PYTHON, "-c", "from anki.buildinfo import version, buildhash;"
                       "print(version, buildhash)"])
    matrix = {
        "linguist-anki-bridge": re.search(r'version = "([^"]+)"',
                                          (ROOT / "Cargo.toml").read_text()).group(1),
        "git_commit": version_of(["git", "-C", str(ROOT), "rev-parse", "HEAD"]),
        "rustc": version_of(["rustc", "-V"]),
        "cargo": version_of(["cargo", "-V"]),
        "rust_msrv_declared": "1.98.1",
        "python": version_of(["python3", "-V"]),
        "anki_python": version_of([ANKI_PYTHON, "-V"]),
        "anki": anki,
        "node": version_of(["node", "--version"]),
        "tesseract": version_of(["tesseract", "--version"]),
        "tesseract_languages": ",".join(tesseract_languages()) or "unavailable",
        "ollama": version_of(["ollama", "--version"], r"version is ([\w.\-]+)"),
        "kernel": version_of(["uname", "-sr"]),
    }
    matrix.update({f"crate:{k}": v for k, v in lock_versions({
        "clap", "rusqlite", "libsqlite3-sys", "reqwest", "serde", "serde_json", "serde_jcs",
        "schemars", "zip", "zstd", "image", "symphonia", "ammonia", "toml", "yaml-rust2",
        "rustls", "uuid"}).items()})
    return matrix


def fixture_hashes(paths):
    out = {}
    for pattern in paths:
        for path in sorted(ROOT.glob(pattern)):
            if path.is_file():
                out[str(path.relative_to(ROOT))] = sha256(path.read_bytes())
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--date", default=datetime.date.today().isoformat())
    parser.add_argument("--msrv", action="store_true", help="also run the suite on Rust 1.98.1")
    parser.add_argument("--release-build", action="store_true",
                        help="also run scripts/release-build.py --verify-reproducible")
    parser.add_argument("--benchmark", action="store_true",
                        help="also run the local-model benchmark (needs Ollama + gemma4:12b)")
    args = parser.parse_args()
    out = ROOT / "docs/cli/evidence" / f"release-{args.date}"
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    # Provisional summary so documentation links resolve while checks run.
    (out / "README.md").write_text(f"# Release check {args.date}\n\nIn progress.\n", encoding="utf-8")
    r = Runner(out)
    cargo_test = ["cargo", "test", "--locked"]

    # ---------------------------------------------------------------- commands
    r.run("build", ["cargo", "build", "--locked", "-p", "linguist-cli"])
    r.run("qt_tree", ["cargo", "tree", "--locked", "-p", "linguist-cli", "-e", "normal,build",
                      "--prefix", "none", "--format", "{p}"])
    r.run("workspace_tests", cargo_test + ["--workspace", "--no-fail-fast"])
    r.run("clippy", ["cargo", "clippy", "--locked", "--workspace", "--all-targets", "--",
                     "-D", "warnings"])
    r.run("fmt", ["cargo", "fmt", "--all", "--", "--check"])
    r.run("domain_contracts", cargo_test + ["-p", "linguist-core"])
    r.run("jcs_vectors", ["node", "scripts/verify-jcs-vectors.mjs"])
    r.run("v2_schemas", [ANKI_PYTHON, "scripts/verify-v2-schemas.py"])
    r.run("config_schema", [ANKI_PYTHON, "scripts/verify-config-schema.py"])
    r.run("settings_coverage", cargo_test + ["-p", "linguist-config", "--test", "coverage",
                                             "--test", "presets", "--test", "config"])
    r.run("corpus_current", ["python3", "scripts/generate-semantic-corpus.py", "--check"])
    r.run("semantic_corpus", cargo_test + ["-p", "linguist-application", "--test",
                                           "semantic_corpus", "--", "--nocapture"])
    r.run("ux", cargo_test + ["-p", "linguist-cli", "--test", "release_ux", "--test",
                              "release_coverage"])
    r.run("scenarios", cargo_test + ["-p", "linguist-cli", "--test", "release_scenarios", "--",
                                     "--ignored", "--test-threads", "1", "--nocapture"],
          env={"LAB_SCENARIO_REPORT_DIR": str(out / "scenarios"),
               "LAB_ANKI_PYTHON": ANKI_PYTHON})
    r.run("jobs_and_leases", cargo_test + ["-p", "linguist-store", "--test", "lease", "--test",
                                           "apply_job", "--test", "journal"])
    r.run("jobs_executor", cargo_test + ["-p", "linguist-application", "--test", "jobs_apply",
                                         "--test", "apply", "--test", "restore", "--test",
                                         "split", "--test", "checkpoint_writes"])
    r.run("jobs_cli", cargo_test + ["-p", "linguist-cli", "--test", "jobs_commands", "--test",
                                    "apply_commands", "--test", "restore_commands"])
    r.run("resources", cargo_test + ["-p", "linguist-application", "--test", "maintenance",
                                     "--test", "ollama"])
    r.run("resources_cli", cargo_test + ["-p", "linguist-cli", "--test", "maintenance_commands"])
    r.run("provider_reader", cargo_test + ["-p", "linguist-provider"])
    r.run("python_companion", ["python3", "-m", "unittest", "discover", "-s",
                               "addons/linguist_bridge/tests"])
    r.run("docs", ["python3", "docs/cli/validate.py"])
    r.run("op_coverage", ["python3", "scripts/op-coverage.py", "--check"])
    r.run("evidence_contract", cargo_test + ["-p", "linguist-cli", "--test", "release_evidence"])
    models = [CLI, "--output", "json", "models", "builtin"]
    for name in ["apply", "restore", "forward-mapping"]:
        r.run(f"native_{name.replace('-', '_')}", [ANKI_PYTHON, f"scripts/verify-native-{name}.py"],
              stdin_from=models)
    r.run("native_inspection", [ANKI_PYTHON, "scripts/verify-native-inspection.py"])
    r.run("native_templates", [ANKI_PYTHON, "scripts/verify-template-cards.py"], stdin_from=models)
    for name in ["checkpoint-scope", "package-restore"]:
        r.run(f"native_{name.replace('-', '_')}",
              [ANKI_PYTHON, f"scripts/verify-native-{name}.py", "--cli", CLI])
    if args.msrv:
        r.run("msrv", cargo_test + ["--workspace", "--no-fail-fast"],
              env={"PATH": f"{MSRV_BIN}:{os.environ['PATH']}", "CARGO_TARGET_DIR": "target/msrv"})
    if args.release_build:
        r.run("release_build", ["python3", "scripts/release-build.py", "--verify-reproducible", "--clean"])
    if args.benchmark:
        bench = r.run("model_benchmark", ["python3", "scripts/model-benchmark.py"], timeout=7200)
        if bench["exit_code"] == 0:
            (out / "model-benchmark.json").write_text(redact(bench["text"].split("\n--- stderr")[0]),
                                                     encoding="utf-8")

    # ---------------------------------------------------------------- facts
    passed, failed = test_counts(r.text("workspace_tests"))
    qt = [line for line in r.text("qt_tree").splitlines()
          if re.search(r"(^|[-_])(qt|qml|cxx-qt|qmetaobject)([-_ ]|$)", line, re.I)]
    scenario_names = sorted(p.stem for p in (out / "scenarios").glob("*.txt")) \
        if (out / "scenarios").exists() else []
    corpus = re.search(r"semantic corpus: (\{.*\})", r.text("semantic_corpus"))
    bench = {}
    if (out / "model-benchmark.json").exists():
        bench = json.loads((out / "model-benchmark.json").read_text())
    tesseract_langs = tesseract_languages()
    matrix = versions()
    (out / "versions.json").write_text(json.dumps(matrix, indent=1, sort_keys=True) + "\n",
                                       encoding="utf-8")

    def command(*keys):
        return [{k: r.results[key][k] for k in ("redacted_argv", "invocation_digest",
                                                "exit_code", "output_digest")}
                for key in keys if key in r.results]

    def assertion(name, passed, observed):
        return {"name": name, "passed": bool(passed), "observed": observed}

    def ran(*keys):
        return all(r.ok(key) for key in keys)

    scenarios_ok = r.ok("scenarios") and len(scenario_names) == 5
    gates = {
        "EV-01": dict(
            keys=["domain_contracts", "jcs_vectors", "v2_schemas", "native_templates"],
            fixtures=["contracts/v2/fixtures/*.json", "contracts/v2/*.schema.json"],
            assertions=[
                assertion("v2 schemas and fixtures validate", r.ok("v2_schemas"), "26 schemas, 8 fixtures"),
                assertion("JCS vectors match an independent ECMAScript serializer", r.ok("jcs_vectors"), "5 vectors"),
                assertion("domain intent/render/task contract tests pass", r.ok("domain_contracts"), "linguist-core suite"),
                assertion("managed templates generate the expected cards in Anki", r.ok("native_templates"), "Anki " + matrix["anki"]),
            ], blocked=None),
        "EV-02": dict(
            keys=["settings_coverage", "config_schema"],
            fixtures=["docs/cli/configuration/settings-registry.json", "docs/cli/configuration/purpose-defaults.json"],
            assertions=[
                assertion("every registry entry has a consumer or reported unavailable feature", r.ok("settings_coverage"), "156 entries"),
                assertion("final purpose presets", r.ok("settings_coverage"), "4 presets"),
                assertion("normalized config schema covers defaults", r.ok("config_schema"), "156 entries"),
            ],
            blocked="EV02_NONDEFAULT_CONSUMER_TESTS_INCOMPLETE"),
        "EV-03": dict(keys=["python_companion"], fixtures=["addons/linguist_bridge/*.py"],
                      assertions=[assertion("companion ledger/protocol unit tests", r.ok("python_companion"), "Anki-free")],
                      blocked="NATIVE_COMPANION_NOT_REGISTERED"),
        "EV-04": dict(keys=["python_companion"], fixtures=["addons/linguist_bridge/lineage.py"],
                      assertions=[assertion("lineage helper unit tests", r.ok("python_companion"), "no live session/rebind fixture")],
                      blocked="NATIVE_SESSION_REBIND_UNTESTED"),
        "EV-05": dict(
            keys=["native_checkpoint_scope", "native_package_restore", "scenarios"],
            fixtures=["scripts/verify-native-checkpoint-scope.py", "scripts/verify-native-package-restore.py"],
            assertions=[
                assertion("Anki-exported .colpkg scope verified with decode restoration", r.ok("native_checkpoint_scope"), "notes, cards, reviews, note types, media"),
                assertion("disposable package restore keeps scheduling, history and media", r.ok("native_package_restore"), "Anki " + matrix["anki"]),
                assertion("scenario checkpoints are real verified packages", scenarios_ok, f"{len(scenario_names)} scenarios"),
            ], blocked=None),
        "EV-06": dict(
            keys=["native_forward_mapping", "native_apply", "native_restore", "scenarios"],
            fixtures=["scripts/verify-native-forward-mapping.py", "scripts/disposable-anki-lab.py"],
            assertions=[
                assertion("forward/reverse mapping keeps card IDs and history", r.ok("native_forward_mapping"), "Basic and Picture Words"),
                assertion("apply keeps IDs, scheduling and FSRS memory state", r.ok("native_apply"), "deck moves via update_card"),
                assertion("restore keeps later history", r.ok("native_restore"), "reverse mapping"),
                assertion("revamp scenarios migrate and restore with FSRS on", scenarios_ok, ", ".join(scenario_names)),
            ], blocked=None),
        "EV-07": dict(keys=["jobs_and_leases", "jobs_executor"], fixtures=["crates/linguist-store/tests/journal.rs"],
                      assertions=[assertion("journal/dedupe/unknown-outcome rules over the fake port", ran("jobs_and_leases", "jobs_executor"), "fake native port")],
                      blocked="NATIVE_BOUNDARY_FAULT_INJECTION_MISSING"),
        "EV-08": dict(
            keys=["semantic_corpus", "native_inspection", "scenarios"],
            fixtures=["crates/linguist-application/tests/fixtures/semantic/corpus-v1.json"],
            assertions=[
                assertion("120 fixtures preserve every source field and media byte", r.ok("semantic_corpus"), corpus.group(1) if corpus else "missing"),
                assertion("native inspection reads note/card/review/media", r.ok("native_inspection"), "bounded helper"),
                assertion("scenario captures archive the exact Anki fields", scenarios_ok, "CLI capture vs live note"),
            ], blocked=None),
        "EV-09": dict(keys=["scenarios", "jobs_executor"], fixtures=["crates/linguist-application/tests/split.rs"],
                      assertions=[
                          assertion("real-Anki split: sibling first, anchor keeps history, group rollback", "grammar_split" in scenario_names and r.ok("scenarios"), "grammar_split scenario"),
                          assertion("partial-group crash recovery rules", r.ok("jobs_executor"), "fake native port only"),
                      ], blocked="SPLIT_NATIVE_CRASH_RECOVERY_UNTESTED"),
        "EV-10": dict(
            keys=["scenarios", "native_restore", "native_apply"],
            fixtures=["crates/linguist-cli/tests/release_scenarios.rs"],
            assertions=[
                assertion("later reviews survive restore in real Anki", scenarios_ok, "vocab/grammar revamp"),
                assertion("a later personal edit conflicts until a field decision", scenarios_ok, "grammar_revamp"),
                assertion("home deck kept without a target mapping", scenarios_ok, "grammar_revamp"),
                assertion("filtered-deck membership blocks apply and restore", ran("native_apply", "native_restore"), "BRIDGE_FILTERED_DECK"),
            ], blocked=None),
        "EV-11": dict(
            keys=["semantic_corpus", "corpus_current"] + (["model_benchmark"] if bench else []),
            fixtures=["crates/linguist-application/tests/fixtures/semantic/corpus-v1.json"],
            assertions=[
                assertion("120 semantic fixtures pass with zero source loss, unsupported claims or leakage", r.ok("semantic_corpus"), corpus.group(1) if corpus else "missing"),
                assertion("local model schema compliance >= 95%", bench.get("schema_compliance") is not None and bench["schema_compliance"] >= 0.95,
                          f"{bench.get('generated')}/{bench.get('attempted')} with {bench.get('model')}" if bench else "benchmark not run"),
                assertion("OCR packs for jpn and vie installed", {"jpn", "vie"} <= set(tesseract_langs), "installed: " + ",".join(tesseract_langs)),
            ], blocked="OCR_BENCHMARK_LANGUAGE_PACKS_MISSING" if not {"jpn", "vie"} <= set(tesseract_langs) else None),
        "EV-12": dict(keys=["jobs_and_leases", "jobs_executor", "jobs_cli"], fixtures=["crates/linguist-application/tests/jobs_apply.rs"],
                      assertions=[assertion("lease, liveness, control, retry, mode and restart tests", ran("jobs_and_leases", "jobs_executor", "jobs_cli"), "local store; fake native port")],
                      blocked=None),
        "EV-13": dict(keys=["resources", "resources_cli", "provider_reader"], fixtures=["crates/linguist-application/tests/maintenance.rs"],
                      assertions=[assertion("hash/license/path/limit/offline/private-network resource tests", ran("resources", "resources_cli", "provider_reader"), "local and loopback only")],
                      blocked=None),
        "EV-14": dict(
            keys=["build", "qt_tree", "ux", "scenarios", "op_coverage"],
            fixtures=["crates/linguist-cli/tests/release_scenarios.rs", "scripts/disposable-anki-lab.py"],
            assertions=[
                assertion("headless build without Qt", r.ok("build") and r.ok("qt_tree") and not qt, f"{len(qt)} Qt crates"),
                assertion("fresh-home, pipes, Unicode, SSH, interrupts, limits", r.ok("ux"), "release_ux + release_coverage"),
                assertion("all 61 operations have command-level tests", r.ok("op_coverage"), "docs/cli/implementation/op-coverage.md"),
                assertion("four workflows prepare->review->apply->restore in disposable Anki", scenarios_ok, ", ".join(scenario_names)),
                assertion("CLI --apply performs the writes itself", False, "harness binds plans and transports effects; CLI apply returns CAPABILITY_UNAVAILABLE"),
            ], blocked="CLI_NATIVE_TRANSPORT_UNAVAILABLE"),
    }
    summary = []
    registry_path = ROOT / "docs/cli/decisions/release-gates.json"
    registry = json.loads(registry_path.read_text())
    for gate_id, spec in gates.items():
        commands = command(*spec["keys"])
        all_ok = commands and all(c["exit_code"] == 0 for c in commands)
        if not all_ok:
            status, failure = "fail", "RELEASE_CHECK_COMMAND_FAILED"
        elif spec["blocked"]:
            status, failure = "blocked", spec["blocked"]
        elif all(a["passed"] for a in spec["assertions"]):
            status, failure = "pass", None
        else:
            status, failure = "fail", "RELEASE_CHECK_ASSERTION_FAILED"
        artifacts = sorted({r.results[k]["log"] for k in spec["keys"] if k in r.results})
        if gate_id in ("EV-05", "EV-06", "EV-08", "EV-09", "EV-10", "EV-14") and scenario_names:
            artifacts += [str((out / "scenarios" / f"{n}.txt").relative_to(ROOT)) for n in scenario_names]
        if gate_id == "EV-11" and bench:
            artifacts.append(str((out / "model-benchmark.json").relative_to(ROOT)))
        evidence = {
            "schema_version": 1, "gate_id": gate_id, "status": status,
            "version_matrix": matrix, "fixture_hashes": fixture_hashes(spec["fixtures"]),
            "commands": commands, "assertions": spec["assertions"],
            "artifact_refs": artifacts, "failure_code": failure,
        }
        path = out / f"{gate_id}.json"
        path.write_text(json.dumps(evidence, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
        for gate in registry["gates"]:
            if gate["id"] == gate_id:
                gate["status"] = status
                gate["evidence_paths"] = [str(path.relative_to(ROOT))]
                if failure:
                    gate["failure_code"] = failure
                else:
                    gate.pop("failure_code", None)
        summary.append((gate_id, status, failure or "", "; ".join(
            f"{'✓' if a['passed'] else '✗'} {a['name']}" for a in spec["assertions"])))
    registry_path.write_text(json.dumps(registry, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    lines = [
        f"# Release check {args.date}", "",
        "Generated by `python3 scripts/release-check.py"
        + (" --msrv" if args.msrv else "") + (" --release-build" if args.release_build else "")
        + (" --benchmark" if args.benchmark else "") + "`. Gate records follow "
        "`contracts/v2/gate-evidence.schema.json`; logs are redacted (home and temporary paths).",
        "A pass is scoped to the versions in `versions.json` and to disposable collections.", "",
        f"Workspace tests: {passed} passed, {failed} failed. Scenarios: {', '.join(scenario_names) or 'none'}.",
        "", "| Gate | Status | Failure code | Assertions |", "| --- | --- | --- | --- |",
    ]
    lines += [f"| {g} | {s} | {f} | {a} |" for g, s, f, a in summary]
    lines += ["", "## Commands", "", "| Check | Exit | Seconds | Log |", "| --- | --- | --- | --- |"]
    lines += [f"| {k} | {v['exit_code']} | {v['seconds']} | [{Path(v['log']).name}](logs/{Path(v['log']).name}) |"
              for k, v in r.results.items()]
    (out / "README.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    failures = [k for k, v in r.results.items() if v["exit_code"] != 0]
    print(json.dumps({"evidence": str(out.relative_to(ROOT)), "failed_commands": failures,
                      "gates": {g: s for g, s, _, _ in summary}}, indent=1))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()

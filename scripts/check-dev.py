#!/usr/bin/env python3
"""Check the cli-dev records: decisions, gate registry, gate evidence and links.

    python3 scripts/check-dev.py [--source MAIN_CHECKOUT]

Without `--source` only this branch is checked. With it, gate owners and gap
owners must be packages of that checkout's handbook, the gate registry must
equal its `resources/release-gates.json`, evidence artifacts may live in
either tree, and the traceability record must be current. No Anki calls.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

DEV = Path(__file__).resolve().parents[1]
DOCS = DEV / "docs/cli"
errors = []


def check(condition, message):
    if not condition:
        errors.append(message)


def slug(text):
    text = re.sub(r"[`*]", "", text).lower()
    text = re.sub(r"[^\w\s-]", "", text)
    return re.sub(r"\s+", "-", text.strip())


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--source", type=Path, help="checkout of the main branch")
    args = parser.parse_args()
    source = args.source.resolve() if args.source else None

    ids = None
    if source:
        ids = set(json.loads((source / "docs/cli/reading-map.json").read_text())["records"])

    register = json.loads((DOCS / "decisions/register.json").read_text())
    check({r["id"] for r in register["decisions"]} == {f"R{i:02}" for i in range(1, 35)},
          "Final R decision coverage")
    check(all(r["status"] == "resolved" for r in register["decisions"]), "Unresolved design decisions")
    check({g["id"] for g in register["gaps"]} == {f"FG-{i:02}" for i in range(1, 25)}, "Final gap coverage")
    for r in register["decisions"] + register["gaps"]:
        check((DOCS / "decisions" / r["document"]).is_file(), "Missing decision document: " + r["id"])

    gates = json.loads((DOCS / "decisions/release-gates.json").read_text())["gates"]
    gate_ids = {g["id"] for g in gates}
    check(gate_ids == {f"EV-{i:02}" for i in range(1, 15)}, "Evidence gate coverage")
    for g in register["gaps"]:
        check(g["status"] == "approach_finalized" and g["evidence_gate"] in gate_ids,
              "Unfinalized gap: " + g["id"])
        if ids is not None:
            check(g["owner"] in ids, "Unknown gap owner: " + g["id"])

    def exists(path):
        return (DEV / path).is_file() or bool(source and (source / path).is_file())

    for gate in gates:
        gid, status = gate["id"], gate["status"]
        check(bool(gate["owners"]), "Gate without owners: " + gid)
        if ids is not None:
            check(all(owner in ids for owner in gate["owners"]), "Unknown gate owner: " + gid)
        check(status in {"not_run", "pass", "fail", "blocked"}, "Invalid gate status: " + gid)
        if status == "not_run":
            check("evidence_paths" not in gate, gid + ": not_run with evidence")
            continue
        paths = gate.get("evidence_paths", [])
        check(len(paths) == 1 and (DEV / paths[0]).is_file(), gid + ": missing evidence record")
        if len(paths) != 1 or not (DEV / paths[0]).is_file():
            continue
        evidence = json.loads((DEV / paths[0]).read_text())
        check(evidence["gate_id"] == gid, gid + ": evidence for another gate")
        check(evidence["status"] == status, gid + ": status differs from evidence")
        check(evidence.get("failure_code") == gate.get("failure_code"), gid + ": failure code differs")
        check(bool(evidence["commands"]), gid + ": no commands")
        for artifact in evidence["artifact_refs"]:
            check(exists(artifact), f"{gid}: missing artifact {artifact}")
        commands_ok = all(c["exit_code"] == 0 for c in evidence["commands"])
        if status == "pass":
            check(commands_ok and all(a["passed"] for a in evidence["assertions"]),
                  gid + ": pass with a failing command or assertion")
        if status == "blocked":
            check(commands_ok, gid + ": blocked with a failing command")
            check(bool(evidence.get("failure_code")), gid + ": blocked without a failure code")

    if source:
        check(json.loads((source / "resources/release-gates.json").read_text())["gates"] == gates,
              "Gate registry differs from the source's resources/release-gates.json")
        trace = subprocess.run([sys.executable, str(DEV / "scripts/traceability.py"), "--check",
                                "--source", str(source)], capture_output=True, text=True)
        check(trace.returncode == 0, "traceability: " + (trace.stderr or trace.stdout).strip())

    for path in sorted(DOCS.rglob("*.md")):
        if "/logs/" in str(path) or "/scenarios/" in str(path):
            continue
        text = path.read_text()
        check(text.count("```") % 2 == 0, f"{path.relative_to(DEV)}: fences")
        for target in re.findall(r"\]\(([^)\s]+)\)", text):
            if target.startswith(("http", "mailto:")):
                continue
            name, sep, fragment = target.partition("#")
            linked = (path.parent / name).resolve() if name else path
            check(linked.exists(), f"{path.relative_to(DEV)}: missing link {target}")
            if sep and fragment and linked.is_file() and linked.suffix == ".md":
                headings = [slug(h) for h in re.findall(r"^#{1,6} (.+)$", linked.read_text(), re.M)]
                check(fragment in headings, f"{path.relative_to(DEV)}: missing anchor {target}")

    if errors:
        raise SystemExit("\n".join(errors))
    print(f"PASS: {len(register['decisions'])} decisions, {len(register['gaps'])} gaps, {len(gates)} gates"
          + (", source registry and traceability current" if source else "") + "; links valid.")


if __name__ == "__main__":
    main()

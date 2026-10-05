#!/usr/bin/env python3
"""Write docs/cli/implementation/packages.md: every work package's
specification, completion audit and known limits in one document.

Package bodies are copied from `wp-01.md` … `wp-16.md` (headings demoted
one level), so the per-package files stay the source of truth. WP-01–WP-05
predate the "Known limits" convention; WP-16 recorded their limits here.
`--check` fails when the committed document is stale.
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DIR = ROOT / "docs/cli/implementation"
OUT = DIR / "packages.md"

STATUS = {
    "WP-01": "complete", "WP-02": "complete (audited by WP-16)",
    "WP-03": "**incomplete** — fallback: writes blocked", "WP-04": "complete",
    "WP-05": "complete", "WP-06": "complete, known limits", "WP-07": "complete, known limits",
    "WP-08": "complete, known limits", "WP-09": "complete, known limits",
    "WP-10": "complete, known limits", "WP-11": "complete, known limits",
    "WP-12": "complete, known limits", "WP-13": "complete, known limits",
    "WP-14": "complete, known limits", "WP-15": "complete, known limits",
    "WP-16": "complete",
}

EARLY_LIMITS = {
    "WP-01": [
        "Automatic v1→v2 promotion and v2→v1 downgrade are unavailable; v1 documents are archived losslessly only.",
        "Readiness is domain validation only; it never proves collection apply readiness (INV-18).",
        "Custom prompt loading and non-builtin kanji schemas remain unavailable (see WP-06–WP-08, WP-14).",
        "Native card, deck and history behaviour of the fixed models is proven only in disposable Anki (template test, WP-15 scenarios), not through a registered companion.",
    ],
    "WP-02": [
        "No package audit was written at the time; WP-16 checked the done criterion: every unfinished operation reports `CAPABILITY_UNAVAILABLE` and cannot mutate.",
        "`inject`/`modernize` aliases from the design are not implemented (canonical `vocab|grammar add|revamp` only).",
        "Every `LAB_*` environment variable except `LAB_CONFIG`/`LAB_PROFILE` is read as a setting override and unknown ones fail (`UNKNOWN_SETTING`), so a credential or tool variable named `LAB_*` breaks every command (review issue RI-01).",
        "Setting coverage is structural plus targeted nondefault tests, not one behaviour test per setting (EV-02 blocked).",
        "`output.language` has one value (`en`); human output is key/value lines; no man pages; only Linux is supported.",
        "Per-item JSONL streaming and richer human summaries remain pending; progress is limited to start lines on an interactive stderr.",
    ],
    "WP-03": [
        "Incomplete. The companion registers no `lab*` mutation actions; no Rust `labMutate` transport exists; `labBegin`/`labEnd` owner fencing, the main-thread critical section and ledger-to-effect linkage are not wired.",
        "Strong collection identity (incarnation/lineage handshake) and same-profile collection replacement detection are not implemented; plans carry no binding (RI-02, RI-03).",
        "Native dedupe and CAS run only in the disposable lab and the effect functions; model manifest digests are not computed natively and differ between read capture and managed manifests (RI-05).",
        "Disposable-collection evidence exists for read inspection, template cards, checkpoint scope and package restore, forward/reverse mappings, apply and restore effects (including FSRS memory state); it does not exercise AnkiConnect registration, concurrency with UI edits or crash windows.",
        "Per its done criterion, migration stays blocked and the full revamp release is not marked complete.",
    ],
    "WP-04": [
        "State must be on a local Linux filesystem; shared or network filesystems are rejected.",
        "Store schema upgrades are forward-only (now schema 13); older binaries cannot open a newer store; there is no compaction or vacuum command.",
        "Journal, snapshot and receipt durability is proven against local crash and file-size-limit injection, not against a real native effect boundary (EV-07).",
    ],
    "WP-05": [
        "Capture is a repeated read-port observation, not an atomic native snapshot; native history is unverified, so revamp drafts stop at `SOURCE_NATIVE_HISTORY_REVIEW`, which has no resolution (RI-04).",
        "Deck-model scans cap at 100,000 notes in batches of 100; explicit note-ID selectors cap at 10,000.",
        "Plain-text example fields are never split into examples; they need schema review.",
        "Duplicate candidates are review evidence only; `selection.duplicate_policy = skip_exact` is refused.",
    ],
}


def slug(text):
    """Same anchor rule as docs/cli/validate.py."""
    text = re.sub(r"[`*]", "", text).lower()
    text = re.sub(r"[^\w\s-]", "", text)
    return re.sub(r"\s+", "-", text.strip())


def body(path):
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    start = next(i for i, line in enumerate(lines) if line.startswith("## WP-"))
    title = lines[start][3:].strip()
    rest = []
    for line in lines[start + 1:]:
        if line.startswith("#"):
            line = "#" + line
        rest.append(line)
    return title, "\n".join(rest).strip()


def render():
    gates = json.loads((ROOT / "docs/cli/decisions/release-gates.json").read_text())["gates"]
    gate_line = ", ".join(f"{g['id']} {g['status']}" for g in gates)
    out = [
        "# Implementation packages — consolidated record", "",
        "Generated by `python3 scripts/consolidate-packages.py` from [wp-01.md](wp-01.md) … [wp-16.md](wp-16.md). "
        "It gathers every package's specification, completion audit and known limits in one place for the later "
        "full-implementation pass. The per-package files remain the source of truth; edit them and regenerate.", "",
        "Related records: [final review](../review/final-review-2026-10-05.md), [traceability](../review/traceability.md), "
        "[operation coverage](op-coverage.md), [invariant ownership](invariant-test-ownership.md), "
        "[setting coverage](../configuration/setting-coverage.md), [release check](../evidence/release-2026-10-05/README.md), "
        "[implementation status](status.md).", "",
        "## Current state", "",
        "The headless CLI prepares, reviews, validates, approves and exports vocabulary and grammar cards, reads Anki "
        "through AnkiConnect, and keeps durable, recoverable local plans, jobs, snapshots and journals. **It cannot write "
        "to Anki**: apply, restore, rollback, reconcile, checkpoint creation and model installation stop with "
        "`CAPABILITY_UNAVAILABLE` because the native companion transport (WP-03) is incomplete. Their orchestration is "
        "proven over a fake port and, through a test harness, over real Anki 25.09.2 effects in disposable collections.", "",
        f"Release gates: {gate_line}. Blocked gates and the remaining review issues RI-01–RI-08 in the "
        "[final review](../review/final-review-2026-10-05.md#remaining-review-issues) define the work left for full implementation.", "",
        "## Package status", "", "| Package | Status | Audit | Known limits |", "| --- | --- | --- | --- |",
    ]
    sections = []
    for number in range(1, 17):
        wp = f"WP-{number:02}"
        path = DIR / f"wp-{number:02}.md"
        title, text = body(path)
        has_audit = "### Completion audit" in text or "### Storage evidence" in text
        has_limits = "### Known limits" in text
        limits = EARLY_LIMITS.get(wp)
        if limits and not has_limits:
            text += "\n\n### Known limits (recorded by WP-16)\n\n" + "\n".join(f"- {item}" for item in limits)
            has_limits = True
        heading = f"Package {title}"
        anchor = slug(heading)
        out.append(f"| [{title}](#{anchor}) | {STATUS[wp]} | {'yes' if has_audit else 'no (see status)'} | "
                   f"{'yes' if has_limits else '—'} |")
        sections.append(f"## {heading}\n\nSource: [wp-{number:02}.md](wp-{number:02}.md)\n\n{text}\n")
    out.append("")
    return "\n".join(out) + "\n" + "\n".join(sections)


def main():
    text = render()
    if "--check" in sys.argv:
        if OUT.read_text(encoding="utf-8") != text:
            raise SystemExit(f"{OUT} is stale; run scripts/consolidate-packages.py")
        return
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()

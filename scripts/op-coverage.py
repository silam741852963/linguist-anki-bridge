#!/usr/bin/env python3
"""Write docs/cli/implementation/op-coverage.md: every OP-01..OP-61 with its
CLI handler and the tests that exercise it.

Command-level tests are found by scanning `crates/linguist-cli/tests/*.rs`
for the command words inside each `#[test]` function. Library-level tests
for write paths the CLI cannot perform yet are listed explicitly. The test
`crates/linguist-cli/tests/release_evidence.rs` re-checks that every listed
test function exists. Run with `--check` to fail when the file is stale.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs/cli/implementation/op-coverage.md"
OPS = [("OP-01", "doctor"), ("OP-02", "config init"), ("OP-03", "config show"),
       ("OP-04", "config describe"), ("OP-05", "config validate"), ("OP-06", "config set"),
       ("OP-07", "config unset"), ("OP-08", "config reset"), ("OP-09", "config import"),
       ("OP-10", "config migrate"), ("OP-11", "decks list"), ("OP-12", "decks show"),
       ("OP-13", "decks map"), ("OP-14", "decks unmap"), ("OP-15", "models list"),
       ("OP-16", "models inspect"), ("OP-17", "models install"), ("OP-18", "notes list"),
       ("OP-19", "notes show"), ("OP-20", "notes count"), ("OP-21", "vocab add"),
       ("OP-22", "vocab revamp"), ("OP-23", "grammar add"), ("OP-24", "grammar revamp"),
       ("OP-25", "plans list"), ("OP-26", "plans show"), ("OP-27", "plans diff"),
       ("OP-28", "plans edit"), ("OP-29", "plans resolve"), ("OP-30", "plans regenerate"),
       ("OP-31", "plans validate"), ("OP-32", "plans export"), ("OP-33", "plans approve"),
       ("OP-34", "apply"), ("OP-35", "jobs create"), ("OP-36", "jobs list"),
       ("OP-37", "jobs show"), ("OP-38", "jobs items"), ("OP-39", "jobs run"),
       ("OP-40", "jobs pause"), ("OP-41", "jobs resume"), ("OP-42", "jobs retry"),
       ("OP-43", "jobs cancel"), ("OP-44", "jobs rollback"), ("OP-45", "jobs delete"),
       ("OP-46", "jobs audit"), ("OP-47", "jobs migrate"), ("OP-48", "snapshots list"),
       ("OP-49", "snapshots show"), ("OP-50", "snapshots restore"), ("OP-51", "snapshots export"),
       ("OP-52", "backup create"), ("OP-53", "backup list"), ("OP-54", "backup verify"),
       ("OP-55", "cache status"), ("OP-56", "cache prune"), ("OP-57", "resources list"),
       ("OP-58", "resources install"), ("OP-59", "recover inspect"),
       ("OP-60", "recover reconcile"), ("OP-61", "completions")]
APP = "crates/linguist-application/tests/"
LIBRARY = {
    "OP-17": [APP + "checkpoint_writes.rs::created_model_is_journaled_before_call_and_verified",
              APP + "checkpoint_writes.rs::exact_model_is_reused_and_same_name_different_manifest_blocks"],
    "OP-34": [APP + "apply.rs::create_commits_with_marker_snapshot_receipt_and_owner_release",
              APP + "apply.rs::mapped_migration_retains_card_id_history_and_scheduling"],
    "OP-39": [APP + "jobs_apply.rs::simulate_never_writes_and_apply_jobs_need_the_current_flag",
              APP + "jobs_apply.rs::an_unknown_outcome_stops_dispatch_and_a_restart_reconciles_without_duplicates"],
    "OP-41": [APP + "jobs_apply.rs::pause_during_sent_write_finishes_the_item_then_resume_continues_without_duplicates"],
    "OP-44": [APP + "restore.rs::group_rollback_previews_and_restores_each_journaled_item",
              APP + "jobs_apply.rs::rollback_preview_and_group_rollback_use_the_job_as_group"],
    "OP-50": [APP + "restore.rs::restore_after_later_study_restores_content_and_keeps_new_history",
              APP + "restore.rs::later_personal_edits_conflict_until_explicitly_merged"],
    "OP-52": [APP + "checkpoint_writes.rs::verified_checkpoint_has_receipt_restoration_and_committed_journal"],
    "OP-60": [APP + "apply.rs::timeout_after_accepted_create_reconciles_to_one_note",
              APP + "apply.rs::unknown_add_with_exact_marker_candidate_is_adopted"],
}
SCENARIOS = "crates/linguist-cli/tests/release_scenarios.rs::"
REAL_ANKI = {
    "OP-21": ["vocab_add_prepare_review_apply_restore"],
    "OP-22": ["vocab_revamp_capture_migrate_study_restore"],
    "OP-23": ["grammar_add_prepare_review_apply_study_restore_keeps_history"],
    "OP-24": ["grammar_revamp_capture_migrate_study_restore", "grammar_revamp_multi_unit_split_apply_rollback"],
    "OP-34": ["vocab_add_prepare_review_apply_restore", "vocab_revamp_capture_migrate_study_restore"],
    "OP-44": ["grammar_revamp_multi_unit_split_apply_rollback"],
    "OP-50": ["vocab_add_prepare_review_apply_restore", "grammar_revamp_capture_migrate_study_restore"],
    "OP-29": ["grammar_revamp_multi_unit_split_apply_rollback"],
}
WRITE_UNAVAILABLE = {"OP-17", "OP-34", "OP-44", "OP-50", "OP-52", "OP-60"}


def cli_tests():
    tests = []
    for path in sorted((ROOT / "crates/linguist-cli/tests").glob("*.rs")):
        text = path.read_text()
        for match in re.finditer(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn (\w+)\(\)(.*?)(?=\n#\[test\]|\n#\[cfg|\Z)", text, re.S):
            tests.append((path.name, match.group(1), match.group(2)))
    return tests


def handler_line(words):
    main = (ROOT / "crates/linguist-cli/src/main.rs").read_text().splitlines()
    variant = "".join(w.capitalize() for w in words[-1].split("-"))
    group = {"config": "ConfigCommand", "decks": "DeckCommand", "models": "ModelCommand",
             "notes": "NoteCommand", "vocab": "PrepareCommand", "grammar": "PrepareCommand",
             "plans": "PlanCommand", "jobs": "JobCommand", "snapshots": "SnapshotCommand",
             "backup": "BackupCommand", "cache": "CacheCommand", "resources": "ResourceCommand",
             "recover": "RecoveryCommand"}.get(words[0])
    needles = [f"{group}::{variant}"] if group and len(words) > 1 else [f"Command::{variant}"]
    start = next(i for i, line in enumerate(main) if line.startswith("fn run(cli: Cli)"))
    for index, line in enumerate(main[start:], start + 1):
        if any(n in line for n in needles) and "=>" in "".join(main[index - 1:index + 12]):
            return f"crates/linguist-cli/src/main.rs:{index}"
    for index, line in enumerate(main, 1):
        if any(n in line for n in needles) and "fn state_write_estimate" not in "".join(main[max(0, index - 60):index]):
            return f"crates/linguist-cli/src/main.rs:{index}"
    return "crates/linguist-cli/src/main.rs"


def render():
    tests = cli_tests()
    lines = ["# Operation coverage", "",
             "Generated by `python3 scripts/op-coverage.py`; `release_evidence.rs` checks that each listed test exists.",
             "Command tests run the built binary. Library tests drive the same orchestration over the fake native port.",
             "Real-Anki tests are the ignored disposable scenarios (`scripts/release-check.py` runs them).",
             "Rows marked *write unavailable* have no CLI mutation path in this build; their `--apply` forms are tested to refuse.",
             "", "| OP | Command | Dispatch (first match arm) | Command tests | Library tests | Real-Anki scenarios |",
             "| --- | --- | --- | --- | --- | --- |"]
    for op, command in OPS:
        words = command.split()
        if len(words) == 1:
            pattern = re.compile(r'"%s"' % re.escape(words[0]))
        else:
            pattern = re.compile(r'"%s"\s*,\s*"%s"' % (re.escape(words[0]), re.escape(words[1])))
        hits = [f"{name}::{fn}" for name, fn, body in tests if pattern.search(body)]
        if not hits:
            raise SystemExit(f"{op} {command}: no command-level test")
        shown = "<br>".join(f"`{h}`" for h in hits[:4]) + (f"<br>(+{len(hits) - 4} more)" if len(hits) > 4 else "")
        library = "<br>".join(f"`{Path(t).name}`" for t in LIBRARY.get(op, [])) or "—"
        real = "<br>".join(f"`{t}`" for t in REAL_ANKI.get(op, [])) or "—"
        label = command + (" (*write unavailable*)" if op in WRITE_UNAVAILABLE else "")
        lines.append(f"| {op} | `{label}` | {handler_line(words)} | {shown} | {library} | {real} |")
    lines += ["", "## Listed tests", "",
              "Machine-checked list (`file::function`):", "", "```text"]
    listed = set()
    for op, command in OPS:
        words = command.split()
        pattern = re.compile(r'"%s"' % re.escape(words[0])) if len(words) == 1 else re.compile(
            r'"%s"\s*,\s*"%s"' % (re.escape(words[0]), re.escape(words[1])))
        for name, fn, body in tests:
            if pattern.search(body):
                listed.add(f"crates/linguist-cli/tests/{name}::{fn}")
        listed.update(LIBRARY.get(op, []))
        listed.update(SCENARIOS + t for t in REAL_ANKI.get(op, []))
    lines += sorted(listed) + ["```", ""]
    return "\n".join(lines)


def main():
    text = render()
    if "--check" in sys.argv:
        if OUT.read_text() != text:
            raise SystemExit(f"{OUT} is stale; run scripts/op-coverage.py")
        return
    OUT.write_text(text)
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()

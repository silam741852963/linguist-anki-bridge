#!/usr/bin/env python3
"""EV-11 local-model benchmark over the semantic corpus add fixtures.

For every `vocab_add`/`grammar_add` fixture of
`crates/linguist-application/tests/fixtures/semantic/corpus-v1.json` this runs
the built CLI's authored add with generation enabled against the configured
local Ollama model (`llm.model`, default gemma4:12b) and records, per item:
whether generation was attempted, whether the reply passed the schema and
field validation (`generated`), the error code otherwise, the issues the
generated draft carries and the wall time. Nothing is applied anywhere; each
item uses a private state directory that is deleted afterwards.

Schema compliance = generated / attempted, where "attempted" excludes items
the CLI refuses before inference (for example a missing accepted answer).
Generated facts always need review (GENERATED_FACT_REVIEW); this benchmark does
not judge language quality. Output: one JSON report on stdout.
"""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "crates/linguist-application/tests/fixtures/semantic/corpus-v1.json"
NOT_ATTEMPTED = {
    "GENERATION_ACCEPTED_ANSWER_REQUIRED", "GENERATION_ACCEPTED_UNIT_REQUIRED",
    "GENERATION_NOTHING_TO_FILL", "GENERATION_DISABLED",
}
SCHEMA_FAILURES = {
    "GENERATION_OUTPUT_SCHEMA_INVALID", "GENERATION_FIELD_NOT_ALLOWED",
    "GENERATION_EXAMPLE_LIMIT", "GENERATION_INCOMPLETE_EXAMPLE", "GENERATION_KIND_CONFLICT",
    "OLLAMA_COMPLETION_INCOMPLETE", "OLLAMA_COMPLETION_SCHEMA_INVALID",
    "GENERATION_OUTPUT_ENCODING", "GENERATION_OUTPUT_LIMIT",
}


def ollama(endpoint, path):
    with urllib.request.urlopen(endpoint.rstrip("/") + path, timeout=10) as response:
        return json.loads(response.read())


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--cli", type=Path, default=ROOT / "target/debug/linguist-anki-bridge")
    parser.add_argument("--endpoint", default="http://127.0.0.1:11434")
    parser.add_argument("--limit", type=int, default=0, help="first N add fixtures only")
    args = parser.parse_args()
    fixtures = [f for f in json.loads(CORPUS.read_text())["fixtures"]
                if f["workflow"] in ("vocab_add", "grammar_add") and not f["dictionary"]]
    if args.limit:
        fixtures = fixtures[: args.limit]
    items = []
    for fixture in fixtures:
        home = Path(tempfile.mkdtemp(prefix="lab-benchmark-"))
        try:
            document = home / "input.json"
            document.write_text(json.dumps(fixture["input"], ensure_ascii=False))
            command = "vocab" if fixture["workflow"] == "vocab_add" else "grammar"
            started = time.monotonic()
            result = subprocess.run(
                [str(args.cli), "--output", "json", "--purpose", fixture["purpose"],
                 "--set", f"llm.endpoint={args.endpoint}",
                 "--set", "dictionary.provider=authored", "--set", "images.search_when_missing=false",
                 "--set", "kanji.enabled=false", command, "add", "--document", str(document)],
                capture_output=True, text=True, env={"HOME": str(home), "PATH": "/usr/bin:/bin"},
                timeout=900)
            elapsed = time.monotonic() - started
            entry = {"id": fixture["id"], "exit_code": result.returncode,
                     "seconds": round(elapsed, 2)}
            try:
                output = json.loads(result.stdout)
            except json.JSONDecodeError:
                output = None
            generation = (output or {}).get("generation") or {}
            outcome = (generation.get("items") or [{}])[0]
            if not generation:
                error = json.loads(result.stderr or "{}").get("error", "") if result.stderr else ""
                entry.update(attempted=False, generated=False,
                             error=(error.split(":")[0] or "GENERATION_NOT_REQUESTED"))
            else:
                skipped = outcome.get("skipped")
                error = outcome.get("error") or skipped
                entry.update(generated=bool(outcome.get("generated")), error=error,
                             attempted=not skipped and error not in NOT_ATTEMPTED)
                if outcome.get("generated"):
                    entry["issues"] = sorted({i["code"] for i in outcome["result"]["issues"]})
            items.append(entry)
        finally:
            shutil.rmtree(home, ignore_errors=True)
    attempted = [i for i in items if i["attempted"]]
    generated = [i for i in attempted if i["generated"]]
    schema_failures = [i for i in attempted if i.get("error") in SCHEMA_FAILURES]
    seconds = sorted(i["seconds"] for i in attempted)
    try:
        running = ollama(args.endpoint, "/api/ps")
        tags = ollama(args.endpoint, "/api/tags")
    except OSError as error:
        running, tags = {"error": str(error)}, {}
    model = next((m for m in tags.get("models", []) if m["name"] == "gemma4:12b"), {})
    report = {
        "schema_version": 1, "model": "gemma4:12b", "model_digest": model.get("digest"),
        "fixtures": len(items), "attempted": len(attempted), "generated": len(generated),
        "schema_failures": len(schema_failures),
        "schema_compliance": round(len(generated) / len(attempted), 4) if attempted else None,
        "bounded_repair_used": False,
        "latency_seconds": {"min": seconds[0] if seconds else None,
                            "median": seconds[len(seconds) // 2] if seconds else None,
                            "max": seconds[-1] if seconds else None},
        "ollama_ps": [{"name": m.get("name"), "size": m.get("size"), "size_vram": m.get("size_vram")}
                      for m in running.get("models", [])] if isinstance(running, dict) else running,
        "items": items,
    }
    print(json.dumps(report, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()

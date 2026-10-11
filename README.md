# Linguist Anki Bridge — development records (`cli-dev` branch)

This branch holds the development records of the headless CLI. The source code, the companion add-on and the operational and implementation documentation live on [`main`](https://github.com/silam741852963/linguist-anki-bridge/tree/main). The archived Rust/Qt and Python applications live on [`legacy`](https://github.com/silam741852963/linguist-anki-bridge/tree/legacy).

| Path | Contents |
| --- | --- |
| [`docs/cli/decisions/`](docs/cli/decisions/README.md) | Final design decisions (R01–R34), finalized gaps (FG-01–FG-24) and the release-gate registry (EV-01–EV-14) |
| [`docs/cli/evidence/`](docs/cli/evidence/) | Dated release checks (gate records, version matrices, redacted logs) and native-transport evidence |
| [`docs/cli/review/`](docs/cli/review/) | Final review, implementation-pass review, reconciliation, safety review and the traceability record |
| [`docs/cli/reference/`](docs/cli/reference/) | Application reference, dated research, the Anki inventory and the legacy relocation record |
| `scripts/release-check.py` | Runs every release check against a `main` checkout and writes gate evidence here |
| `scripts/traceability.py` | Regenerates `docs/cli/review/traceability.md` from a `main` checkout |
| `scripts/check-dev.py` | Checks decisions, the gate registry, gate evidence, traceability and links |

## Working with a main checkout

The scripts run against a checkout of `main`, passed with `--source` (or `LINGUIST_SOURCE`; default `../linguist-anki-bridge`). A worktree keeps both side by side:

```sh
git worktree add ../linguist-anki-bridge-dev cli-dev   # from the main checkout
cd ../linguist-anki-bridge-dev
python3 scripts/check-dev.py --source ../linguist-anki-bridge
python3 scripts/traceability.py --source ../linguist-anki-bridge
python3 scripts/release-check.py --source ../linguist-anki-bridge --msrv --release-build --benchmark
```

The release check needs Anki 25.09.2 with its Python, the AnkiConnect package (`LAB_ANKICONNECT_ZIP`) and, for `--benchmark`, Ollama with `gemma4:12b`. It runs real Anki only on disposable base folders. It writes `docs/cli/evidence/release-DATE/` and `docs/cli/decisions/release-gates.json` here, and copies the registry to the source's `resources/release-gates.json`, which the binary embeds for `doctor`; commit both.

Links into the handbook point at `main` on GitHub. Records dated before 2026-10-11 describe the single-branch layout, where these files lived beside the source.

# Application specification reference

## 5. Command surface and shell contract

**RECOMMENDED canonical executable:** `linguist-anki-bridge`; documentation may use shell alias `lab`, but do not require an alias. **REVIEW R04** covers naming and compatibility with the existing Python executable.

| Command family | Proposed operations | Effect |
| --- | --- | --- |
| `doctor` | health/capability checks, optionally `--offline` | Read-only checks; explain unavailable capabilities. |
| `config` | `init`, `show`, `validate`, `set`, `import` | Explicit local configuration management. |
| `decks` | `list`, `map`, `show` | Inspect Anki decks and configure source/destination mappings. |
| `models` | `list`, `inspect`, `install PURPOSE` | Inspect models; installation previews by default and mutates only with `--apply`. |
| `notes` | `list`, `show`, `count` with selector flags | Read collection data with bounded output. |
| `add vocab` / `add grammar` | Words/patterns, authored content, files/stdin/CSV; grammar also images/article URLs | Resolve rows/units, enrich, and save a plan; no Anki writes. `ingest` may remain a compatibility alias. |
| `revamp vocab` / `revamp grammar` | `--note-id`, explicit ID file, or selector | Enrich existing notes and save a plan; no Anki writes. `modernize` may remain a compatibility alias. |
| `plans` | `list`, `show`, `diff`, `edit`, `validate`, `export`, `approve` | Review saved documents, mapping, media and decisions. |
| `apply PLAN_ID` | default simulation; `--apply` to write | Apply a frozen plan revision after revalidation. |
| `jobs` | `create`, `list`, `show`, `items`, `run`, `pause`, `resume`, `retry`, `cancel`, `rollback`, `delete` | Durable execution and management; dangerous operations are explicit. |
| `snapshots` | `list`, `show`, `restore ID...` | Inspect recovery history; restore previews unless `--apply`. |
| `backup` | collection backup or deck export | Explicit named backup type, location/result, and scheduling limitations. |
| `cache` | `status`, `prune` | Inspect/prune disposable cache without deleting durable plans or snapshots. |
| `completions` | generate shell completion for supported shells | Discover commands/options without launching a UI. |

All commands support `--help`; top-level supports `--version`. Global options include `--config PATH`, `--output human|json|jsonl`, `--quiet`, `--verbose`, `--no-color`, and `--non-interactive`. Endpoint/model overrides are explicit and visible in resolved configuration. `--offline` suppresses network providers; Anki/Ollama loopback service access must be documented separately from internet access.

**REQUIRED write authorization contract:** `--apply` is the explicit mutation gate. A command without it must not alter collection fields, media, models, tags, or notes. `--yes` suppresses an applicable prompt but does not enable writing, bypass validation, or override conflicts. A plan already approved at a specific revision need not trigger redundant confirmations. **REVIEW R05:** whether explicit `--apply` alone is sufficient in a terminal or whether one summary confirmation is desired. Recommended: explicit intent is sufficient; reserve prompts for irreversible deletion and exceptional overrides.

Selected direction for R05: explicit `--apply` is sufficient for the exact reviewed plan; no redundant routine prompt. Model/schema mutation and deliberate source expansion must be visible in that plan. Routine implementation work does not require repeated user approval.

Job creation records a dry-run or apply policy and the approved plan revision. Resuming cannot promote a simulation job into a write job. Recommended: require a separately created apply job from the reviewed plan.

### 5.1 Output, errors, and signals

Business results go to stdout; progress and diagnostics go to stderr. JSON stdout contains one versioned result envelope; JSONL is a sequence of versioned records. Error envelopes include a stable code, operation, affected item ID, retryability, human explanation, and next action. No ANSI formatting, spinner frames, or debug traces contaminate machine output.

Human results show selected/input counts, Inject/Modernize/Skip/Needs-review counts, warnings, plan/job/snapshot IDs, and the next useful command. Avoid printing base64 media. Color is optional and must respect `NO_COLOR` and non-terminal output. Do not rely on color, Unicode borders, a pager, or mouse interaction to communicate status.

**RECOMMENDED exit code contract:** 0 success (including valid empty selection or dry-run); 1 operational failure; 2 syntax/config/input error; 3 conflict or unresolved review; 4 partial item failure; 5 missing required service/capability; 130 interrupted by SIGINT. Help lists these meanings. JSON status must agree with the exit code. **REVIEW R06** confirms this public contract.

First interrupt cancels read-only enrichment and requests a safe pause at the current mutation boundary. Do not abandon an in-flight write without saving its uncertain outcome. Persist enough state for recovery and print `jobs resume JOB_ID`. A forced second interrupt may terminate, but restart must reconcile the journal. `jobs run` remains in the foreground in v1; use a shell/session supervisor for detachment. Separate processes may inspect and request control without sharing the write lease.

### 5.2 Example user journeys (target syntax)

```bash
linguist-anki-bridge doctor
linguist-anki-bridge config init
linguist-anki-bridge decks list
linguist-anki-bridge decks map japanese_vocab --deck "Japanese"
linguist-anki-bridge models install japanese_vocab
linguist-anki-bridge models install japanese_vocab --apply

linguist-anki-bridge add vocab --word "食べる" --language ja --note "Restaurant conversation"
linguist-anki-bridge add grammar --pattern "〜にして" --language ja --source-file grammar.txt
linguist-anki-bridge plans show PLAN_ID
linguist-anki-bridge plans diff PLAN_ID
linguist-anki-bridge plans edit PLAN_ID --field meaning_text
linguist-anki-bridge plans validate PLAN_ID
linguist-anki-bridge apply PLAN_ID
linguist-anki-bridge apply PLAN_ID --apply

linguist-anki-bridge revamp vocab --deck-key english_vocab --query 'is:new' --limit 10
linguist-anki-bridge revamp grammar --deck-key japanese_grammar --limit 10
linguist-anki-bridge add vocab --file words.csv --format csv
linguist-anki-bridge jobs create --plan PLAN_ID
linguist-anki-bridge jobs run JOB_ID
linguist-anki-bridge snapshots restore SNAPSHOT_ID
linguist-anki-bridge snapshots restore SNAPSHOT_ID --apply
```

Examples must become executable acceptance scenarios when implemented; do not advertise them in installation instructions before that point.

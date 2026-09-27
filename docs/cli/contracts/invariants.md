# Domain contracts

## Fixed invariants

| ID | Rule |
| --- | --- |
| INV-01 | Help/read/prepare/edit/validate/approve commands never mutate Anki. |
| INV-02 | A config value, `--yes`, or old job approval alone never authorizes an Anki write. |
| INV-03 | Actual write binds exact plan revision/digest, source manifest, collection identity and caller's explicit authorization. |
| INV-04 | Source text/fields/media remain recoverable; user/generated/dictionary/OCR provenance is distinct. |
| INV-05 | Ready is structural validity + meaningful content + all blocking/review decisions resolved. |
| INV-06 | Snapshot, intended effect and operation ID are durably stored before the first external mutation. |
| INV-07 | Unknown outcome is reconciled before retry/compensation; absence of local receipt is not proof of no write. |
| INV-08 | One app collection writer covers apply, model changes, backup checkpoint and restore; external Anki edits remain possible. |
| INV-09 | Retained card tasks/IDs/history/scheduling preserved through supported native mappings; new cards do not inherit maturity. |
| INV-10 | Ordinary conversion never physically deletes source media or unrecognized shared models/templates. |
| INV-11 | Completion requires read-back, matching intended state and durable receipt. |
| INV-12 | Resume never regenerates a reviewed document or replaces its pre-write snapshot. |
| INV-13 | Restore compares after-state, journals intermediate effects and preserves later retained-card reviews. |
| INV-14 | Pruning cannot remove unresolved journals, snapshots, accepted assets, backup evidence or active-reader references. |
| INV-15 | Every mutable behavior setting has one typed registry entry, validation, documented default and consumer test. |
| INV-16 | Neither model output nor external text invokes tools, shell commands, downloads or Anki writes. |
| INV-17 | Every managed collection mutation requires the verified native bridge, current binding and durable deduplication/precondition checks. |
| INV-18 | Content readiness/approval never substitutes for live apply capability, identity and checkpoint eligibility. |

These rules are not configurable off-switches. Settings choose policies within them. Invalid configurations fail rather than relax them.

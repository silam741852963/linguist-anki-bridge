# CLI handbook: reading index

Design finalized for implementation, not implemented behavior. Four workflows: vocabulary add/revamp and grammar add/revamp. Foreground CLI only. Existing code is archived under `../../legacy/`; future implementation starts at repository root.

Read [final baseline decisions](decisions/README.md) once before implementation. Detailed choices are settled; only named test gates remain.

## Smallest useful reading set

1. Select one `WP-*` whose dependencies are complete from [implementation index](implementation/README.md).
2. Read [invariants](contracts/invariants.md), the chosen package and its referenced `OP-*`/`ALG-*` sections. Commands also require [shared rules](operations/README.md); writes also require [journal/lease rules](recovery/README.md) and [step protocol](recovery/journal.md).
3. Read only relevant settings via `read.py --setting PREFIX`. Schema/config work also needs [configuration rules](configuration/README.md).
4. Inspect relevant archived source in `legacy/`. Port reusable logic into new root code; preserve unrelated edits and source history. Legacy behavior is evidence, not the new contract.
5. Implement stated failure branches/tests. Missing capability must block unsafe writes. Record actual tests and remaining review IDs.

```sh
python3 docs/cli/read.py OP-34
python3 docs/cli/read.py ALG-RESTORE
python3 docs/cli/read.py WP-11
python3 docs/cli/read.py --setting audio
python3 docs/cli/read.py --list operations
```

The reader prints one section or matching settings, followed by prerequisite paths. It does not recursively load dependencies. [reading-map.json](reading-map.json) is generated ID-to-file metadata; `validate.py` verifies it.

## Task routes

| Need | Entry |
| --- | --- |
| Finalized gaps/decisions | [Baseline](decisions/README.md) |
| Command logic | [Operations](operations/README.md) |
| Domain fields/records/states | [Contracts](contracts/README.md) |
| Extraction/generation | [Pipelines](pipelines/README.md) |
| Apply/recovery/restore/jobs | [Recovery](recovery/README.md) |
| Settings/defaults/import | [Configuration](configuration/README.md) |
| Next coding task | [Work packages](implementation/README.md) |
| Safety/evidence gaps | [Safety review](review/safety.md) |
| Conflicting earlier proposals | [Reconciliation](review/reconciliation.md) |
| End-to-end examples | [Workflows](examples/workflows.md) |
| Product/background only | [Application reference](reference/application/README.md) |
| Dated research only | [Research reference](reference/research/README.md) |
| Move history/checks | [Legacy relocation](reference/legacy-relocation.md) |

Priority: user instructions, finalized decisions, reconciled handbook, application reference, research, historical legacy documentation. Do not load every file or the whole registry by default. Large references are sectioned and optional. Read complete numbered algorithms when coding; safety rules must not be abbreviated away.

Actual collection actions still require session authorization. No delegation is required. Run `python3 docs/cli/validate.py` after documentation changes; consistency checks do not prove native migration or backup safety.

# Linguist Anki Bridge: persistent CLI application specification

Specification version: 1.2 reconciled handbook draft. Updated: 2026-09-26.
Code baseline: Archived under `legacy/`: Git `33c41fd` **plus the existing modified and untracked working-tree files** reviewed on that date. This is not a description of the committed release alone.

Current implementation design: [CLI research and implementation plan](../research/README.md), grounded in [read-only Anki inventory](../anki-inventory-2026-09-26.json). Read the researched plan alongside this document. Its selected v2 model/schema, four-workflow scope, card-preserving migration design, technology choices and delivery sequence supersede earlier alternative defaults below where explicitly identified. Nothing in these documents claims those changes are implemented.

Detailed operation logic, exhaustive settings and delivery packages: **[CLI implementation handbook](../../README.md)**. Its [reconciliation](../../review/reconciliation.md), [final v2 field contracts](../../contracts/README.md), [operation catalogue](../../operations/README.md) and [write/recovery algorithms](../../recovery/README.md) supersede less detailed or conflicting proposals below. Canonical commands are `vocab add|revamp` and `grammar add|revamp`. These are design documents, not implemented safety guarantees.


## Read a single reference section

- [1. Authority, interpretation, and maintenance](01-authority-interpretation-and-maintenance.md)
- [2. Product purpose and release scope](02-product-purpose-and-release-scope.md)
- [3. Current architecture and evidence map](03-current-architecture-and-evidence-map.md)
- [4. End-to-end workflow and dependency order](04-end-to-end-workflow-and-dependency-order.md)
- [5. Command surface and shell contract](05-command-surface-and-shell-contract.md)
- [6. Configuration, setup, and preflight](06-configuration-setup-and-preflight.md)
- [7. Selection, ingestion, and duplicate decisions](07-selection-ingestion-and-duplicate-decisions.md)
- [8. Enrichment pipeline details](08-enrichment-pipeline-details.md)
- [9. Canonical documents, templates, and durable review](09-canonical-documents-templates-and-durable-review.md)
- [10. Apply transaction and safety boundaries](10-apply-transaction-and-safety-boundaries.md)
- [11. Durable jobs, service pacing, and restart](11-durable-jobs-service-pacing-and-restart.md)
- [12. Snapshots, rollback, and backup](12-snapshots-rollback-and-backup.md)
- [13. Storage, privacy, and network policy](13-storage-privacy-and-network-policy.md)
- [14. Engineering and usability recommendations with references](14-engineering-and-usability-recommendations-with-references.md)
- [15. Implementation milestones and acceptance evidence](15-implementation-milestones-and-acceptance-evidence.md)
- [16. User review register](16-user-review-register.md)
- [17. Persistent implementation checklist](17-persistent-implementation-checklist.md)

Current implementation rules live in the handbook; references provide background and dated evidence.

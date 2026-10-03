# Preparation pipeline

## ALG-VALIDATE — readiness

1. Validate document version/types/lengths, known keys, supported languages/kind and FieldIntent applicability.
2. Resolve effective fields; detect loss of meaningful unmapped source content and unresolved media. Either preserve it in visible source/personal material or require a reviewed decision plus durable archive.
3. Check semantic correctness: nonempty expression/pattern, selected meaning/use, grounded reading/formation, meaningful examples, source/explanation language, and task-specific cues. Check answer leakage on question side, excessive ambiguity and multi-unit content.
4. Check sanitized HTML, media references, size/format limits, duplicate intent, fixed target field/template compatibility and mapped tasks. Novel generated grammar facts/ungrounded core facts need explicit content review. Apply capability failures are separate preflight blockers; absent bridge does not prevent offline content-ready export.
5. Collect error/review/warning issues with stable codes and field/stage links. Errors cannot be waived. Review issues need an explicit valid decision; warnings can be accepted in approval. Revalidate decisions against input fingerprints.
6. Content-ready requires no blocking content issues; grammar requires at least one validated use example, while vocabulary examples_min is a generation target rather than mandatory count. Apply additionally requires current capability/identity/checkpoint preflight. Approval binds resulting digest, including rendered fields/assets/model actions. Report each unresolved issue with a next command; exit with the documented status.

Current local plan validation also requires at least one document. It compares
batch items by target language, kind, expression/pattern, selected sense/use,
meaning and context (vocabulary also includes reading). A repeated identity
produces `DUPLICATE_BATCH_ITEM` on later items and blocks whole-plan content
readiness. Distinct homographs, readings, uses and contexts remain separate.
This only checks the saved batch. `plans duplicate-candidates PLAN --item-id UUID`
can now read candidate IDs and managed v2 fields for one rendered authored add
item. Its exact-primary-field Anki search is bounded by `selection.max_notes`,
but legacy models, HTML search behavior, context and concurrent changes mean
even matching returned fields do not prove semantic identity or absence of
other duplicates. The report always says collection duplicate checking is
incomplete and cannot authorize `selection.duplicate_policy=skip_exact`.
With `--record --digest D`, found candidates become a `COLLECTION_DUPLICATE_REVIEW` issue resolved per candidate note with `create_new` or `skip`. The decision is recorded intent only; an empty search records nothing, and complete native duplicate inspection plus enforcement of skip outcomes at apply remain pending.
Query escaping and exact-field behavior follow the [Anki search manual](https://docs.ankiweb.net/searching.html#limiting-to-a-field).

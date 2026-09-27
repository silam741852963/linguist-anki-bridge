# Research reference

## 3. Four workflows, one engine

| Workflow | Sources | Main stages | Result |
| --- | --- | --- | --- |
| Revamp vocabulary | Existing note fields, images and recordings | Source mapping → OCR/classification → expression/sense resolution → dictionary → annotations → preserve/enrich media → document | Reviewed existing-note migration/update, optionally reviewed splits. |
| Revamp grammar | Existing front/back text and screenshot groups | Source mapping → OCR/layout → pattern segmentation → structured evidence → grammar explanation/examples → review units/exercises | Existing-card anchor mapping plus approved new units. |
| Add vocabulary | Words/phrases, line input, CSV, user meaning/context | Exact duplicate decision → dictionary/author evidence → annotations/media → document | New note or explicit routing to existing-note revamp. |
| Add grammar | Pattern/context, text file, screenshot(s), article URL, CSV | Duplicate candidates → source extraction → pattern/unit proposal → supported explanation/examples/exercise → document | New grammar note(s) or reviewed existing-note revamp. |

The working CLI vocabulary is `revamp` and `add`; retain `modernize`/`ingest` as documented aliases if useful. The user should only need four visible steps: **prepare → review → apply → restore if needed**. Advanced plans/jobs/provider settings remain available without obscuring the core commands.

```bash
linguist-anki-bridge revamp vocab --deck "森の言葉" --language ja --limit 10
linguist-anki-bridge revamp grammar --deck "夕暮れの詞" --language ja --limit 10
linguist-anki-bridge add vocab --word "solicitation" --language en --deck "Moonlit Manuscripts"
linguist-anki-bridge add grammar --pattern "〜にして" --language ja --source-file grammar.txt
linguist-anki-bridge add grammar --image page.png --language ja
linguist-anki-bridge add grammar --url ARTICLE_URL --language ja
linguist-anki-bridge plans show PLAN_ID
linguist-anki-bridge plans edit PLAN_ID
linguist-anki-bridge apply PLAN_ID --apply
```

`add grammar` accepts explicit authored meaning/formation/examples as well as enrichment; a pattern alone is not permission to manufacture authoritative rules. Fetch article content as evidence through the configured provider, with downloaded content limits and source provenance. A query by grammar title alone is insufficient exact identity: use normalized pattern + language + reviewed use/formation key. Overlapping grammar uses produce candidates, not automatic fuzzy merges.

### 3.1 Pipeline ordering and extraction boundaries

1. Validate command/config/capabilities; resolve profile and purpose; prepare inputs/selector; capture immutable source manifest.
2. Fetch relevant fields and unique media once. Deduplicate image content shared between front/back; retain its field/position associations. The grammar sample has images on both sides, so “OCR Picture only” is insufficient.
3. Run OCR/layout with region ordering. Keep source Japanese/Vietnamese/English text and bounding regions. Grammar processing separates literal extraction from interpretation; vocabulary image decisions are per-image.
4. Resolve accepted expression or grammar learning units. Surface questionable OCR, lemma changes, homographs, sense ambiguity and one-to-many proposals.
5. Vocabulary retrieves authoritative dictionary evidence. Grammar retrieves configured reference/article evidence when provided; source OCR/text remains the traceable basis for extraction.
6. Generate bounded schema-constrained annotations or grammar objects. Vocabulary generation cannot write definitions/readings; grammar generation labels inferred rules and traces supporting source spans.
7. Concurrently enrich independent Kanji/image/audio stages, with one GPU generation request at a time initially. Grammar does not need a stock illustration; keep source screenshot on the back/reference.
8. Build typed LearningDocument v2; validate semantically and structurally; persist durable plan/assets; show changes and review-needed decisions.
9. Apply the exact reviewed revision with conflict guards, backup, mapped migration when needed, operation journal, snapshots and read-back verification.

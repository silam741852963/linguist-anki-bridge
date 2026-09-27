# Research reference

## 7. Evaluation and release criteria

Build a small stratified evaluation set after explicit fixture redaction or use synthetic equivalents: Japanese/English vocab, image-only grammar, dark screenshots, Vietnamese explanations, multiple patterns per image, repeated front/back media, homographs, malformed/missing audio, mixed note types and mature cards. Do not commit raw personal note content or screenshots as a convenient test corpus.

For OCR, compare critical-token extraction (patterns, negation, formation operators), region order, CER where ground truth is available, review-needed rate and latency. For generated content, score factual agreement with sources, meaning preservation, example grammar/naturalness, translation alignment, useful exercise answerability, schema compliance and repair rate. No arbitrary model-quality percentage is asserted before benchmarking.

Hard gates: no unauthorized Anki mutation in prepare/dry-run; no source/personal-context loss; no known card-task mapping error; unchanged retained card IDs/review history/scheduling in isolated migration tests; no new-note duplicate after tested unknown outcomes; complete before-media recovery; correct partial/conflict reports. Where Anki mutates derived bookkeeping, distinguish expected changes from unintended due/interval/history changes and verify the actual review log through Anki-supported test mechanisms.

Usability gates: four workflows readable from help; progress per stage; resumable commands in errors; clean JSON stdout; keyboard/shell-only review; no forced browser/editor/model download; previewed revision equals applied revision. These follow relevant [CLI design conventions](https://clig.dev/), with application-specific recovery guarantees tested separately.

Performance targets to measure rather than promise: bounded memory through 200-note pages; no unbounded base64 duplication in SQLite; no repeated OCR of identical source images; one backup per protected migration batch; one GPU job at a time initially; recover without rerunning completed enrichment. Report real end-to-end latency before tuning worker counts.

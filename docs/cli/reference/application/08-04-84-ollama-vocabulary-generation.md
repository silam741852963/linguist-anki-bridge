# Application specification reference

### 8.4 Ollama vocabulary generation

Input: accepted exact expression, source language, configured translation language, authoritative structured dictionary evidence, raw OCR with source markers, and user context. External text is data, not instructions controlling tools or collection writes.

Output ownership: `nuances` and `examples[{sentence, translation}]` only. Reject/ignore generated definition/readings/senses fields. Preserve all complete relevant OCR examples in their original target language; translate explanatory text into the configured translation language. Generate enough additional examples to reach at least three where possible, without truncating a larger recovered set. Record recovered versus generated example provenance.

Use structured schema requests, bounded normalization of known nested/aliased envelopes, and a limited repair retry. Invalid output must remain a visible issue; never silently accept arbitrary response text as card HTML. Escape generated text, preserve ordinary punctuation, and deduplicate only demonstrably equivalent examples. Record model identity, prompt/settings revision, and evidence hashes for reproducibility.

**REVIEW R19:** absent dictionary/LLM behavior and whether a dictionary-only card can be applied. Selected direction: distinguish optional warnings from blocking errors; a meaningful dictionary-only or explicitly author-defined card can be accepted, while empty or placeholder meanings block apply. Existing `issues` strings uniformly block readiness, and Python sometimes merely logs provider failures: replace that inconsistency with structured issue severity.

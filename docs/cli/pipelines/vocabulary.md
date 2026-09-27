# Preparation pipeline

## ALG-VOCAB — add or revamp vocabulary

1. Run ALG-CAPTURE; revamp requires a source note, add requires authored input. Resolve expression, target language, explanation language, context and requested tasks. Apply enabled normalization only to lookup/identity; retain source text.
2. Run ALG-OCR where images contain possible lexical evidence. A screenshot with several entries needs explicit expression/region selection. A missing expression is blocking.
3. Query language-appropriate dictionary; preserve entries, forms, readings, senses, labels, examples and provenance. Unsupported provider/language fails clearly. Dictionary outage can use a permitted fresh/cache policy; authored facts require provenance and review. Never turn “not found” into invented dictionary evidence.
4. Select reviewed sense(s). Conflicting readings, homographs and duplicate notes need a decision. Dictionary owns meaning/reading/pronunciation facts; generation cannot overwrite them.
5. After sense resolution, run requested enrichment only (llm.enabled=false uses source/dictionary/authored material). Independent branches use bounded concurrency: usage/examples generation, kanji data for applicable languages, image candidates and audio candidates. Pass source context, OCR regions and selected evidence to generation as untrusted data. Require schema output; no tool calls or shell output execution.
6. Preserve user notes and source media by default. Replacement candidates are staged locally with attribution, decoded type and hashes. Validate audio language/reading/voice; absent optional media is a warning unless selected task requires it.
7. Construct document with FieldIntent values; author ProductionPrompt/SpellingPrompt only when corresponding tasks are enabled. Retained tasks require usable cues. Do not remove existing cards to make validation pass.
8. Run ALG-VALIDATE and ALG-RENDER. Persist new immutable plan revision, issues, resource/settings fingerprints and dependency graph. Return ready or needs_review, never implicitly apply.

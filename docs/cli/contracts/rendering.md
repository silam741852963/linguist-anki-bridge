# Domain contracts

## ALG-RENDER — render and readiness rules

1. Resolve each FieldIntent against captured source mapping. Keep requires an existing source value; Set preserves explicit value; Clear is explicit empty output. Include all fixed target fields, even those intentionally empty.
2. Convert typed meanings/examples/formation/usage into a compact primary answer plus distinct reference section. Keep dictionary, OCR, generated and personal provenance distinguishable; never concatenate rich senses into an unlabelled guess.
3. Escape ordinary text; sanitize permitted HTML through a versioned allowlist. Block script/event handlers, executable URLs, remote embedded active content and arbitrary CSS. Render local hashed media and native sound tags through controlled helpers, not model-provided HTML.
4. Generate enabled task fronts. Vocabulary v3 fronts show fields (Picture, Meaning, Audio, Pronunciation); grammar fronts use reviewed RecognitionPrompt and exercise cues. Validate prerequisites and leakage before flags become enabled. Existing retained tasks lacking a usable front remain a review issue.
5. Write exact final fields in the fixed model order. Vocabulary v3 carries target/explanation language, kind, tasks and dictionary facts as `lab::` tags; sense identity stays in the plan document. User source language remains archived even when explanations are translated.
6. Derive expected card task set from the same front gating rules used by installed templates. Add per-card deck map and media-reference manifest. A terminal preview is not proof of Anki card generation; disposable template tests must establish parity.
7. Hash the complete rendered intent, task/model/deck/media manifests and render resource versions into plan digest. Diff and apply consume these persisted values; neither re-renders using a changed live config.
8. Produce plain terminal view and optional safe HTML export. Never execute arbitrary template JavaScript for preview. Return semantic issues to ALG-VALIDATE; valid HTML alone does not make a card ready.

- Comprehension requires Expression + meaningful selected Meaning. Primary front does not show answer/translation.
- Production (v3) requires enabled flag + Expression + meaningful Meaning. Its front shows Picture and the Meaning field. Meaning is the selected entry's sense list with the selection marked by the `lab-selected` class; it never shows provider names, URLs, sense keys, readings, examples or headwords, and every occurrence of the expression is masked as `〜`.
- Spelling (v3) requires enabled flag + Expression + a Pronunciation (or Reading) or a selected audio (`SPELLING_CUE_MISSING`). A Pronunciation (or Reading) equal to the written form (a kana word such as いじめ) is not rendered, because it is the Spelling answer; such a Spelling card then needs a selected audio (WP-20).
- Recognition requires Pattern + meaningful Meaning + approved/derived RecognitionPrompt whose scope matches the use. Formation/examples live after reveal.
- Application requires enabled flag + unambiguous ExercisePrompt + ExerciseAnswer. Regular card; no native cloze markup in a regular model.
- Retained task missing prerequisites: block migration until a retained/edited valid cue is approved. Disabling its flag does not authorize deleting/suspending its mature card. V1 has no automatic card deletion/suspension operation.
- `Keep`/`Clear` on a managed migration is resolved against original/archive + explicit output schema; unrelated fields missing from output cannot be assumed discarded with consent.
- Language tags validated as BCP-47; template attributes escape values; arbitrary Source/PersonalNotes are sanitized content, not executable HTML.
- Vocabulary v3 UsageExamples renders, in this fixed order and only when present: Usage paragraph, Nuance (near-synonym → difference), Collocations (phrase + gloss), Examples (sentence with the expression highlighted + translation). Kanji renders one block per `kanji_details` entry: the stroke-order GIF (media role `kanji_stroke`) or the character, meanings, ON/KUN readings, strokes, radical, parts and JLPT. The legacy plain `kanji` text is not rendered.

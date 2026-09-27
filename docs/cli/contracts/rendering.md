# Domain contracts

## ALG-RENDER — render and readiness rules

1. Resolve each FieldIntent against captured source mapping. Keep requires an existing source value; Set preserves explicit value; Clear is explicit empty output. Include all fixed target fields, even those intentionally empty.
2. Convert typed meanings/examples/formation/usage into a compact primary answer plus distinct reference section. Keep dictionary, OCR, generated and personal provenance distinguishable; never concatenate rich senses into an unlabelled guess.
3. Escape ordinary text; sanitize permitted HTML through a versioned allowlist. Block script/event handlers, executable URLs, remote embedded active content and arbitrary CSS. Render local hashed media and native sound tags through controlled helpers, not model-provided HTML.
4. Generate enabled task fronts from reviewed prompts and answers. Validate ProductionPrompt/SpellingPrompt/RecognitionPrompt/exercise prerequisites and leakage before flags become enabled. Existing retained tasks lacking useful cues remain a review issue.
5. Write exact final fields in the fixed model order; include target/explanation language tags and stable sense/use identity. User source language remains archived even when explanations are translated.
6. Derive expected card task set from the same front gating rules used by installed templates. Add per-card deck map and media-reference manifest. A terminal preview is not proof of Anki card generation; disposable template tests must establish parity.
7. Hash the complete rendered intent, task/model/deck/media manifests and render resource versions into plan digest. Diff and apply consume these persisted values; neither re-renders using a changed live config.
8. Produce plain terminal view and optional safe HTML export. Never execute arbitrary template JavaScript for preview. Return semantic issues to ALG-VALIDATE; valid HTML alone does not make a card ready.

- Comprehension requires Expression + meaningful selected Meaning. Primary front does not show answer/translation.
- Production requires enabled flag + meaningful approved ProductionPrompt (picture/context) + Expression; ambiguous image-only prompts need reviewed disambiguation. It does not use the entire Meaning reference blob as a convenient prompt.
- Spelling requires enabled flag + approved SpellingPrompt/audio cue + Expression; cue must not reveal exact spelling. Japanese reading may reveal a kana answer, so validate cue suitability for that expression rather than assume any reading is safe.
- Recognition requires Pattern + meaningful Meaning + approved/derived RecognitionPrompt whose scope matches the use. Formation/examples live after reveal.
- Application requires enabled flag + unambiguous ExercisePrompt + ExerciseAnswer. Regular card; no native cloze markup in a regular model.
- Retained task missing prerequisites: block migration until a retained/edited valid cue is approved. Disabling its flag does not authorize deleting/suspending its mature card. V1 has no automatic card deletion/suspension operation.
- `Keep`/`Clear` on a managed migration is resolved against original/archive + explicit output schema; unrelated fields missing from output cannot be assumed discarded with consent.
- Language tags validated as BCP-47; template attributes escape values; arbitrary Source/PersonalNotes are sanitized content, not executable HTML.

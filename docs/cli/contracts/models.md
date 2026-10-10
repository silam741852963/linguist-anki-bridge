# Domain contracts

## Final Anki model fields

This is the authoritative fixed order. Fields added to close cue/language/grammar identity gaps supersede shorter research tables. Models are new versioned families; existing v1 models are not silently reshaped.

`Linguist Vocabulary v3` fields in order (WP-19; replaces v2 for new writes):

1. Expression
2. Pronunciation
3. Meaning
4. UsageExamples
5. Picture
6. Audio
7. Kanji
8. EnableProduction
9. EnableSpelling

Templates: Comprehension ordinal 0; Production ordinal 1; Spelling ordinal 2. Fronts show fields, not text cues: Comprehension shows Expression; Production shows Picture and Meaning; Spelling shows Audio, Pronunciation, Meaning and a type-in box. Expression appears on task fronts only on the back. Add defaults: only Comprehension. Revamp: preserve existing semantic tasks, confirmed enabled flags and per-card deck membership. Explicit mapping old 0→0, 1→1, 2→2 applies to verified Picture Words only; arbitrary/v1 models use their own observed task map.

Mapping from older schemas: Reading and Pronunciation become Pronunciation (Pronunciation wins when both are set). Usage and Examples become UsageExamples. SenseKey stays in the document as identity and becomes a hidden selection, not a field. Language and ExplanationLanguage become tags. PersonalNotes and Source stay archived in the plan and source snapshot; they are not rendered. ProductionPrompt and SpellingPrompt are kept in older documents but never rendered.

Tags (`linguist_core::render::tags`): `lab::lang::<target>`, `lab::explain::<explanation>`, `lab::kind::vocabulary|grammar`, `lab::task::<task>` per requested task, `lab::jlpt::<level>`, `lab::common` and `lab::pos::<label>` from the selected dictionary entry, and `lab::has::kanji|picture|audio`.

`Linguist English Vocabulary v1` fields in order (English targets): Expression, Pronunciation (IPA), Meaning, UsageExamples, Picture, Audio, EnableProduction, EnableSpelling. Same templates and CSS as v3 without the Kanji section; kanji does not apply to English. `audio.provider=dictionary` takes the General American IPA and an en-US recording from the word's Wiktionary page.

UsageExamples also lists the selected dictionary entry's neighbours: other spellings ("Also written"), the other entries of the lookup ("Related words", form, reading and first gloss) and its cross-references ("See also"). They appear on the back only.

`Linguist Vocabulary v2` (18 fields: Expression, Reading, Pronunciation, Meaning, Usage, Examples, Picture, Audio, Kanji, PersonalNotes, Source, Language, SenseKey, EnableProduction, EnableSpelling, ProductionPrompt, SpellingPrompt, ExplanationLanguage) is no longer installed. The companion still accepts it so existing v2 notes can be restored or revamped to v3.

`Linguist Grammar v4` fields in order (WP-23; replaces v3 for new writes):

1. Pattern
2. Meaning
3. Formation
4. Example
5. Usage
6. Nuance

Template: Recognition ordinal 0 only, with the vocabulary v3 CSS plus `grammar-v4.css`. There is no Application card and no exercise. The front shows the Pattern only: an example there would bring its sound, because Anki plays every sound of a side, even one hidden with CSS. Meaning holds the gloss in the explanation language only; the source's own meaning line (`source_meaning`) stays in the document and the source archive. Example lists every example: its sentence with the pattern highlighted, its translation, and the `[sound:]` play button of each selected reading of that sentence. Usage and Nuance (contrasts with similar patterns, a source's `[Chú ý]` notes) have their own fields. Highlighting uses the document's reviewed `forms` together with forms derived from the pattern (〜, word-class slots and `+` removed, alternatives split, optional parts in parentheses expanded); the longest match wins.

Audio: enrichment synthesizes one reading per example (VOICEVOX for Japanese). Each reading is its own audio candidate with its own `AUDIO_CANDIDATE_REVIEW` (id `AUDIO_CANDIDATE_REVIEW:<document>:<digest>`); a selected reading renders next to the example whose sentence it reads (the `text` of its enrichment evidence). A reading of a sentence that is no longer an example is not rendered or sent. Anki plays the back's sounds in order unless the deck's options turn off automatic audio.

Mapping from v3: UsageExamples splits into Example (all examples), Usage and Nuance; the Vietnamese meaning line is dropped from Meaning; ExercisePrompt, ExerciseAnswer, EnableApplication and Audio are gone (audio sits in Example). Recognition ordinal 0 maps to Recognition 0. A v3 note with an Application card cannot be migrated while that card exists (`APPLY_MIGRATION_DROPS_CARD`).

`Linguist Grammar v3` (WP-22: Pattern, Meaning, Formation, Example, UsageExamples, ExercisePrompt, ExerciseAnswer, Audio, EnableApplication; Recognition and Application) is no longer installed for new notes. The companion still accepts it so existing v3 notes can be restored.

`Linguist Grammar v2` (15 fields: Pattern, Meaning, Formation, Usage, Examples, ExercisePrompt, ExerciseAnswer, Audio, PersonalNotes, Source, Language, EnableApplication, UseKey, RecognitionPrompt, ExplanationLanguage) is no longer installed. The companion still accepts it so existing v2 notes can be restored or revamped to v3.

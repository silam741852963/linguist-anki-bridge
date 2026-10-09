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

`Linguist Grammar v3` fields in order (WP-22; replaces v2 for new writes):

1. Pattern
2. Meaning
3. Formation
4. Example
5. UsageExamples
6. ExercisePrompt
7. ExerciseAnswer
8. Audio
9. EnableApplication

Templates: Recognition ordinal 0; Application ordinal 1, with the vocabulary v3 CSS plus `grammar-v3.css`. Fronts show fields, not text cues: Recognition shows Pattern and Example (the document's first example with the pattern highlighted); Application shows ExercisePrompt and needs ExerciseAnswer. Meaning is never on a front. Meaning holds the meaning in the explanation language and, under it, the source's own meaning line (`source_meaning`, verbatim, for example the Vietnamese gloss of a textbook page). UsageExamples holds usage, nuance against similar patterns (a source's `[Chú ý]` contrasts) and every example with the pattern highlighted. Highlighting uses the document's reviewed `forms` together with forms derived from the pattern (〜, word-class slots and `+` removed, alternatives split, optional parts in parentheses expanded); the longest match wins. Audio is a synthesized reading of the Example sentence (VOICEVOX for Japanese). Add defaults: Recognition only. Verified Basic grammar ordinal 0→Recognition 0. Application prerequisites: approved ExercisePrompt/ExerciseAnswer. A multi-pattern source explicitly selects the anchor carrying its old recognition history.

Mapping from v2: Usage and Examples become UsageExamples (and the first example becomes Example). UseKey stays in the document as identity. RecognitionPrompt is no longer required or rendered. Language and ExplanationLanguage become tags; the JLPT level and lesson become `lab::jlpt::<level>` and `lab::lesson::<lesson>`. PersonalNotes and Source stay archived.

`Linguist Grammar v2` (15 fields: Pattern, Meaning, Formation, Usage, Examples, ExercisePrompt, ExerciseAnswer, Audio, PersonalNotes, Source, Language, EnableApplication, UseKey, RecognitionPrompt, ExplanationLanguage) is no longer installed. The companion still accepts it so existing v2 notes can be restored or revamped to v3.

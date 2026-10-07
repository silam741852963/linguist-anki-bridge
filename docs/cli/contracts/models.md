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

`Linguist Vocabulary v2` (18 fields: Expression, Reading, Pronunciation, Meaning, Usage, Examples, Picture, Audio, Kanji, PersonalNotes, Source, Language, SenseKey, EnableProduction, EnableSpelling, ProductionPrompt, SpellingPrompt, ExplanationLanguage) is no longer installed. The companion still accepts it so existing v2 notes can be restored or revamped to v3.

`Linguist Grammar v2` fields in order:

1. Pattern
2. Meaning
3. Formation
4. Usage
5. Examples
6. ExercisePrompt
7. ExerciseAnswer
8. Audio
9. PersonalNotes
10. Source
11. Language
12. EnableApplication
13. UseKey
14. RecognitionPrompt
15. ExplanationLanguage

Templates: Recognition ordinal 0; Application ordinal 1. Add defaults: Recognition only. Verified Basic grammar ordinal 0→Recognition 0. Application prerequisites: approved ExercisePrompt/ExerciseAnswer. A multi-pattern source explicitly selects the anchor carrying its old recognition history.

# Domain contracts

## Final Anki v2 model fields

This is the authoritative fixed order. Fields added to close cue/language/grammar identity gaps supersede shorter research tables. Models are new versioned families; existing v1 models are not silently reshaped.

`Linguist Vocabulary v2` fields in order:

1. Expression
2. Reading
3. Pronunciation
4. Meaning
5. Usage
6. Examples
7. Picture
8. Audio
9. Kanji
10. PersonalNotes
11. Source
12. Language
13. SenseKey
14. EnableProduction
15. EnableSpelling
16. ProductionPrompt
17. SpellingPrompt
18. ExplanationLanguage

Templates: Comprehension ordinal 0; Production ordinal 1; Spelling ordinal 2. Add defaults: only Comprehension. Revamp: preserve existing semantic tasks, confirmed enabled flags and per-card deck membership. Explicit mapping old 0→0, 1→1, 2→2 applies to verified Picture Words only; arbitrary/v1 models use their own observed task map.

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

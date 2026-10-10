# Research reference

### 2.2 Grammar standard

One unit represents a grammar pattern/use with a recognizable formation and a focused recall objective. A screenshot can produce zero, one or several proposed units. Different uses of the same pattern can remain supporting reference or become separate units after review. Keep related contrasts/context together where splitting would lose meaning.

**Target regular note type:** `Linguist Grammar v2`, with fixed field order:

| Field | Purpose |
| --- | --- |
| Pattern | Canonical grammar pattern; preserve script/operators/negation. |
| Meaning | Concise function/use of this learning unit. |
| Formation | Connection rules and valid grammatical forms. |
| Usage | Restrictions, register, contrasts and caveats. |
| Examples | Target-language sentences and configured translations, with source/generated labels. |
| ExercisePrompt | One approved contextual gap/reformulation cue for application practice. |
| ExerciseAnswer | Expected form plus acceptable alternatives/explanation. |
| Audio | Optional example pronunciation; not mandatory for grammar. |
| PersonalNotes | User explanations/annotations; preserve original language. |
| Source | Original excerpt/image/article provenance and attribution. |
| Language | Target language. |
| EnableApplication | Optional contextual application card. |
| UseKey | Reviewed use identity, distinct from pattern alone. |
| RecognitionPrompt | Focused recognition cue for this use. |
| ExplanationLanguage | Explanation/translation BCP-47 language. |

Templates: **Recognition (0)** and **Application (1)**. Recognition tests the pattern's specific use and reveals explanation/formation/examples. Application presents the reviewed gap/cue and asks for the target form, revealing the complete sentence and explanation. It is a regular card with an explicit gap, not native `{{cloze:...}}` inside a regular note type. A separate native Cloze model can be added later without changing this migration contract; Anki distinguishes [Cloze generation from regular templates](https://docs.ankiweb.net/templates/generation.html).

Default new grammar notes generate Recognition, and Application only when an unambiguous exercise has been validated and opted in. Existing Basic grammar card ordinal 0 maps to Recognition, preserving its history where native migration supports it. One source note with several patterns must undergo reviewed expansion: original card history goes only to the explicitly chosen anchor unit; siblings start as new cards. Never clone maturity/review history onto siblings. (Superseded in part by WP-23: at the user's request a sibling copies its source card's schedule, still never its review history.)

# Research reference

### 1.1 What the legacy cards actually require

Picture Words fields: `Word`, `Picture`, `Gender, Personal Connection, Extra Info (Back side)`, `Pronunciation (Recording and/or IPA)`, `Test Spelling? (y = yes, blank = no)`.

Legacy card ordinals are **0 Comprehension, 1 Production, 2 Spelling**. The previous proposed managed order was Comprehension, Spelling, Production. Migration cannot simply match ordinals: it could associate a mature Production card's schedule with a Spelling task. Either retain the legacy order in the new model or explicitly map old card ordinals to the equivalent new task. This plan does both: matching order plus explicit mapping verification.

Sampled cards have substantial existing review history and multi-year intervals. That establishes preservation as important; the small card sample does **not** establish whole-collection scheduler settings or FSRS status. Do not alter scheduling parameters or enable FSRS automatically. Anki manages scheduling, including its current [FSRS integration](https://docs.ankiweb.net/deck-options).

Image presence is nearly universal. Many existing cards have useful pronunciation audio/IPA but little textual meaning. New generation should recover and preserve those assets, not replace every image/recording with downloaded alternatives.

There are 85 English-deck and 110 Japanese-deck expression fields with markup/newline split indicators. These **195 candidates are not 195 proven multi-expression notes**: formatting breaks can represent readings or layout. Treat them as a review queue, not an automatic split batch.

The grammar samples are image-only front/back material. One screenshot contains five numbered grammar patterns; another contains one pattern, formation rules and several Vietnamese explanations. Therefore expression-line splitting cannot solve grammar modernization. Grammar must segment screenshot content, preserve original Japanese forms and Vietnamese evidence, identify potentially distinct learning units, and ask for review where scope/meaning is ambiguous.

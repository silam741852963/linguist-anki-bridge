# Application specification reference

### 3.2 Grounding from the live collection

Read-only inspection found 18,975 notes and 55,761 cards across the observed decks. The collection has 18,421 legacy Picture Words notes and 554 Vietnamese-localized Basic notes. The working language/kind interpretation, supported by bounded samples, is English vocabulary in Moonlit Manuscripts, Japanese vocabulary in 森の言葉, and Japanese grammar in 夕暮れの詞. Deck membership alone does not classify every note: ten Basic notes also exist in the vocabulary deck.

The legacy vocabulary tasks are ordinal 0 Comprehension, 1 Production, 2 Spelling; preserve/map those tasks explicitly. The installed managed Japanese model is empty and still has only Japanese Recognition. Grammar sample images contain Japanese patterns with Vietnamese explanation; one source image contains five patterns. Grammar extraction must read front/back screenshot groups and support reviewed learning-unit segmentation.

Live AnkiConnect lacks `createBackup`. Its installed custom `updateNoteModel` directly changes the note model/fields without an explicit native card-template mapping operation. Therefore the current Rust backup assumption and reflection-only migration guard are insufficient for this collection. Require the researched plan's verified backup path and tested native mapped migration adapter. Inspection did not modify Anki or test these writes.

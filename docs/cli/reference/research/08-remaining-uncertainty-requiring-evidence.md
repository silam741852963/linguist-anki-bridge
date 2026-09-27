# Research reference

## 8. Remaining uncertainty requiring evidence

- Exact native Anki/AnkiConnect extension registration and migration/undo semantics on the installed `anki-git` build. This blocks model migration, not specification or preparation.
- Whether each of the ten Basic notes in the Japanese vocabulary deck is vocab, grammar or another kind.
- Which of the 195 linebreak candidates actually needs splitting, and how many grammar screenshots contain several units.
- Actual media validity, image class/relevance, useful dictionary sense choice, and source permissions. Inventory counts do not answer these.
- Best installed local model/OCR strategy on this corpus and hardware; no generation benchmarks were run in this planning task.
- User preference for English versus Vietnamese supporting explanations and opt-in Production/Spelling/Application cards. Defaults above are concrete proposals and can be changed in review.

These are bounded review/validation items. All other routine implementation decisions above have a selected direction; work need not stop to request 34 separate approvals.

Final disposition: integration/model/language policies are settled in [decisions](../../decisions/README.md). Native compatibility and model quality are named implementation gates. Mixed note kinds, linebreak splitting, sense/media/source interpretation become per-item runtime review issues rather than architecture blockers.

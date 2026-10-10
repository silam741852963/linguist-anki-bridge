# Preparation pipeline

## ALG-GRAMMAR — add or revamp grammar

1. Capture source front/back fields and screenshots together. Resolve target and explanation language separately. Authored structured grammar can skip OCR.
2. OCR all relevant regions in reading order (screenshots need `images.existing_policy=inspect`; `ocr.page_segmentation_mode=6` reads single-column web and textbook pages best). Extract candidate patterns, formations, explanations, constraints and examples with source spans. A screenshot listing five patterns produces five candidates, not one guessed combined rule.
3. Review ambiguous segmentation. Each learning unit represents one pattern/use with UseKey. Keep alternate uses separate where recall answers differ. Mark incomplete source claims rather than treating model extrapolation as observed evidence.
4. For revamp with several units, choose exactly one source-history anchor through an explicit review decision. Other units are new notes/cards. Never duplicate mature scheduling across children. Preserve archive references on every child.
5. Ground formations and restrictions in supplied source or retrieved approved evidence. Generate concise explanation, minimal contrasting examples and optional application exercise. Record generated claims; uncertainty about correctness is blocking review.
6. The Recognition front shows the pattern only; there is no Application card (WP-23). The back lists every example with its own synthesized reading (one reviewed audio candidate per example), then usage and nuance in their own fields. Generation (`builtin:grammar-v3`) supplies meaning, formation for every word class, usage, up to 3 nuance contrasts, highlight forms, the requested number of examples and a JLPT level; every fact is reviewed and wrong ones are rejected (`content_rejected`) and generated again.
7. Validate all child units and task maps. Build one grouped plan with independently journaled children and a reviewed anchor; ALG-SPLIT handles eventual write ordering.
8. Render and persist revision exactly as for vocabulary. Do not treat valid JSON as proof of pedagogical accuracy.

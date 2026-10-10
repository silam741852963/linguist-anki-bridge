# Application specification reference

### 8.8 Grammar extraction, modernization and authoring

Grammar is a first-release workflow, not a vocabulary pipeline with a different prompt. Existing Basic notes can have no expression text and can store source screenshots on both front and back. Deduplicate identical image content while retaining field/position associations; OCR/layout precedes all grammar interpretation. Recognize headings, pattern notation, formation, explanations, examples and distinct uses as structured source evidence.

One screenshot can propose several learning units. Preserve original pattern operators, negation, Japanese examples and Vietnamese/source-language explanations. Separate literal extraction from generated explanation/translation, with source-span/region provenance and unresolved OCR marked for review. Preserve a source archive even when it is replaced by concise text in the main recall task.

`add grammar` accepts a pattern with authored context, text/CSV, local screenshots or an article URL. URL input is captured as source evidence; a pattern with insufficient evidence may produce a review-needed draft rather than fabricated authoritative rules. Duplicate candidates use normalized language/pattern plus the reviewed use/formation key; overlapping patterns are not automatically merged.

Each resulting grammar unit has a concise function, formation/connection rules, usage restrictions, source-linked examples/translations and optional approved contextual application exercise. New grammar notes use the researched plan's Grammar v2 Recognition and optional Application templates. Native `{{cloze:...}}` is not inserted into a regular note type. If a separate Cloze model is added later, give it its own generation/migration contract.

Review one-to-many expansion before apply. A source note's existing recognition history maps only to an explicitly selected anchor unit; sibling units are new notes/cards. Grammar cannot be auto-split using expression linebreak heuristics or inherit mature history on every child. (WP-23, user decision: each sibling copies its source card's schedule, not its review history.) Missing decorative artwork/audio is optional; unverified pattern/formation, conflicting source/generated meaning and unexplained source loss block apply.

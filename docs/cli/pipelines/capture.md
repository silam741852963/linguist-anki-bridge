# Preparation pipeline

## ALG-CAPTURE — resolve inputs and preserve evidence

1. Resolve validated settings with ALG-CONFIG. Require exactly one input mode: existing note selector, inline input, UTF-8 file, or structured document. A file can contain multiple explicit records; never infer separate records from arbitrary line breaks inside a record.
2. For selectors, compile Anki query with proper quoting, fetch IDs once, sort by configured order, apply limit, and persist that exact selection. Explicit IDs must all exist. Empty results return success with zero items; malformed queries fail. Deck membership is evidence, not a language/kind classifier.
3. Fetch every selected note's fields, model, templates, tags and cards/decks. Reject unsupported cloze/ambiguous field maps with a review issue. Capture all fields, including unmapped fields, without HTML normalization.
4. Parse media references as HTML/media syntax, not substring replacement. Retrieve referenced bytes through the Anki adapter; validate decoded size/type. Missing bytes produce an issue and never a fabricated replacement. Preserve source filenames, hashes and associations in the archive.
5. Detect source language, explanation language and kind from explicit overrides, reviewed mappings and evidence, in that precedence. Conflicting evidence needs review. Vietnamese explanations do not make Japanese grammar Vietnamese.
6. Locate duplicate candidates by semantic identity within configured scope. Exact existing note IDs remain authoritative. Report ambiguous/same-expression different-sense candidates. Never silently choose the first result or update an existing note from an add command.
7. Save capture and archive atomically. Failure leaves an inspectable failed item; original Anki data remains untouched.

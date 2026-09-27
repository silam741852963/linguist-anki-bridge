# Preparation pipeline

## Dependency invalidation

| Change | Recompute/invalidate |
| --- | --- |
| Source selection/fields/images | Capture-dependent evidence, documents, all decisions and approval |
| Expression/language/sense | Dictionary, generation, kanji, audio, relevant image search, rendering and approval |
| OCR/transcription/segmentation | Dependent lexical/grammar extraction and generation; unaffected media kept |
| Context/explanation language/prompt/model | Dependent generation and cues; dictionary facts retained |
| Voice/audio text | Audio and rendering only |
| Chosen picture | Picture validation/rendering only |
| User notes/task flags/field edits | Semantic validation/rendering/approval; do not overwrite edits with regeneration |
| Template/deck/model mapping | Compatibility, migration plan, rendering and approval |
| Retry/log/output settings | Execution envelope only; changing semantic frozen settings requires a new revision |

Regeneration always creates a new revision with explicit changed fields. A resumed approved job uses staged outputs; it never calls generation to recreate them.

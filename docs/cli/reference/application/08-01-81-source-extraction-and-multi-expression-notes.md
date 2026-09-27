# Application specification reference

### 8.1 Source extraction and multi-expression notes

Fetch current fields/model/tags/deck information and source media once per note where possible. Honor configured expression/picture/audio/context mappings; fallback candidates must be visible in evidence. Distinguish missing media from failed transport. Preserve original fields in full before conversion; do not silently drop personal notes or unrelated legacy fields.

Python detects multiple deliberate expression lines from line breaks/block boundaries, not inline spans or sound-tag count. It prepares each expression independently, keeps the original note ID for the first, and creates sibling notes for others. OCR is reused; visual images may be shared. Legacy audio matches by expression in filename, then by order if counts match. A single expression keeps all pronunciation tracks.

**SELECTED CLI default:** show a split proposal requiring an explicit reviewed split policy. Review cards/scheduling implications, sibling tags, audio assignment, and content ownership. **REVIEW R15**. If implemented, journal every created sibling ID immediately, use one parent recovery group, and reconcile partial creation after a crash. Filename/order audio matching is evidence, not guaranteed correctness.

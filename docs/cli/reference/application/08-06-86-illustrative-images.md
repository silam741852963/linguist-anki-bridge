# Application specification reference

### 8.6 Illustrative images

Retrieve internet images; do not synthesize pictures with AI. Search expression and accepted dictionary meaning with a configurable suffix. Current Python and Rust use Wikipedia/Commons paths, with different query/ranking policies. Record candidate source, query, source URL, content digest, and rejection reasons. Allow selecting a candidate or supplying a local image through plan edits.

Validate actual decoded content, download size, pixel dimensions, permitted formats, and quality. Current Rust limits include 8 MiB downloads and 24 million pixels; downloaded artwork should normalize alpha/CMYK/etc. to baseline RGB JPEG with stable naming. Reject error pages, invalid images, nearly black/low-information candidates, and unsupported SVG rather than mislabeling them as JPEG. Numeric quality filters do not prove semantic relevance.

Prefer original mnemonic artwork over automatic replacement. Failure to find an illustration is a warning unless the selected template requires a meaningful picture. **REVIEW R21:** image relevance review, source/license attribution retention, and minimum quality policy. Recommended: expose a candidate list and provenance, preserve original by default.

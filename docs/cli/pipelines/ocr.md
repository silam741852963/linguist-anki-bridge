# Preparation pipeline

## ALG-OCR — extract before generation

1. Enumerate source images in field/reading order; preserve originals. Decode safely under byte/pixel limits. Unsupported types fail that asset, not the whole process without an item issue.
2. Apply configured image policy: preserve, inspect, replace after review, or explicitly omit the *rendered reference*. Omission never deletes archived bytes.
3. For inspection, run configured OCR on temporary derivatives. Keep source image hash, region coordinates, engine/version, language pack, preprocessing and confidence with the text. Missing language packs produce actionable errors; do not download implicitly.
4. Classify illustration/dictionary/grammar/mixed/uncertain using configured evidence and confidence threshold. Vision is an optional second opinion. OCR confidence is engine-specific, not a calibrated truth probability.
5. Low confidence, mixed content or conflicting classification produces a review issue with image/regions/raw OCR. Preserve the picture until reviewed. Manual transcription can replace evidence text while retaining provenance.
6. Finish required OCR and reviewed segmentation before any dependent generation. Cache by image hash + engine/resource/settings fingerprint.

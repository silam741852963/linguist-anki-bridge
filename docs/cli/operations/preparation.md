# Preparation operations

Read [shared command rules](README.md) before implementing any handler.

## OP-21 — `vocab add`

Inputs: Purpose, input/selector, --set overrides; optional --output.

Effects: Anki/provider reads; local immutable plan/assets.

1. Validate add input mode with ALG-CAPTURE; add cannot silently become existing-note update.
2. Run ALG-VOCAB and its OCR/provider dependencies with frozen settings.
3. Persist archive/evidence/rendered outputs/issues in immutable revision.
4. Report readiness, review issue IDs and exact next commands; never applies even if legacy dry_run=false.

Result/failure: Plan ID/revision/digest; needs_review has actionable exit, ready is not committed. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current structured add mode (OP-21/OP-23): `--document FILE|-` defaults to one strict v2 JSON object. `--format jsonl` explicitly frames one complete v2 object per physical line for either kind. Blank lines, malformed/incorrect-kind rows and an empty batch fail with a line number before local state opens. The whole file is bounded by `input.max_file_mb`, each line by `input.max_record_chars`, and rows by `selection.max_notes`. Every original line, including its terminator, is preserved as a source field and content-addressed asset. All rows are parsed, enriched and validated before one ordered immutable plan is published; an invalid content item remains reviewable, whereas malformed framing/schema aborts the batch. Output has shared plan identity and ordered `items`; ready requires every item to pass effective validation. Batch-local exact semantic duplicates are reported as review issues. Collection duplicate checking and native writes remain unavailable.

Current read-only candidate command: `plans duplicate-candidates PLAN --item-id UUID [--revision N]` requires a rendered authored add item. It searches the matching managed v2 note type by its primary field using Anki field search, rejects oversized queries or candidate sets, fetches returned notes with profile checks, and compares selected managed field values. The output contains note IDs and match flags, while `collection_duplicate_check_complete`, `semantic_identity_verified` and `apply_eligible` remain false. Older note types and concurrent changes are outside this search. No empty result or matching candidate authorizes an automatic skip.

## OP-22 — `vocab revamp`

Inputs: Purpose, input/selector, --set overrides; optional --output.

Effects: Anki/provider reads; local immutable plan/assets.

1. Validate revamp input mode with ALG-CAPTURE; add cannot silently become existing-note update.
2. Run ALG-VOCAB and its OCR/provider dependencies with frozen settings.
3. Persist archive/evidence/rendered outputs/issues in immutable revision.
4. Report readiness, review issue IDs and exact next commands; never applies even if legacy dry_run=false.

Result/failure: Plan ID/revision/digest; needs_review has actionable exit, ready is not committed. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-23 — `grammar add`

Inputs: Purpose, input/selector, --set overrides; optional --output.

Effects: Anki/provider reads; local immutable plan/assets.

1. Validate add input mode with ALG-CAPTURE; add cannot silently become existing-note update.
2. Run ALG-GRAMMAR and its OCR/provider dependencies with frozen settings.
3. Persist archive/evidence/rendered outputs/issues in immutable revision.
4. Report readiness, review issue IDs and exact next commands; never applies even if legacy dry_run=false.

Result/failure: Plan ID/revision/digest; needs_review has actionable exit, ready is not committed. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-24 — `grammar revamp`

Inputs: Purpose, input/selector, --set overrides; optional --output.

Effects: Anki/provider reads; local immutable plan/assets.

1. Validate revamp input mode with ALG-CAPTURE; add cannot silently become existing-note update.
2. Run ALG-GRAMMAR and its OCR/provider dependencies with frozen settings.
3. Persist archive/evidence/rendered outputs/issues in immutable revision.
4. Report readiness, review issue IDs and exact next commands; never applies even if legacy dry_run=false.

Result/failure: Plan ID/revision/digest; needs_review has actionable exit, ready is not committed. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current revamp implementation (OP-22/OP-24): repeat explicit `--note-id` for multiple existing notes, with a required matching global `--purpose`. Validate IDs, reject duplicate IDs/selection counts above `selection.max_notes`, and freeze order using `selection.order=note_id` (numeric ascending) or `input` (argument order) before repeated read-only note/model/card capture. The aggregate raw capture assets must fit `input.max_file_mb`; exceedance fails without truncation or local plan creation. Validate every capture before creating local state, publish all assets first, then one immutable revision containing all documents in frozen order. A failed capture rejects this batch; partial failed-item publication remains pending. Single-note output retains `result`; multi-note output uses `result.items` and `item_count`, with the same plan ID/revision/digest on every item. Select `llm.enabled=false`, `dictionary.provider=authored`, disable image search and select preserve/disabled audio; Japanese vocabulary also requires `kanji.enabled=false` until that enrichment is connected. Requested unavailable enrichment fails rather than being skipped. Output labels `preparation_stage=source_draft`, enrichment incomplete and writes disabled, and exits 4 for review. Source/media interpretation and native task/history evidence remain pending; no Anki write, binding, render or approval is produced. Alternatively select `--query QUERY` or `--deck NAME`; selector families conflict. Deck names use the existing quoted Anki query compiler. A query/deck search runs once with profile checks before and after; returned IDs are normalized to unique numeric order by the read port, then pass through the same batch limits/capture/publication. Empty searches return `result.items=[]`, `item_count=0` and exit 0 without creating state. Query text must be nonempty and fit `input.max_record_chars`. The plan stores a typed selection receipt with the exact selector, normalized matched IDs, frozen selected IDs/order and configured note limit. Its validation and approval digest bind those values to captured source identities and frozen settings. `--limit N` is available only for query/deck selectors and accepts 1–100,000. It selects the first N IDs after frozen ordering; the receipt retains all matched IDs and `command_limit` separately. An explicit limit may exceed the configured default, while `selection.max_notes` remains unchanged in frozen settings. Without an explicit limit, over-default matches fail instead of being silently truncated. Match pools above 100,000 fail even with a smaller explicit limit. Explicit IDs cannot be combined with `--limit`. Partial item outcomes and full pipeline execution remain pending.

Revamp source-media archival: retrieve each discovered local filename through the profile-pinned read port, bounded by `media.max_asset_mb` and aggregate `input.max_file_mb`. Attach all filename/hash/size or missing observations to the source manifest before publication; errors reject the batch without partial attachment. Preserve exact bytes as source-owned archive-only media. Staging checks names, bytes, sizes and archive references; missing/empty/format-unverified content remains explicit review/error state. No filename extension establishes MIME, and no source bytes are replaced or physically deleted. Raster inspection identifies JPEG/PNG/GIF/WebP from bytes, enforces `media.allowed_image_types`, and decodes all animation buffers within the `media.max_asset_mb` cumulative buffer cap. APNG default-image buffers count separately. Successful inspection stores digest-linked `media_format` evidence and decoded MIME while retaining the archive role and content review. Unsupported, malformed or disallowed formats remain archived with `SOURCE_MEDIA_FORMAT_REVIEW`; invalid configuration fails staging. Failed inspection persists a digest-linked `media_format` evidence receipt with a stable failure category and `ambiguous=true`, without a success inspection. The review message gives replacement/configuration guidance. Decoder-supplied file text is excluded from failure receipts; original bytes remain available after reopening the plan. Decoder allocation limits are best effort; process isolation and hard deadlines, complete MP3/Ogg validation, render-role selection and native media consistency verification remain pending.

Source-audio inspection: when image format detection is unsupported, probe audio content without an extension hint. Accept only enabled MP3, Ogg/Vorbis and WAV/PCM decoding and configured `media.allowed_audio_types`. Enforce `media.max_asset_mb` on encoded bytes and cumulative decoded samples (eight budget bytes per channel sample). Decode every packet without skipping failures; reject multiple tracks and changing/chained streams. Disable metadata picture extraction and cap tag extraction at 64 KiB or the configured media cap, whichever is smaller. Receipts include sample rate, channels, decoded frames/packets, sample budget, decoder verification if available and separate stream-end/container-extent flags. Standard RIFF/WAVE requires exact chunk/container extents, valid block alignment and matching decoded frame count. MP3/Ogg container completeness remains unverified and produces `SOURCE_AUDIO_COMPLETENESS_REVIEW`. All media retains its archive role and original bytes. Unsupported content records `MEDIA_FORMAT_UNSUPPORTED`; typed audio failures retain guidance and no success receipt. Invalid audio allowlist configuration fails staging, never becomes a reviewable decode failure.

Ogg completeness: before probing content beginning with `OggS`, validate every page with RFC 3533 framing and CRC. Require one serial, contiguous sequences, correct continuation flags, complete lacing/payload bytes and a final EOS page with no pending packet or following bytes. Version/reserved flag violations fail. Multiple logical streams and chaining remain unsupported. Successful framing plus Vorbis decoding sets `container_extent_verified=true`; MP3 continues to require completeness review. This verifies framing, not authorship or rendering role.

MP3 completeness: after container probing identifies MP3, require every captured audio byte to belong to a complete indexed-bitrate MPEG Layer III frame or a supported tag extent. Check MPEG-1/2/2.5 headers, frame sizes including padding, stable sample rate/channel/version, leading ID3v2 synchsafe size and optional matching v2.4 footer, and optional trailing 128-byte ID3v1 tag. Do not scan past unexplained junk, infer free-format boundaries or accept partial final frames. Framing plus successful decoding sets `container_extent_verified=true`; original bytes remain archive-only. Whole-frame removal is undetectable without a trusted external length, so this flag proves captured-byte structure rather than authenticity.

Current authored grammar split staging: `plans split-grammar PLAN --request FILE`
loads the retained source draft and verifies the request's base revision/digest,
document/input digest and actor. The request contains `schema_version=2`,
`anchor_index` (zero-based) and ordered `units` using the typed Grammar body.
Each unit must have a distinct nonempty pattern/use key. One unit retains the
source document ID; every sibling gets a fresh ID and Recognition task. The
anchor retains its requested tasks. Preserve original fields, media and archives
on all units; source archives are evidence, not scheduling instructions.
Reject stale input, invalid indices, duplicate units, non-user examples and
frozen byte/character/derived-size limit violations before publishing a child.
Archive the exact request, then publish one child under parent CAS. The
approval-bound grammar group records the anchor and ordered siblings, actor and
request asset digest. Exit 4 with native split/history review unresolved and writes
disabled. See the [request schema](../../../contracts/v2/grammar-split-request.schema.json).
Automatic OCR segmentation and native write/recovery execution remain pending.

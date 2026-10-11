# Generation and transformation workflow

Active request: `/home/lam/.codex/attachments/62c674cb-059a-4a1d-bf21-a54bf3a78a9f/pasted-text-1.txt`.

The previous release gate covered compilation and unit contracts, not the full
workflow below. Completion requires functional evidence for every item.

Current priority: complete Japanese and English vocabulary first. Defer further
grammar work. Pictures must be retrieved from the internet, following legacy
`fetch_web_image` in `tui/screens.py`; do not synthesize pictures with AI.

## Required outcome

- A continuous themed popup border, including all corners.
- One transformation/generation screen with comparable legacy and target field
  values and equally sized fields; downloaded images must render.
- Clickable transformation nodes between fields, exposing the actual per-field
  settings and shared dependencies inherited from the Python workflow.
- Exercise Japanese and English generation end to end against dictionary/API,
  local Ollama, prompts, parsing, and image providers. Do not write test cards to
  the user's collection as part of this check.
- A separate preview, unavailable before generation, rendering actual Anki
  template HTML/CSS. Place explicit dry-run and apply controls below that preview.
- A recovery screen supporting one card and selected multiple cards with the
  existing snapshot conflict checks.

## Verified progress

- Grammar examples now hydrate from Anki, accept independently in either order
  with explanation, preserve edits through undo/redo, and map back to exactly
  Expression/Explanation/Examples. Focused round-trip test covers this behavior.
- Transformation and generation controls now share a screen; preview remains a
  separate screen. Further layout work below remains necessary.
- Preview is gated on accepted generation and resolution of pending changes.
  Explicit dry-run/apply controls sit below the card faces; backend guards also
  prevent shortcut calls from bypassing generation.
- Dialogs use a square accent frame, removing the rounded corner gaps in the
  old frame. Visual verification remains outstanding.
- Target fields display pipeline HTML and downloaded image data, including
  pending media before accepting changes. Target heights accommodate their
  matched source fields.
- Vocabulary commits now select their managed output schema independently of
  legacy source mappings. Preview purpose resolves from the deck configuration.
- Native internet image lookup receives dictionary meaning for English
  Wikipedia/Commons queries, matching legacy search context. Cache keys include
  that meaning so a corrected definition cannot reuse an unrelated old result.

## Remaining implementation and verification

- Verify target field values and pending downloaded media with live vocabulary
  pipeline results.
- Choose preview templates by actual purpose and replace Qt Text.RichText with
  an Anki-capable HTML renderer. Move the dry-run results under the card faces.
- Ensure legacy source mappings actually drive generation input and retain user
  context/audio. Grammar screenshot routing is deferred until vocabulary works.
- Expose real transformation settings and dependencies through connection nodes.
- Run slow generation off the GUI thread, with progress and usable errors.
- Build recovery selection UI on snapshot restore; prove multi-card conflict and
  partial-failure behavior.
- Verify the whole live Japanese/English pipeline and then run release checks.

Keep goal active until these items are finished. Unit tests alone do not prove
HTML preview fidelity or successful live dictionary/LLM/image integration.

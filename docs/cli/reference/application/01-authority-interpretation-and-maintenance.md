# Application specification reference

## 1. Authority, interpretation, and maintenance

**Read this document before implementing or changing the new CLI.** It is the persistent product and pipeline specification for that implementation. The user's current direction is a command-only application first. Earlier TUI and Qt screen requirements are historical interface requirements; preserve their useful workflow semantics, not their screens.

The following labels distinguish evidence from decisions:

- **CURRENT**: behavior observed in source; it may differ between Rust and Python.
- **REQUIRED**: requirements of the proposed CLI contract, subject to the review decisions explicitly listed below.
- **RECOMMENDED**: engineering or usability proposals, not existing functionality or approved user preferences.
- **REVIEW Rxx**: a review reference for a design choice or evidence gap. The user has delegated detailed implementation choices to the assistant; the researched plan now supplies selected proposals for these references. They are reviewable decisions, not 34 unanswered prerequisites. A review flag does not authorize a destructive fallback.

The command names and flags below describe the **target interface**. They do not currently exist except where explicitly identified. This task writes specifications, not the CLI implementation. Update this document and its review register when decisions change; do not silently promote recommendations to implemented behavior. New CLI behavior takes precedence over historical GUI workflow documents. Existing compatibility contracts remain binding until explicitly versioned or migrated.

# Application specification reference

## 2. Product purpose and release scope

The app turns existing Anki language-learning notes and new vocabulary inputs into inspectable, consistent learning material. It combines dictionary evidence, screenshot OCR, local Ollama annotations, Kanji information, illustrative internet images, pronunciation audio, and user context. It writes through AnkiConnect and keeps recoverable local history.

The unit of work is an **Anki note**, not an individual review card. A managed vocabulary note can produce three cards. Counts must say whether they represent input rows, source notes, output notes, or Anki cards.

**REQUIRED first interface:** ordinary shell subcommands, usable over SSH and from scripts, with no full-screen TUI, Qt window, mandatory graphical preview, or background server. Invocation without a subcommand prints useful help and exits successfully. No services start merely because help is requested.

**REQUIRED release scope, clarified by the user:** revamp existing vocabulary, revamp existing grammar, add vocabulary, and add grammar to the new standards. All four workflows share CLI preparation, review, apply, batching and recovery. Japanese and English are the initial validated language targets; grammar is not deferred. Earlier vocabulary-first GUI priorities in `transformation-workflow.md` do not govern this implementation. **REVIEW R01 is resolved for workflow scope.**

Not first-release work: review scheduling algorithms, direct writes to Anki's collection database, AnkiWeb synchronization, a GUI/TUI, an HTTP application service, AI picture synthesis, or unattended model/language-pack downloads. Anki Desktop and AnkiConnect remain external runtime dependencies for collection operations.

**SELECTED implementation proposal:** a Rust CLI composition crate using the existing domain, application, provider, jobs, and snapshot crates. Python remains reference/compatibility tooling and an isolated optional advanced-OCR/browser helper, never the mandatory TUI runtime. This is the assistant's delegated design choice for review: **REVIEW R02**.

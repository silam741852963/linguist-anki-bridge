# Application specification reference

## 14. Engineering and usability recommendations with references

These references were checked when writing this draft. They support the practices named here; the detailed pipeline decisions elsewhere are application-specific proposals based on the source review.

- Keep help discoverable, results separate from diagnostics, machine output stable, color optional, errors actionable, and interruption understandable. These are CLI conventions described in the [Command Line Interface Guidelines](https://clig.dev/). For this app, a failed item should identify its expression/stage, retain its plan, and print the exact retry/review command.
- Separate config, durable state, data and cache according to the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir/latest/). For this app, cache cleanup must never erase accepted plans or rollback evidence.
- Keep WAL databases on a supported local filesystem and account for WAL/shared-memory files when migrating/backing up active stores. SQLite documents WAL concurrency and deployment constraints in its [WAL documentation](https://www.sqlite.org/wal.html). For this app, use consistent database backup/import procedures rather than assuming a main-file copy alone captures active state.

Additional app-specific recommendations: keep command parsing thin; inject providers into application use cases; use one canonical document builder; isolate deterministic parsing/rendering from transport; version plans and schemas; record stage dependencies; make recovery an ordinary supported command; favor source-preserving defaults; and expose effective settings in human terms. These are design recommendations, not claims that a particular standard mandates this architecture.

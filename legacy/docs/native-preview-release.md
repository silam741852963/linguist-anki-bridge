# Native GUI preview release

The native Qt preview installs as `linguist-anki-bridge-native`. It does not
replace the Python executable, `linguist-anki-bridge`; both may be installed
and used alongside each other during the migration.

Build and install the Arch preview package with
`makepkg -p PKGBUILD.native --syncdeps --install`, then start the native preview
with:

```bash
linguist-anki-bridge-native
```

Keep dry-run enabled until the preview has been checked. The QML views and
Lucide-derived SVG assets are embedded in the native binary, so the installed
executable does not depend on files from the source checkout. Python remains
installed as an independent recovery path throughout the preview release.

Before packaging a release candidate, run:

```bash
scripts/verify-native-release.sh
```

This checks the Arch recipe and staged install layout, all Rust functionality,
the Python fallback suite when its virtual environment is available, linting,
and the release binary with its embedded QML resources. It does not connect to
a live Anki profile or mutate user data.

## Before trying writes

Back up these paths while Anki is closed:

- `$XDG_CONFIG_HOME/linguist-anki-bridge/config.yaml` — Python settings
- `$XDG_CONFIG_HOME/linguist-anki-bridge/card_snapshots.json` — Python snapshots
- `$XDG_CONFIG_HOME/linguist-anki-bridge/snapshots-v1/` — native snapshots
- `$XDG_CONFIG_HOME/linguist-anki-bridge/batch_jobs*` — Python batch state
- `$XDG_CONFIG_HOME/linguist-anki-bridge/batch_jobs.native.sqlite3*` — native jobs
- the Anki profile collection and media directory

The native importer reads `config.yaml` but never rewrites it. Its separate
versioned config is `native-config-v1.json` in the same config directory.

## Roll back to Python

1. Stop the native GUI; no batch runner resumes automatically after restart.
2. Restore a snapshot from the Python app for any completed write, resolving
   newer-write conflicts before retrying.
3. Start `linguist-anki-bridge` and continue from the preserved Python queue,
   snapshots, and configuration.
4. If native batch data needs inspection, keep its SQLite file and artifact
   directory intact; do not delete either before exporting diagnostics.

Uninstalling the native package removes only packaged files. It does not remove
configuration, snapshots, jobs, Anki data, or the Python application.

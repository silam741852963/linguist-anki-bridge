# Native GUI preview release

The native Qt preview installs as `linguist-anki-bridge-native`. It does not
replace the Python executable, `linguist-anki-bridge`; both may be installed
and used alongside each other during the migration.

Build the Arch preview package with `makepkg -f PKGBUILD.native`, then start
the native preview with:

```bash
linguist-anki-bridge-native
```

Keep dry-run enabled until a preview has been checked. Python remains the
recovery path while native provider and commit adapters reach full parity.

## Before trying writes

Back up these paths while Anki is closed:

- `$XDG_CONFIG_HOME/linguist-anki-bridge/config.yaml` — Python settings
- `$XDG_CONFIG_HOME/linguist-anki-bridge/snapshots*` — card snapshots
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

Uninstalling the native package removes only the executable and this document;
it does not remove configuration, snapshots, jobs, Anki data, or the Python
application.

//! WP-14 handlers: legacy config import/activation, resources, cache and
//! legacy job import. Each delegates to library code and emits one result.
use crate::{Cli, emit};
use std::collections::BTreeMap;
use std::path::Path;

fn environment() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

/// OP-09: write a candidate, report and exported resources; never activates.
pub fn config_import(cli: &Cli, file: &Path, output: &Path, replace: bool) -> Result<u8, String> {
    let environment = environment();
    let live = linguist_config::config_path(cli.config.as_deref(), &environment)?;
    let source = {
        use std::io::Read;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let handle = options
            .open(file)
            .map_err(|_| "LEGACY_CONFIG_IO: cannot read the legacy file")?;
        if !handle.metadata().map_err(|_| "LEGACY_CONFIG_IO")?.is_file() {
            return Err("LEGACY_CONFIG_NOT_REGULAR_FILE".into());
        }
        let mut bytes = Vec::new();
        handle
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "LEGACY_CONFIG_IO")?;
        bytes
    };
    crate::diagnostics::progress("importing legacy configuration");
    let mut import = linguist_config::legacy::import(&source, &environment)?;
    let written =
        linguist_config::legacy::write_outputs(&mut import, file, output, &live, replace)?;
    let report = &import.report;
    emit(&serde_json::json!({
        "schema_version": 1,
        "source": file,
        "source_format": report.source_format,
        "source_sha256": report.source_sha256,
        "source_modified": false,
        "candidate": written.candidate,
        "candidate_sha256": report.candidate_sha256,
        "report": written.report,
        "resource_directory": written.resource_directory,
        "exported_resources": written.resources,
        "counts": report.counts,
        "blocking_keys": report.blocking_keys,
        "activation_blocked": report.activation_blocked,
        "follow_up": report.follow_up,
        "activated": false,
        "live_config": live,
        "next_commands": [
            format!("config validate --file {}", written.candidate.display()),
            format!(
                "config migrate --from-import {}{} --execute",
                written.candidate.display(),
                report
                    .blocking_keys
                    .iter()
                    .map(|k| format!(" --accept-unresolved {k}"))
                    .collect::<String>()
            ),
        ],
    }))?;
    Ok(if report.activation_blocked { 4 } else { 0 })
}

/// OP-10 activation of an imported candidate after every blocking key is accepted.
pub fn config_activate(
    cli: &Cli,
    candidate: &Path,
    accepted: &[String],
    execute: bool,
) -> Result<u8, String> {
    let environment = environment();
    let live = linguist_config::config_path(cli.config.as_deref(), &environment)?;
    if std::fs::canonicalize(candidate).ok() == std::fs::canonicalize(&live).ok()
        && std::fs::canonicalize(candidate).is_ok()
    {
        return Err("IMPORT_CANDIDATE_IS_LIVE_CONFIG".into());
    }
    let (bytes, report, accepted) = linguist_config::legacy::check_activation(candidate, accepted)?;
    let receipt = linguist_config::edit::activate(&live, &bytes, execute, &environment)?;
    emit(&serde_json::json!({
        "schema_version": 1,
        "candidate": candidate,
        "candidate_sha256": report.candidate_sha256,
        "source_sha256": report.source_sha256,
        "accepted_unresolved": accepted,
        "follow_up": report.follow_up,
        "activation": receipt,
        "requested_execute": execute,
    }))?;
    Ok(0)
}

pub fn cache_status(
    settings: &linguist_config::Effective,
    provider: Option<&str>,
) -> Result<u8, String> {
    emit(&linguist_application::cache::status(
        settings,
        &environment(),
        provider,
    )?)?;
    Ok(0)
}

pub fn cache_prune(
    settings: &linguist_config::Effective,
    provider: Option<&str>,
    age_days: Option<u32>,
    budget_mb: Option<u64>,
    execute: bool,
) -> Result<u8, String> {
    if execute {
        crate::diagnostics::progress("pruning unreferenced cache");
    }
    emit(&linguist_application::cache::prune(
        settings,
        &environment(),
        provider,
        age_days,
        budget_mb,
        execute,
    )?)?;
    Ok(0)
}

pub fn resources_list(
    settings: &linguist_config::Effective,
    installed: bool,
    required: bool,
    resource: Option<&str>,
) -> Result<u8, String> {
    if installed && required {
        return Err("RESOURCE_FILTER_CONFLICT: choose --installed or --required".into());
    }
    emit(&linguist_application::resources::list(
        settings,
        &environment(),
        resource,
        installed,
        required,
    )?)?;
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
pub fn resources_install(
    settings: &linguist_config::Effective,
    resource: &str,
    source: &str,
    version: &str,
    sha256: &str,
    license: &str,
    destination: Option<&Path>,
    execute: bool,
) -> Result<u8, String> {
    if execute {
        crate::diagnostics::progress(&format!("installing {resource}"));
    }
    emit(&linguist_application::resources::install(
        settings,
        &environment(),
        &linguist_application::resources::InstallRequest {
            resource: resource.into(),
            source: source.into(),
            version: version.into(),
            sha256: sha256.into(),
            license: license.into(),
            destination: destination.map(Path::to_owned),
            execute,
        },
    )?)?;
    Ok(0)
}

/// OP-47 legacy job import: preview, or `--execute` to keep the original bytes
/// and the read-only translation in local state.
pub fn jobs_legacy(
    settings: &linguist_config::Effective,
    root: &Path,
    legacy: &Path,
    execute: bool,
) -> Result<u8, String> {
    let source = linguist_application::legacy_jobs::read_source(legacy)?;
    let scratch = linguist_config::expand_path(
        settings.values["storage.temp_dir"].as_str().unwrap(),
        &environment(),
    )?;
    let report = linguist_application::legacy_jobs::translate(&source, &scratch)?;
    let review = report["requires_review"].as_array().map_or(0, Vec::len);
    if !execute {
        emit(&serde_json::json!({
            "schema_version": 1,
            "mode": "preview",
            "source": legacy,
            "source_modified": false,
            "report": report,
            "next": "repeat with --execute to keep the original database and this read-only translation",
        }))?;
        return Ok(if review > 0 { 4 } else { 0 });
    }
    let mut store = linguist_store::Store::open(root)?;
    let mut report = report;
    if let Some(wal) = &source.wal {
        // The WAL is retained too: its digest in the report keeps it reachable.
        store.publish_asset(wal, 100 * 1024 * 1024)?;
    }
    report["source"]["path"] = serde_json::json!(legacy);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let (record, created) = store.record_legacy_job_import(&source.database, &report, now)?;
    emit(&serde_json::json!({
        "schema_version": 1,
        "mode": "import",
        "import_id": record.id,
        "created": created,
        "source_sha256": record.source_sha256,
        "report_digest": record.report_digest,
        "requires_review": record.report["requires_review"],
        "counts": record.report["counts"],
        "source_modified": false,
        "dispatches": false,
    }))?;
    Ok(if review > 0 { 4 } else { 0 })
}

pub fn jobs_legacy_list(root: &Path, show: Option<uuid::Uuid>) -> Result<u8, String> {
    if std::fs::symlink_metadata(root.join("state.sqlite3")).is_err() {
        emit(&serde_json::json!({"schema_version":1,"imports":[]}))?;
        return Ok(0);
    }
    let store = linguist_store::Store::read_only(root)?;
    match show {
        Some(id) => emit(&store.legacy_job_import(id)?)?,
        None => {
            let imports: Vec<_> = store
                .legacy_job_imports(1000)?
                .into_iter()
                .map(|(id, digest, ms)| serde_json::json!({"import_id": id, "source_sha256": digest, "imported_ms": ms}))
                .collect();
            emit(&serde_json::json!({"schema_version":1,"imports":imports}))?;
        }
    }
    Ok(0)
}

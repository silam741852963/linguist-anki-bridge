//! `cache status` (OP-55) and `cache prune` (OP-56).
//!
//! Two areas are managed. The provider response cache under
//! `storage.cache_dir/provider-v1` is disposable: accepted evidence is archived
//! separately. Store assets are pruned only through the store's mark-and-sweep
//! (reachable plans, jobs, approvals, snapshots, journals, receipts and imports
//! are roots). Anki media, backups, resources and history are never touched.
use linguist_config::Effective;
use linguist_store::gc::{self, PrunePolicy};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SERVICES: [&str; 4] = ["dictionary", "kanji", "image", "tts"];
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderEntry {
    pub service: String,
    pub key: String,
    pub files: Vec<PathBuf>,
    pub bytes: u64,
    /// Fetch time from intact metadata, else the newest file time.
    pub age_ms: u64,
    pub state: &'static str,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn cache_root(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let root = linguist_config::expand_path(
        settings.values["storage.cache_dir"]
            .as_str()
            .ok_or("CACHE_SETTING_MISSING")?,
        environment,
    )?;
    if !root.is_absolute() {
        return Err("CACHE_DIR_MUST_BE_ABSOLUTE".into());
    }
    Ok(root)
}

fn state_root(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let root = linguist_config::expand_path(
        settings.values["storage.state_dir"]
            .as_str()
            .ok_or("CACHE_SETTING_MISSING")?,
        environment,
    )?;
    if !root.is_absolute() {
        return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
    }
    Ok(root)
}

/// Inventory one service's provider cache. Unknown files are reported, not touched.
fn provider_entries(root: &Path, service: &str, now: u64) -> (Vec<ProviderEntry>, Vec<PathBuf>) {
    let directory = root.join("provider-v1").join(service);
    let mut groups: BTreeMap<String, Vec<(PathBuf, std::fs::Metadata)>> = BTreeMap::new();
    let mut unmanaged = Vec::new();
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return (Vec::new(), unmanaged);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let key = name.split('.').next().unwrap_or_default().to_owned();
        let shaped = key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit());
        if !meta.is_file() || meta.is_symlink() || !shaped {
            unmanaged.push(path);
            continue;
        }
        groups.entry(key).or_default().push((path, meta));
    }
    let mut out = Vec::new();
    for (key, files) in groups {
        let bytes = files.iter().map(|(_, m)| m.len()).sum();
        let newest = files.iter().map(|(_, m)| mtime_ms(m)).max().unwrap_or(0);
        let meta_path = directory.join(format!("{key}.json"));
        let body_path = directory.join(format!("{key}.body"));
        let temporary = files.iter().all(|(p, _)| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.starts_with("tmp-"))
        });
        let parsed = std::fs::read(&meta_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let (state, fetched_ms) = match (&parsed, std::fs::symlink_metadata(&body_path)) {
            _ if temporary => ("temporary", None),
            (Some(entry), Ok(body))
                if entry["schema"] == "linguist-provider-cache-v1"
                    && entry["bytes"].as_u64() == Some(body.len()) =>
            {
                ("intact", entry["fetched_at"].as_u64().map(|s| s * 1000))
            }
            _ => ("incomplete_or_corrupt", None),
        };
        out.push(ProviderEntry {
            service: service.into(),
            key,
            files: files.into_iter().map(|(p, _)| p).collect(),
            bytes,
            age_ms: now.saturating_sub(fetched_ms.unwrap_or(newest)),
            state,
        });
    }
    (out, unmanaged)
}

fn selected_services(provider: Option<&str>) -> Result<Vec<&'static str>, String> {
    match provider {
        None => Ok(SERVICES.to_vec()),
        Some(name) => SERVICES
            .iter()
            .find(|s| **s == name)
            .map(|s| vec![*s])
            .ok_or_else(|| {
                format!(
                    "CACHE_PROVIDER_UNKNOWN: {name}; use one of {}",
                    SERVICES.join(", ")
                )
            }),
    }
}

fn open_store(root: &Path, writable: bool) -> Result<Option<linguist_store::Store>, String> {
    if std::fs::symlink_metadata(root.join("state.sqlite3")).is_err() {
        return Ok(None);
    }
    if writable {
        linguist_store::Store::open_existing(root).map(Some)
    } else {
        linguist_store::Store::read_only(root).map(Some)
    }
}

fn retention_ms(settings: &Effective, age_days: Option<u32>) -> u64 {
    age_days.map(u64::from).unwrap_or_else(|| {
        settings.values["cache.unreferenced_retention_days"]
            .as_u64()
            .unwrap_or(30)
    }) * DAY_MS
}

fn budget_bytes(settings: &Effective, budget_mb: Option<u64>) -> u64 {
    budget_mb.unwrap_or_else(|| {
        settings.values["cache.max_size_mb"]
            .as_u64()
            .unwrap_or(2048)
    }) * 1024
        * 1024
}

/// Totals, reachability and retention policy. Never deletes.
pub fn status(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    provider: Option<&str>,
) -> Result<Value, String> {
    let now = now_ms();
    let services = selected_services(provider)?;
    let cache = cache_root(settings, environment)?;
    let retention = retention_ms(settings, None);
    let mut per_service = serde_json::Map::new();
    let mut provider_bytes = 0;
    let mut unmanaged_paths = Vec::new();
    for service in &services {
        let (entries, unmanaged) = provider_entries(&cache, service, now);
        let bytes: u64 = entries.iter().map(|e| e.bytes).sum();
        provider_bytes += bytes;
        unmanaged_paths.extend(unmanaged);
        per_service.insert(
            (*service).into(),
            json!({
                "entries": entries.len(),
                "bytes": bytes,
                "older_than_retention": entries.iter().filter(|e| e.age_ms >= retention).count(),
                "incomplete_or_corrupt": entries.iter().filter(|e| e.state != "intact").count(),
            }),
        );
    }
    let store = if provider.is_some() {
        Value::Null
    } else {
        let root = state_root(settings, environment)?;
        match open_store(&root, false)? {
            None => json!({"state": "absent"}),
            Some(store) => {
                let census = store.asset_census()?;
                use linguist_store::gc::FileKind;
                json!({
                    "state": "present",
                    "files": census.files.len(),
                    "total_bytes": census.bytes(|_| true),
                    "reachable_bytes": census.bytes(|f| f.reachable),
                    "protected_roots": census.roots,
                    "unreferenced_bytes": census.bytes(|f| !f.reachable && f.kind != FileKind::Unmanaged),
                    "unreferenced_files": census.files.iter().filter(|f| !f.reachable && f.kind != FileKind::Unmanaged).count(),
                    "stray_files": census.files.iter().filter(|f| f.kind == FileKind::Stray).count(),
                    "temporary_files": census.files.iter().filter(|f| f.kind == FileKind::Temporary).count(),
                    "unmanaged_files": census.files.iter().filter(|f| f.kind == FileKind::Unmanaged).map(|f| f.name.clone()).collect::<Vec<_>>(),
                    "indexed_but_missing": census.missing_files,
                    "active_leases": census.active_leases,
                    "pending_recovery_journals": census.pending_journals,
                    "interrupted_prune_runs": census.interrupted_runs,
                    "prune_blockers": census.blockers(),
                    "recent_prune_runs": store.gc_runs(5)?,
                })
            }
        }
    };
    Ok(json!({
        "schema_version": 1,
        "cache_dir": cache,
        "provider_cache": {"bytes": provider_bytes, "services": per_service, "unmanaged_paths": unmanaged_paths},
        "store_assets": store,
        "policy": {
            "cache.unreferenced_retention_days": settings.values["cache.unreferenced_retention_days"],
            "cache.max_size_mb": settings.values["cache.max_size_mb"],
            "orphan_grace_seconds": gc::ORPHAN_GRACE_MS / 1000,
            "never_pruned": ["reachable store assets", "Anki media", "checkpoints/backups", "installed resources", "journals and snapshots"],
        },
        "deletes": false,
    }))
}

/// Preview (default) or execute pruning. Execution re-inventories, rechecks
/// leases and recovery under the store's write lock and refuses when blocked.
pub fn prune(
    settings: &Effective,
    environment: &BTreeMap<String, String>,
    provider: Option<&str>,
    age_days: Option<u32>,
    budget_mb: Option<u64>,
    execute: bool,
) -> Result<Value, String> {
    let now = now_ms();
    let services = selected_services(provider)?;
    let cache = cache_root(settings, environment)?;
    let retention = retention_ms(settings, age_days);
    let budget = budget_bytes(settings, budget_mb);
    let state = state_root(settings, environment)?;
    let mut store = if provider.is_some() {
        None
    } else {
        open_store(&state, execute)?
    };
    // Any active lease (a reader/worker/writer) or pending recovery blocks both
    // areas: a running job may be reading the provider cache.
    let blockers = match store.as_ref() {
        Some(store) => store.asset_census()?.blockers(),
        None => match open_store(&state, false)? {
            Some(store) => store.asset_census()?.blockers(),
            None => Vec::new(),
        },
    };
    let mut entries = Vec::new();
    for service in &services {
        entries.extend(provider_entries(&cache, service, now).0);
    }
    let grace = gc::ORPHAN_GRACE_MS;
    let mut chosen: Vec<(ProviderEntry, &'static str)> = Vec::new();
    let mut kept = Vec::new();
    for entry in entries {
        if entry.state != "intact" && entry.age_ms >= grace {
            chosen.push((entry, "incomplete, corrupt or interrupted entry"));
        } else if entry.state == "intact" && entry.age_ms >= retention {
            chosen.push((entry, "older than retention"));
        } else {
            kept.push(entry);
        }
    }
    let census = match store.as_ref() {
        Some(store) => Some(store.asset_census()?),
        None => None,
    };
    let age_policy = PrunePolicy {
        retention_ms: retention,
        unreferenced_budget_bytes: None,
        now_ms: now,
    };
    let store_after_age: u64 = census.as_ref().map_or(0, |c| {
        let names: std::collections::BTreeSet<String> = gc::candidates(c, &age_policy)
            .into_iter()
            .map(|c| c.name)
            .collect();
        c.bytes(|f| !f.reachable && f.kind != gc::FileKind::Unmanaged && !names.contains(&f.name))
    });
    let mut provider_remaining: u64 = kept.iter().map(|e| e.bytes).sum();
    if provider_remaining + store_after_age > budget {
        kept.sort_by_key(|e| std::cmp::Reverse(e.age_ms));
        for entry in kept {
            if provider_remaining + store_after_age <= budget {
                break;
            }
            provider_remaining -= entry.bytes;
            chosen.push((entry, "over cache.max_size_mb budget (oldest first)"));
        }
    }
    let store_policy = PrunePolicy {
        retention_ms: retention,
        unreferenced_budget_bytes: Some(budget.saturating_sub(provider_remaining)),
        now_ms: now,
    };
    let provider_candidates: Vec<Value> = chosen
        .iter()
        .map(|(e, reason)| json!({"service": e.service, "key": e.key, "bytes": e.bytes, "age_seconds": e.age_ms / 1000, "state": e.state, "reason": reason}))
        .collect();
    let provider_bytes: u64 = chosen.iter().map(|(e, _)| e.bytes).sum();
    if !execute {
        let store_preview = match store.as_mut() {
            Some(store) => serde_json::to_value(store.prune_assets(&store_policy, false)?)
                .map_err(|e| e.to_string())?,
            None => Value::Null,
        };
        return Ok(json!({
            "schema_version": 1,
            "executed": false,
            "blockers": blockers,
            "policy": {"retention_days": retention / DAY_MS, "budget_bytes": budget, "orphan_grace_seconds": grace / 1000},
            "provider_cache": {"candidates": provider_candidates, "bytes": provider_bytes},
            "store_assets": store_preview,
            "next": "repeat with --execute; reachability, leases and recovery are rechecked first",
        }));
    }
    if !blockers.is_empty() {
        return Err(blockers.join("; "));
    }
    let store_receipt = match store.as_mut() {
        Some(store) => Some(store.prune_assets(&store_policy, true)?),
        None => None,
    };
    let mut removed = Vec::new();
    let mut failures = Vec::new();
    for (entry, _) in &chosen {
        // Metadata first: a concurrent reader then sees a miss, never a mismatch.
        let mut files = entry.files.clone();
        files.sort_by_key(|p| !p.extension().is_some_and(|e| e == "json"));
        let mut ok = true;
        for file in files {
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    ok = false;
                    failures.push(file);
                }
            }
        }
        if ok {
            removed.push(json!({"service": entry.service, "key": entry.key, "bytes": entry.bytes}));
        }
    }
    Ok(json!({
        "schema_version": 1,
        "executed": true,
        "policy": {"retention_days": retention / DAY_MS, "budget_bytes": budget, "orphan_grace_seconds": grace / 1000},
        "provider_cache": {"removed": removed, "bytes": provider_bytes, "failures": failures},
        "store_assets": store_receipt,
        "anki_media_touched": false,
    }))
}

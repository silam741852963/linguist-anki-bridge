//! ALG-GC for the content-addressed asset directory.
//!
//! Reachability is conservative: every 64-hex run in any column of any table
//! (other than the asset index and GC bookkeeping) marks a matching asset, and
//! marked assets are scanned transitively for further references. Active
//! leases and pending journals block deletion entirely. Deletion is recorded
//! as a tombstone first, then the file is moved into a private trash directory
//! inside the same write transaction, and only unlinked after commit.
use crate::{Result, Store, sql};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS gc_runs(
 id TEXT PRIMARY KEY, created_ms INTEGER NOT NULL CHECK(created_ms>=0),
 policy BLOB NOT NULL, state TEXT NOT NULL CHECK(state IN('tombstoned','completed')),
 completed_ms INTEGER);
CREATE TABLE IF NOT EXISTS gc_tombstones(
 run TEXT NOT NULL REFERENCES gc_runs(id), name TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN('asset','stray','temporary')),
 size INTEGER NOT NULL CHECK(size>=0), modified_ms INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN('tombstoned','unlinked')),
 PRIMARY KEY(run,name));
CREATE TRIGGER IF NOT EXISTS gc_tombstones_no_delete BEFORE DELETE ON gc_tombstones BEGIN SELECT RAISE(ABORT,'gc tombstones are retained'); END;
CREATE TRIGGER IF NOT EXISTS gc_runs_no_delete BEFORE DELETE ON gc_runs BEGIN SELECT RAISE(ABORT,'gc runs are retained'); END;
";

/// Unreferenced store files younger than this are never deleted, whatever the
/// configured retention: a concurrent writer may not have referenced them yet.
pub const ORPHAN_GRACE_MS: u64 = 60 * 60 * 1000;
const TRASH: &str = ".gc-trash";
const SCAN_LIMIT: u64 = 100 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// Indexed in `assets` and present on disk.
    Asset,
    /// A 64-hex file with no index row (an interrupted publication).
    Stray,
    /// An interrupted temporary write (`.UUID.tmp`).
    Temporary,
    /// Anything else; reported, never deleted.
    Unmanaged,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetFile {
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified_ms: u64,
    pub reachable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Census {
    pub files: Vec<AssetFile>,
    /// Indexed assets whose file is missing (reported, never "fixed").
    pub missing_files: Vec<String>,
    /// Tables holding at least one live reference, with reference counts.
    pub roots: BTreeMap<String, u64>,
    pub active_leases: Vec<String>,
    pub pending_journals: u64,
    pub interrupted_runs: Vec<String>,
}

impl Census {
    pub fn blockers(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.active_leases.is_empty() {
            out.push(format!(
                "GC_BLOCKED_BY_ACTIVE_LEASE: {} active worker/reader/writer lease(s): {}",
                self.active_leases.len(),
                self.active_leases.join(", ")
            ));
        }
        if self.pending_journals > 0 {
            out.push(format!(
                "GC_BLOCKED_BY_RECOVERY: {} journal(s) need recovery; their objects are roots until resolved",
                self.pending_journals
            ));
        }
        out
    }
    pub fn bytes(&self, f: impl Fn(&AssetFile) -> bool) -> u64 {
        self.files.iter().filter(|a| f(a)).map(|a| a.size).sum()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PrunePolicy {
    /// Minimum age of an unreferenced file before it may be deleted.
    pub retention_ms: u64,
    /// When set, older unreferenced files beyond the age rule are also deleted
    /// (oldest first) until unreferenced bytes fit this budget.
    pub unreferenced_budget_bytes: Option<u64>,
    pub now_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified_ms: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PruneReceipt {
    pub run: Option<uuid::Uuid>,
    pub executed: bool,
    pub candidates: Vec<Candidate>,
    pub candidate_bytes: u64,
    pub unlinked: Vec<String>,
    pub unlink_failures: Vec<String>,
    pub blockers: Vec<String>,
    pub finished_interrupted_runs: Vec<String>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hex_runs(bytes: &[u8], known: &BTreeSet<String>, out: &mut Vec<String>) {
    let mut start = None;
    for (i, b) in bytes.iter().chain(std::iter::once(&b' ')).enumerate() {
        let hex = b.is_ascii_digit() || (b'a'..=b'f').contains(b);
        match (hex, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= 64 {
                    // Mark every 64-character window: longer runs may embed digests.
                    for w in s..=i - 64 {
                        let candidate = std::str::from_utf8(&bytes[w..w + 64]).unwrap();
                        if known.contains(candidate) {
                            out.push(candidate.to_owned());
                        }
                    }
                }
                start = None;
            }
            _ => {}
        }
    }
}

fn modified_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn is_digest(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_temporary(name: &str) -> bool {
    name.strip_prefix('.')
        .and_then(|rest| rest.strip_suffix(".tmp"))
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}

fn census_in(connection: &rusqlite::Connection, root: &Path) -> Result<Census> {
    let assets_dir = root.join("assets");
    let mut indexed = BTreeMap::new();
    {
        let mut statement = connection
            .prepare("SELECT digest,size FROM assets")
            .map_err(sql)?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(sql)?;
        for row in rows {
            let (digest, size) = row.map_err(sql)?;
            indexed.insert(digest, size as u64);
        }
    }
    let mut files = BTreeMap::new();
    let entries = match std::fs::read_dir(&assets_dir) {
        Ok(entries) => entries.collect::<Vec<_>>(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return Err("GC_ASSET_DIRECTORY_IO".into()),
    };
    for entry in entries {
        let entry = entry.map_err(|_| "GC_ASSET_DIRECTORY_IO")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == TRASH {
            continue;
        }
        let meta = std::fs::symlink_metadata(entry.path()).map_err(|_| "GC_ASSET_DIRECTORY_IO")?;
        let kind = if !meta.is_file() || meta.is_symlink() {
            FileKind::Unmanaged
        } else if is_digest(&name) {
            if indexed.contains_key(&name) {
                FileKind::Asset
            } else {
                FileKind::Stray
            }
        } else if is_temporary(&name) {
            FileKind::Temporary
        } else {
            FileKind::Unmanaged
        };
        files.insert(
            name.clone(),
            AssetFile {
                name,
                kind,
                size: meta.len(),
                modified_ms: modified_ms(&meta),
                reachable: false,
            },
        );
    }
    let known: BTreeSet<String> = files
        .values()
        .filter(|f| matches!(f.kind, FileKind::Asset | FileKind::Stray))
        .map(|f| f.name.clone())
        .chain(indexed.keys().cloned())
        .collect();
    // Mark from every database row.
    let tables: Vec<String> = {
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT IN('assets','gc_runs','gc_tombstones') ORDER BY name")
            .map_err(sql)?;
        statement
            .query_map([], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?
    };
    let mut roots = BTreeMap::new();
    let mut marked = BTreeSet::new();
    for table in tables {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM \"{}\"", table.replace('"', "\"\"")))
            .map_err(sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query([]).map_err(sql)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(sql)? {
            for i in 0..columns {
                match row.get_ref(i).map_err(sql)? {
                    rusqlite::types::ValueRef::Text(t) | rusqlite::types::ValueRef::Blob(t) => {
                        hex_runs(t, &known, &mut found)
                    }
                    _ => {}
                }
            }
        }
        if !found.is_empty() {
            roots.insert(table, found.len() as u64);
            marked.extend(found);
        }
    }
    // Transitive marking through the content of reachable assets.
    let mut queue: Vec<String> = marked.iter().cloned().collect();
    while let Some(digest) = queue.pop() {
        let path = assets_dir.join(&digest);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > SCAN_LIMIT {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let mut found = Vec::new();
        hex_runs(&bytes, &known, &mut found);
        for next in found {
            if marked.insert(next.clone()) {
                queue.push(next);
            }
        }
    }
    for file in files.values_mut() {
        file.reachable = marked.contains(&file.name);
    }
    let missing_files = indexed
        .keys()
        .filter(|d| !files.contains_key(*d))
        .cloned()
        .collect();
    let active_leases = {
        let mut statement = connection
            .prepare("SELECT resource FROM leases WHERE active=1 ORDER BY resource")
            .map_err(sql)?;
        statement
            .query_map([], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?
    };
    let pending_journals: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM journal_heads h JOIN journal_events e ON e.operation=h.operation AND e.sequence=h.sequence WHERE e.pending=1",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    let interrupted_runs = {
        let mut statement = connection
            .prepare("SELECT id FROM gc_runs WHERE state='tombstoned' ORDER BY created_ms,id")
            .map_err(sql)?;
        statement
            .query_map([], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?
    };
    Ok(Census {
        files: files.into_values().collect(),
        missing_files,
        roots,
        active_leases,
        pending_journals: pending_journals as u64,
        interrupted_runs,
    })
}

/// Select deletion candidates; reachable files and unmanaged entries never qualify.
pub fn candidates(census: &Census, policy: &PrunePolicy) -> Vec<Candidate> {
    let age = |f: &AssetFile| policy.now_ms.saturating_sub(f.modified_ms);
    let eligible: Vec<&AssetFile> = census
        .files
        .iter()
        .filter(|f| {
            !f.reachable
                && f.kind != FileKind::Unmanaged
                && age(f) >= ORPHAN_GRACE_MS
                && f.modified_ms <= policy.now_ms
        })
        .collect();
    let mut chosen: Vec<Candidate> = eligible
        .iter()
        .filter(|f| f.kind == FileKind::Temporary || age(f) >= policy.retention_ms)
        .map(|f| Candidate {
            name: f.name.clone(),
            kind: f.kind,
            size: f.size,
            modified_ms: f.modified_ms,
            reason: if f.kind == FileKind::Temporary {
                "interrupted temporary write".into()
            } else {
                "unreferenced beyond retention".into()
            },
        })
        .collect();
    if let Some(budget) = policy.unreferenced_budget_bytes {
        let chosen_names: BTreeSet<String> = chosen.iter().map(|c| c.name.clone()).collect();
        let mut remaining: u64 = census
            .files
            .iter()
            .filter(|f| {
                !f.reachable && f.kind != FileKind::Unmanaged && !chosen_names.contains(&f.name)
            })
            .map(|f| f.size)
            .sum();
        let mut extra: Vec<&&AssetFile> = eligible
            .iter()
            .filter(|f| !chosen_names.contains(&f.name))
            .collect();
        extra.sort_by_key(|f| (f.modified_ms, f.name.clone()));
        for f in extra {
            if remaining <= budget {
                break;
            }
            remaining -= f.size;
            chosen.push(Candidate {
                name: f.name.clone(),
                kind: f.kind,
                size: f.size,
                modified_ms: f.modified_ms,
                reason: "unreferenced over budget (oldest first)".into(),
            });
        }
    }
    chosen.sort_by(|a, b| a.name.cmp(&b.name));
    chosen
}

impl Store {
    /// Read-only reachability census of the asset directory.
    pub fn asset_census(&self) -> Result<Census> {
        census_in(&self.connection, &self.root)
    }

    /// Finish unlinking files left in the trash by an interrupted prune.
    fn finish_interrupted(&mut self) -> Result<Vec<String>> {
        let runs: Vec<String> = census_in(&self.connection, &self.root)?.interrupted_runs;
        for run in &runs {
            let names: Vec<String> = {
                let mut statement = self
                    .connection
                    .prepare("SELECT name FROM gc_tombstones WHERE run=?1 AND state='tombstoned'")
                    .map_err(sql)?;
                statement
                    .query_map([run], |r| r.get(0))
                    .map_err(sql)?
                    .collect::<std::result::Result<_, _>>()
                    .map_err(sql)?
            };
            let trash = self.root.join("assets").join(TRASH).join(run);
            for name in names {
                match std::fs::remove_file(trash.join(&name)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err("GC_UNLINK_IO".into()),
                }
                self.connection
                    .execute(
                        "UPDATE gc_tombstones SET state='unlinked' WHERE run=?1 AND name=?2",
                        params![run, name],
                    )
                    .map_err(sql)?;
            }
            let _ = std::fs::remove_dir(&trash);
            self.connection
                .execute(
                    "UPDATE gc_runs SET state='completed',completed_ms=?2 WHERE id=?1",
                    params![run, now_ms() as i64],
                )
                .map_err(sql)?;
        }
        Ok(runs)
    }

    /// Preview, or with `execute` delete, unreferenced store files. Execution
    /// recomputes reachability inside an immediate write transaction (the
    /// storage lock), rechecks leases and recovery, and refuses while blocked.
    pub fn prune_assets(&mut self, policy: &PrunePolicy, execute: bool) -> Result<PruneReceipt> {
        if !execute {
            let census = self.asset_census()?;
            let candidates = candidates(&census, policy);
            return Ok(PruneReceipt {
                run: None,
                executed: false,
                candidate_bytes: candidates.iter().map(|c| c.size).sum(),
                candidates,
                unlinked: Vec::new(),
                unlink_failures: Vec::new(),
                blockers: census.blockers(),
                finished_interrupted_runs: Vec::new(),
            });
        }
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let finished = self.finish_interrupted()?;
        let run = uuid::Uuid::new_v4();
        let trash_root = self.root.join("assets").join(TRASH);
        let trash = trash_root.join(run.to_string());
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let census = census_in(&tx, &self.root)?;
        let blockers = census.blockers();
        if !blockers.is_empty() {
            drop(tx);
            return Err(blockers.join("; "));
        }
        let candidates = candidates(&census, policy);
        if candidates.is_empty() {
            drop(tx);
            return Ok(PruneReceipt {
                run: None,
                executed: true,
                candidates,
                candidate_bytes: 0,
                unlinked: Vec::new(),
                unlink_failures: Vec::new(),
                blockers,
                finished_interrupted_runs: finished,
            });
        }
        crate::private_directory(&trash)?;
        tx.execute(
            "INSERT INTO gc_runs(id,created_ms,policy,state) VALUES(?1,?2,?3,'tombstoned')",
            params![
                run.to_string(),
                policy.now_ms as i64,
                serde_json::to_vec(policy).map_err(|e| e.to_string())?
            ],
        )
        .map_err(sql)?;
        let assets = self.root.join("assets");
        let mut moved = Vec::new();
        let outcome = (|| -> Result<()> {
            for candidate in &candidates {
                tx.execute(
                    "INSERT INTO gc_tombstones(run,name,kind,size,modified_ms,state) VALUES(?1,?2,?3,?4,?5,'tombstoned')",
                    params![
                        run.to_string(),
                        candidate.name,
                        match candidate.kind {
                            FileKind::Asset => "asset",
                            FileKind::Stray => "stray",
                            _ => "temporary",
                        },
                        candidate.size as i64,
                        candidate.modified_ms as i64
                    ],
                )
                .map_err(sql)?;
                if candidate.kind == FileKind::Asset {
                    tx.execute("DELETE FROM assets WHERE digest=?1", [&candidate.name])
                        .map_err(sql)?;
                }
                std::fs::rename(assets.join(&candidate.name), trash.join(&candidate.name))
                    .map_err(|_| "GC_TRASH_IO")?;
                moved.push(candidate.name.clone());
            }
            crate::sync_dir(&assets)?;
            crate::sync_dir(&trash)
        })();
        if let Err(error) = outcome {
            // Undo the moves; the transaction rolls back the tombstones.
            for name in moved {
                let _ = std::fs::rename(trash.join(&name), assets.join(&name));
            }
            let _ = std::fs::remove_dir(&trash);
            return Err(error);
        }
        if let Err(error) = tx.commit() {
            for name in moved {
                let _ = std::fs::rename(trash.join(&name), assets.join(&name));
            }
            let _ = std::fs::remove_dir(&trash);
            return Err(sql(error));
        }
        let mut unlinked = Vec::new();
        let mut failures = Vec::new();
        for candidate in &candidates {
            match std::fs::remove_file(trash.join(&candidate.name)) {
                Ok(()) => {
                    self.connection
                        .execute(
                            "UPDATE gc_tombstones SET state='unlinked' WHERE run=?1 AND name=?2",
                            params![run.to_string(), candidate.name],
                        )
                        .map_err(sql)?;
                    unlinked.push(candidate.name.clone());
                }
                Err(_) => failures.push(candidate.name.clone()),
            }
        }
        if failures.is_empty() {
            let _ = std::fs::remove_dir(&trash);
            self.connection
                .execute(
                    "UPDATE gc_runs SET state='completed',completed_ms=?2 WHERE id=?1",
                    params![run.to_string(), now_ms() as i64],
                )
                .map_err(sql)?;
        }
        Ok(PruneReceipt {
            run: Some(run),
            executed: true,
            candidate_bytes: candidates.iter().map(|c| c.size).sum(),
            candidates,
            unlinked,
            unlink_failures: failures,
            blockers,
            finished_interrupted_runs: finished,
        })
    }

    /// Recorded prune runs, newest first.
    pub fn gc_runs(&self, limit: u32) -> Result<Vec<serde_json::Value>> {
        let mut statement = self
            .connection
            .prepare("SELECT id,created_ms,state,completed_ms,(SELECT COUNT(*) FROM gc_tombstones t WHERE t.run=r.id),(SELECT COALESCE(SUM(size),0) FROM gc_tombstones t WHERE t.run=r.id) FROM gc_runs r ORDER BY created_ms DESC,id LIMIT ?1")
            .map_err(sql)?;
        statement
            .query_map([limit], |r| {
                Ok(serde_json::json!({
                    "run": r.get::<_, String>(0)?,
                    "created_ms": r.get::<_, i64>(1)?,
                    "state": r.get::<_, String>(2)?,
                    "completed_ms": r.get::<_, Option<i64>>(3)?,
                    "files": r.get::<_, i64>(4)?,
                    "bytes": r.get::<_, i64>(5)?,
                }))
            })
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Path of the trash directory for diagnostics.
pub fn trash_dir(root: &Path) -> PathBuf {
    root.join("assets").join(TRASH)
}

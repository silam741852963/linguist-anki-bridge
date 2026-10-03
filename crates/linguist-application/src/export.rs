//! Stream a checksummed portable inspection bundle; no collection or approval import.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use linguist_core::{
    canonical,
    records::{MediaRole, PlanRevision},
    render::RenderedNote,
};
use linguist_store::Store;
use schemars::JsonSchema;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};
#[derive(Serialize, JsonSchema)]
pub struct ExportAsset {
    pub digest: String,
    pub size_bytes: u64,
}
#[derive(Serialize, JsonSchema)]
pub struct ExportManifest {
    pub schema_version: u16,
    pub format: String,
    pub plan_id: uuid::Uuid,
    pub revision: u32,
    pub source_plan_digest: String,
    pub includes_private_archives: bool,
    pub contains_private_card_content: bool,
    pub apply_authorized: bool,
    pub rendered: Vec<RenderedNote>,
    pub private_plan: Option<PlanRevision>,
    pub assets: Vec<ExportAsset>,
    pub asset_encoding: String,
}
#[derive(Debug, Serialize)]
pub struct ExportReceipt {
    pub schema_version: u16,
    pub path: std::path::PathBuf,
    pub checksum: String,
    pub manifest_digest: String,
    pub size_bytes: u64,
    pub includes_private_archives: bool,
    pub apply_authorized: bool,
}
struct CheckedWriter {
    file: File,
    hash: Sha256,
    size: u64,
}
impl Write for CheckedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let count = self.file.write(bytes)?;
        self.hash.update(&bytes[..count]);
        self.size += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
/// Resolve and check a create-new destination whose parent directory exists.
fn destination(path: &Path) -> Result<std::path::PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|_| "EXPORT_DIRECTORY_IO")?
            .join(path)
    };
    let parent = path.parent().ok_or("EXPORT_PARENT_IO")?;
    if !std::fs::metadata(parent)
        .map_err(|_| "EXPORT_PARENT_IO")?
        .is_dir()
    {
        return Err("EXPORT_PARENT_IO".into());
    }
    match std::fs::symlink_metadata(&path) {
        Ok(_) => return Err("EXPORT_DESTINATION_CONFLICT: output already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("EXPORT_DESTINATION_IO".into()),
    }
    Ok(path)
}

fn asset_list(store: &Store, digests: BTreeSet<String>) -> Result<Vec<ExportAsset>, String> {
    digests
        .into_iter()
        .map(|digest| {
            let size_bytes = store.asset(&digest, 100 * 1024 * 1024)?.len() as u64;
            Ok(ExportAsset { digest, size_bytes })
        })
        .collect()
}

/// Stream `{manifest, manifest_digest, asset_data}` to a private temporary file,
/// sync it, then publish with a no-overwrite hard link.
fn write_bundle<M: Serialize>(
    store: &Store,
    path: &Path,
    manifest: &M,
    domain: &str,
    assets: &[ExportAsset],
    include_private_archives: bool,
) -> Result<ExportReceipt, String> {
    let parent = path.parent().ok_or("EXPORT_PARENT_IO")?;
    let manifest_digest = canonical::digest(domain, manifest).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".lab-export-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<ExportReceipt, String> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&temp).map_err(|_| "EXPORT_TEMP_IO")?;
        let mut writer = CheckedWriter {
            file,
            hash: Sha256::new(),
            size: 0,
        };
        writer
            .write_all(b"{\"manifest\":")
            .map_err(|_| "EXPORT_WRITE_IO")?;
        serde_json::to_writer(&mut writer, manifest).map_err(|_| "EXPORT_WRITE_IO")?;
        writer
            .write_all(b",\"manifest_digest\":")
            .map_err(|_| "EXPORT_WRITE_IO")?;
        serde_json::to_writer(&mut writer, &manifest_digest).map_err(|_| "EXPORT_WRITE_IO")?;
        writer
            .write_all(b",\"asset_data\":[")
            .map_err(|_| "EXPORT_WRITE_IO")?;
        for (index, asset) in assets.iter().enumerate() {
            if index > 0 {
                writer.write_all(b",").map_err(|_| "EXPORT_WRITE_IO")?;
            }
            let bytes = store.asset(&asset.digest, 100 * 1024 * 1024)?;
            if bytes.len() as u64 != asset.size_bytes {
                return Err("EXPORT_ASSET_CONFLICT".into());
            }
            serde_json::to_writer(
                &mut writer,
                &serde_json::json!({"digest":asset.digest,"data":STANDARD.encode(bytes)}),
            )
            .map_err(|_| "EXPORT_WRITE_IO")?;
        }
        writer.write_all(b"]}\n").map_err(|_| "EXPORT_WRITE_IO")?;
        writer.file.sync_all().map_err(|_| "EXPORT_SYNC_IO")?;
        let checksum = format!("{:x}", writer.hash.finalize());
        std::fs::hard_link(&temp, path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "EXPORT_DESTINATION_CONFLICT"
            } else {
                "EXPORT_PUBLISH_IO"
            }
        })?;
        File::open(parent).and_then(|directory|directory.sync_all()).map_err(|_|"EXPORT_DIRECTORY_SYNC_FAILED: output published but directory durability is unconfirmed")?;
        Ok(ExportReceipt {
            schema_version: 2,
            path: path.to_owned(),
            checksum,
            manifest_digest,
            size_bytes: writer.size,
            includes_private_archives: include_private_archives,
            apply_authorized: false,
        })
    })();
    let _ = std::fs::remove_file(temp);
    result
}

pub fn export_plan(
    store: &Store,
    plan: &PlanRevision,
    path: &Path,
    include_private_archives: bool,
) -> Result<ExportReceipt, String> {
    let path = destination(path)?;
    // Re-read the immutable record and complete archive graph before disclosing anything.
    let verified = store.revision(plan.id, plan.revision)?;
    if verified != *plan {
        return Err("EXPORT_PLAN_CONFLICT".into());
    }
    let digests: BTreeSet<_> = plan
        .documents
        .iter()
        .flat_map(|document| {
            document
                .media
                .iter()
                .filter(|asset| include_private_archives || asset.role != MediaRole::Archive)
                .map(|asset| asset.digest.clone())
                .chain(
                    document
                        .archives
                        .iter()
                        .filter(|_| include_private_archives)
                        .flat_map(|archive| archive.asset_digests.clone()),
                )
        })
        .collect();
    let assets = asset_list(store, digests)?;
    let manifest = ExportManifest {
        schema_version: 2,
        format: "lab-plan-bundle-v2".into(),
        plan_id: plan.id,
        revision: plan.revision,
        source_plan_digest: plan.approval_digest().map_err(|e| e.to_string())?,
        includes_private_archives: include_private_archives,
        contains_private_card_content: true,
        apply_authorized: false,
        rendered: plan.rendered.clone(),
        private_plan: include_private_archives.then(|| plan.clone()),
        assets,
        asset_encoding: "base64-standard-padded".into(),
    };
    write_bundle(
        store,
        &path,
        &manifest,
        "plan-export-manifest",
        &manifest.assets,
        include_private_archives,
    )
}

#[derive(Serialize, JsonSchema)]
pub struct SnapshotExportManifest {
    pub schema_version: u16,
    pub format: String,
    pub snapshot: linguist_core::records::Snapshot,
    pub after: Option<linguist_core::records::NativeOperationReceipt>,
    pub post_state_status: String,
    /// Snapshots hold original note content; disclosure is always private.
    pub contains_private_note_content: bool,
    /// A snapshot is recoverable evidence for one operation, not a collection backup.
    pub full_collection_backup: bool,
    pub apply_authorized: bool,
    pub assets: Vec<ExportAsset>,
    pub asset_encoding: String,
}

/// OP-51: export one immutable snapshot with every linked original asset.
pub fn export_snapshot(
    store: &Store,
    snapshot: uuid::Uuid,
    path: &Path,
) -> Result<ExportReceipt, String> {
    let path = destination(path)?;
    let record = store.snapshot(snapshot)?;
    let digests: BTreeSet<String> = record
        .snapshot
        .archives
        .iter()
        .flat_map(|archive| archive.asset_digests.iter().cloned())
        .chain(
            record
                .snapshot
                .media
                .iter()
                .map(|asset| asset.digest.clone()),
        )
        .collect();
    let assets = asset_list(store, digests)?;
    let manifest = SnapshotExportManifest {
        schema_version: 2,
        format: "lab-snapshot-bundle-v2".into(),
        snapshot: record.snapshot,
        after: record.after,
        post_state_status: record.post_state_status,
        contains_private_note_content: true,
        full_collection_backup: false,
        apply_authorized: false,
        assets,
        asset_encoding: "base64-standard-padded".into(),
    };
    write_bundle(
        store,
        &path,
        &manifest,
        "snapshot-export-manifest",
        &manifest.assets,
        true,
    )
}

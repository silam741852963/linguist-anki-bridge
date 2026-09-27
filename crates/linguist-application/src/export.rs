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
pub fn export_plan(
    store: &Store,
    plan: &PlanRevision,
    path: &Path,
    include_private_archives: bool,
) -> Result<ExportReceipt, String> {
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
    let mut assets = Vec::new();
    for digest in digests {
        let bytes = store.asset(&digest, 100 * 1024 * 1024)?;
        assets.push(ExportAsset {
            digest,
            size_bytes: bytes.len() as u64,
        });
    }
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
    let manifest_digest =
        canonical::digest("plan-export-manifest", &manifest).map_err(|e| e.to_string())?;
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
        serde_json::to_writer(&mut writer, &manifest).map_err(|_| "EXPORT_WRITE_IO")?;
        writer
            .write_all(b",\"manifest_digest\":")
            .map_err(|_| "EXPORT_WRITE_IO")?;
        serde_json::to_writer(&mut writer, &manifest_digest).map_err(|_| "EXPORT_WRITE_IO")?;
        writer
            .write_all(b",\"asset_data\":[")
            .map_err(|_| "EXPORT_WRITE_IO")?;
        for (index, asset) in manifest.assets.iter().enumerate() {
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
        std::fs::hard_link(&temp, &path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "EXPORT_DESTINATION_CONFLICT"
            } else {
                "EXPORT_PUBLISH_IO"
            }
        })?;
        File::open(parent).and_then(|directory|directory.sync_all()).map_err(|_|"EXPORT_DIRECTORY_SYNC_FAILED: output published but directory durability is unconfirmed")?;
        Ok(ExportReceipt {
            schema_version: 2,
            path: path.clone(),
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

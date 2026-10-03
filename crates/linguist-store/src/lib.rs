//! Local durable archives and immutable revisions. This store never calls Anki.
use linguist_core::{canonical, records::PlanRevision};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, String>;
const APPLICATION_ID: i64 = 0x4c414232;
const SCHEMA: i64 = 9;
fn sql(e: rusqlite::Error) -> String {
    format!("STORE_SQL: {e}")
}
fn private_options() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    o
}
fn sync_dir(root: &Path) -> Result<()> {
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| "STORE_DIRECTORY_SYNC_FAILED".into())
}
fn private_directory(root: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(root).map_err(|_| "STORE_DIRECTORY_IO")?;
    let meta = std::fs::symlink_metadata(root).map_err(|_| "STORE_DIRECTORY_IO")?;
    if !meta.is_dir() || meta.is_symlink() {
        return Err("STORE_UNSAFE_DIRECTORY".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("STORE_DIRECTORY_NOT_PRIVATE".into());
        }
    }
    Ok(())
}
#[derive(Debug, Serialize, Deserialize)]
pub struct RevisionSummary {
    pub id: uuid::Uuid,
    pub revision: u32,
    pub digest: String,
}
pub struct Store {
    writable: bool,
    root: PathBuf,
    connection: Connection,
}
impl Store {
    /// Open existing state without schema creation or migration.
    pub fn read_only(root: &Path) -> Result<Self> {
        if !root.is_absolute() {
            return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
        }
        let meta = std::fs::symlink_metadata(root).map_err(|_| "STORE_NOT_FOUND")?;
        if !meta.is_dir() || meta.is_symlink() {
            return Err("STORE_UNSAFE_DIRECTORY".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err("STORE_DIRECTORY_NOT_PRIVATE".into());
            }
        }
        check_filesystem(root)?;
        let dbmeta = std::fs::symlink_metadata(root.join("state.sqlite3"))
            .map_err(|_| "STORE_DATABASE_IO")?;
        if !dbmeta.is_file() || dbmeta.is_symlink() {
            return Err("STORE_UNSAFE_DATABASE".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if dbmeta.permissions().mode() & 0o077 != 0 {
                return Err("STORE_DATABASE_NOT_PRIVATE".into());
            }
        }
        let connection = Connection::open_with_flags(
            root.join("state.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(sql)?;
        let app: i64 = connection
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(sql)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(sql)?;
        if app != APPLICATION_ID || version != SCHEMA {
            return Err("STORE_SCHEMA_UNSUPPORTED_OR_INITIALIZATION_INCOMPLETE".into());
        }
        connection
            .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
            .map_err(sql)?;
        Ok(Self {
            root: root.to_owned(),
            connection,
            writable: false,
        })
    }
    pub fn latest_revision(&self, id: uuid::Uuid) -> Result<u32> {
        self.connection
            .query_row(
                "SELECT revision FROM revisions WHERE id=?1 ORDER BY revision DESC LIMIT 1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?
            .ok_or("PLAN_NOT_FOUND".into())
    }
    /// Opens or creates a private local store. Read-only CLI handlers must check existence first.
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_inner(root, true)
    }
    /// Upgrade/open existing state without creating a missing database.
    pub fn open_existing(root: &Path) -> Result<Self> {
        Self::open_inner(root, false)
    }
    fn open_inner(root: &Path, allow_create: bool) -> Result<Self> {
        if !root.is_absolute() {
            return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
        }
        if !allow_create {
            std::fs::symlink_metadata(root).map_err(|_| "STORE_NOT_FOUND")?;
        }
        private_directory(root)?;
        check_filesystem(root)?;
        let db = root.join("state.sqlite3");
        let new = if !allow_create {
            false
        } else {
            match private_options().open(&db) {
                Ok(f) => {
                    f.sync_all().map_err(|_| "STORE_CREATE_SYNC")?;
                    sync_dir(root)?;
                    true
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
                Err(_) => return Err("STORE_CREATE_IO".into()),
            }
        };
        let meta = std::fs::symlink_metadata(&db).map_err(|_| "STORE_DATABASE_IO")?;
        if !meta.is_file() || meta.is_symlink() {
            return Err("STORE_UNSAFE_DATABASE".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err("STORE_DATABASE_NOT_PRIVATE".into());
            }
        }
        let mut connection = Connection::open_with_flags(
            &db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(sql)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(sql)?;
        let application: i64 = connection
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(sql)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(sql)?;
        if !new && (application != APPLICATION_ID || !(1..=SCHEMA).contains(&version)) {
            return Err("STORE_SCHEMA_UNSUPPORTED_OR_INITIALIZATION_INCOMPLETE".into());
        }
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;").map_err(sql)?;
        if !new && version < SCHEMA {
            let backup = root.join(format!(
                ".schema-v{version}-{}.sqlite3",
                uuid::Uuid::new_v4()
            ));
            let file = private_options()
                .open(&backup)
                .map_err(|_| "STORE_MIGRATION_BACKUP_IO")?;
            connection
                .backup(rusqlite::MAIN_DB, &backup, None)
                .map_err(sql)?;
            file.sync_all().map_err(|_| "STORE_MIGRATION_BACKUP_SYNC")?;
            sync_dir(root)?;
            let checked = Connection::open_with_flags(
                &backup,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )
            .map_err(sql)?;
            checked
                .execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
                .map_err(sql)?;
            let check: String = checked
                .pragma_query_value(None, "integrity_check", |r| r.get(0))
                .map_err(sql)?;
            if check != "ok" {
                return Err("STORE_MIGRATION_BACKUP_INVALID".into());
            }
            drop(checked);
            file.sync_all().map_err(|_| "STORE_MIGRATION_BACKUP_SYNC")?;
            sync_dir(root)?;
            let tx = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(sql)?;
            if version == 1 {
                tx.execute_batch(journal::SCHEMA_SQL).map_err(sql)?;
            }
            if version < 3 {
                tx.execute_batch(lease::SCHEMA_SQL).map_err(sql)?;
            }
            if version < 4 {
                tx.execute_batch(validation::SCHEMA_SQL).map_err(sql)?;
            }
            if version < 5 {
                tx.execute_batch(approval::SCHEMA_SQL).map_err(sql)?;
            }
            if version < 6 {
                tx.execute_batch(preparation::SCHEMA_SQL).map_err(sql)?;
            }
            if version < 7 {
                tx.execute_batch(preparation_control::SCHEMA_SQL)
                    .map_err(sql)?;
            }
            tx.execute_batch(snapshot::SCHEMA_SQL).map_err(sql)?;
            tx.execute_batch(checkpoint::SCHEMA_SQL).map_err(sql)?;
            tx.pragma_update(None, "user_version", SCHEMA)
                .map_err(sql)?;
            tx.commit().map_err(sql)?;
            sync_dir(root)?;
        }
        if new {
            let tx = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(sql)?;
            tx.execute_batch("CREATE TABLE assets(digest TEXT PRIMARY KEY,size INTEGER NOT NULL CHECK(size>0)); CREATE TABLE revisions(id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), digest TEXT NOT NULL, body_digest TEXT NOT NULL, body BLOB NOT NULL, PRIMARY KEY(id,revision)); CREATE TABLE revision_assets(id TEXT NOT NULL,revision INTEGER NOT NULL,digest TEXT NOT NULL REFERENCES assets(digest),PRIMARY KEY(id,revision,digest),FOREIGN KEY(id,revision) REFERENCES revisions(id,revision));").map_err(sql)?;
            tx.execute_batch(journal::SCHEMA_SQL).map_err(sql)?;
            tx.execute_batch(lease::SCHEMA_SQL).map_err(sql)?;
            tx.execute_batch(validation::SCHEMA_SQL).map_err(sql)?;
            tx.execute_batch(approval::SCHEMA_SQL).map_err(sql)?;
            tx.execute_batch(preparation::SCHEMA_SQL).map_err(sql)?;
            tx.execute_batch(preparation_control::SCHEMA_SQL)
                .map_err(sql)?;
            tx.execute_batch(snapshot::SCHEMA_SQL).map_err(sql)?;
            tx.execute_batch(checkpoint::SCHEMA_SQL).map_err(sql)?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(sql)?;
            tx.pragma_update(None, "user_version", SCHEMA)
                .map_err(sql)?;
            tx.commit().map_err(sql)?;
            sync_dir(root)?;
        }
        private_directory(&root.join("assets"))?;
        Ok(Self {
            root: root.to_owned(),
            connection,
            writable: true,
        })
    }
    pub fn publish_asset(&mut self, bytes: &[u8], max_bytes: u64) -> Result<String> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if bytes.is_empty() || bytes.len() as u64 > max_bytes.min(100 * 1024 * 1024) {
            return Err("ASSET_SIZE_LIMIT".into());
        }
        let digest = canonical::asset_digest(bytes);
        let root = self.root.join("assets");
        let destination = root.join(&digest);
        let temp = root.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
        let outcome = (|| -> Result<()> {
            let mut f = private_options().open(&temp).map_err(|_| "ASSET_TEMP_IO")?;
            f.write_all(bytes)
                .and_then(|_| f.sync_all())
                .map_err(|_| "ASSET_WRITE_IO")?;
            match std::fs::hard_link(&temp, &destination) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let existing = read_file(&destination, max_bytes)?;
                    if existing != bytes {
                        return Err("ASSET_DIGEST_COLLISION_OR_CORRUPTION".into());
                    }
                }
                Err(_) => return Err("ASSET_PUBLISH_IO".into()),
            }
            sync_dir(&root)?;
            Ok(())
        })();
        let _ = std::fs::remove_file(temp);
        outcome?;
        self.connection
            .execute(
                "INSERT INTO assets(digest,size) VALUES(?1,?2) ON CONFLICT(digest) DO NOTHING",
                params![digest, bytes.len() as i64],
            )
            .map_err(sql)?;
        Ok(digest)
    }
    pub fn asset(&self, digest: &str, max_bytes: u64) -> Result<Vec<u8>> {
        if !digest_valid(digest) {
            return Err("INVALID_ASSET_DIGEST".into());
        }
        let size: Option<i64> = self
            .connection
            .query_row("SELECT size FROM assets WHERE digest=?1", [digest], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)?;
        let size = size.ok_or("ASSET_NOT_FOUND")?;
        if size as u64 > max_bytes {
            return Err("ASSET_SIZE_LIMIT".into());
        }
        let bytes = read_file(&self.root.join("assets").join(digest), max_bytes)?;
        if bytes.len() as i64 != size || canonical::asset_digest(&bytes) != digest {
            return Err("ASSET_CORRUPT".into());
        }
        Ok(bytes)
    }
    pub fn publish_revision(&mut self, plan: &PlanRevision) -> Result<String> {
        self.publish_revision_inner(plan, None)
    }
    pub(crate) fn publish_revision_inner(
        &mut self,
        plan: &PlanRevision,
        worker: Option<&lease::LeaseToken>,
    ) -> Result<String> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if plan.schema_version != 2
            || plan.revision == 0
            || plan
                .documents
                .iter()
                .map(|d| d.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != plan.documents.len()
        {
            return Err("INVALID_PLAN_REVISION".into());
        }
        let mut rendered_ids = std::collections::BTreeSet::new();
        for rendered in &plan.rendered {
            if !rendered_ids.insert(rendered.document_id) {
                return Err("DUPLICATE_RENDERED_DOCUMENT".into());
            }
            let doc = plan
                .documents
                .iter()
                .find(|d| d.id == rendered.document_id)
                .ok_or("RENDERED_DOCUMENT_MISSING")?;
            let empty = std::collections::BTreeMap::new();
            let fields = doc.sources.first().map(|s| &s.fields).unwrap_or(&empty);
            let expected = linguist_core::render::render(doc, fields)
                .map_err(|_| "RENDERED_DOCUMENT_NOT_READY")?;
            if &expected != rendered {
                return Err("RENDERED_DOCUMENT_STALE".into());
            }
        }
        let digest = plan.approval_digest().map_err(|e| e.to_string())?;
        let body = canonical::bytes(plan).map_err(|e| e.to_string())?;
        let body_digest = canonical::asset_digest(&body);
        // Verify original archives as well as rendered media before committing references.
        let assets = self.verify_revision_assets(plan)?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        if let Some(worker) = worker {
            lease::validate_job_worker_token(&tx, worker, plan.id)?;
            if preparation_control::stops_dispatch(&tx, plan.id)? {
                return Err("PREPARATION_CONTROL_BLOCKS_PUBLICATION".into());
            }
        }
        let previous: Option<(u32, String)> = tx
            .query_row(
                "SELECT revision,digest FROM revisions WHERE id=?1 ORDER BY revision DESC LIMIT 1",
                [plan.id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        match previous {
            None if plan.revision == 1 && plan.parent_digest.is_none() => {}
            Some((revision, parent))
                if plan.revision == revision + 1
                    && plan.parent_digest.as_ref() == Some(&parent) => {}
            _ => {
                return Err(
                    "REVISION_CONFLICT: parent or revision does not match immutable history".into(),
                );
            }
        }
        tx.execute(
            "INSERT INTO revisions(id,revision,digest,body_digest,body) VALUES(?1,?2,?3,?4,?5)",
            params![
                plan.id.to_string(),
                plan.revision,
                digest,
                body_digest,
                body
            ],
        )
        .map_err(sql)?;
        for asset in assets {
            tx.execute(
                "INSERT INTO revision_assets(id,revision,digest) VALUES(?1,?2,?3)",
                params![plan.id.to_string(), plan.revision, asset],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        Ok(digest)
    }
    pub fn revision(&self, id: uuid::Uuid, revision: u32) -> Result<PlanRevision> {
        let record: Option<(Vec<u8>, String, String)> = self
            .connection
            .query_row(
                "SELECT body,body_digest,digest FROM revisions WHERE id=?1 AND revision=?2",
                params![id.to_string(), revision],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(sql)?;
        let (body, hash, digest) = record.ok_or("PLAN_NOT_FOUND")?;
        if canonical::asset_digest(&body) != hash {
            return Err("PLAN_CORRUPT".into());
        }
        let plan: PlanRevision = canonical::parse(&body).map_err(|_| "PLAN_CORRUPT")?;
        if plan.id != id
            || plan.revision != revision
            || plan.approval_digest().map_err(|_| "PLAN_CORRUPT")? != digest
        {
            return Err("PLAN_CORRUPT".into());
        }
        let assets = self.verify_revision_assets(&plan)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT digest FROM revision_assets WHERE id=?1 AND revision=?2 ORDER BY digest",
            )
            .map_err(sql)?;
        let indexed: std::collections::BTreeSet<String> = statement
            .query_map(params![id.to_string(), revision], |r| r.get(0))
            .map_err(sql)?
            .map(|r| r.map_err(sql))
            .collect::<Result<_>>()?;
        if assets != indexed {
            return Err("REVISION_ASSET_INDEX_CORRUPT".into());
        }
        Ok(plan)
    }
    fn verify_revision_assets(
        &self,
        plan: &PlanRevision,
    ) -> Result<std::collections::BTreeSet<String>> {
        self.verify_document_assets(&plan.documents)
    }
    fn verify_document_assets(
        &self,
        documents: &[linguist_core::LearningDocument],
    ) -> Result<std::collections::BTreeSet<String>> {
        let assets: std::collections::BTreeSet<_> = documents
            .iter()
            .flat_map(|document| {
                document
                    .media
                    .iter()
                    .map(|media| media.digest.clone())
                    .chain(
                        document
                            .archives
                            .iter()
                            .flat_map(|archive| archive.asset_digests.clone()),
                    )
            })
            .collect();
        for digest in &assets {
            self.asset(digest, 100 * 1024 * 1024)?;
        }
        for document in documents {
            for media in &document.media {
                if self.asset(&media.digest, media.size_bytes)?.len() as u64 != media.size_bytes {
                    return Err("ASSET_MANIFEST_SIZE_MISMATCH".into());
                }
            }
        }
        Ok(assets)
    }
    pub fn list_revisions(&self, limit: u32) -> Result<Vec<RevisionSummary>> {
        if !(1..=10000).contains(&limit) {
            return Err("INVALID_PAGE_LIMIT".into());
        }
        let mut statement = self
            .connection
            .prepare("SELECT id,revision,digest FROM revisions ORDER BY id,revision DESC LIMIT ?1")
            .map_err(sql)?;
        let records = statement
            .query_map([limit], |row| {
                Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(sql)?;
        records
            .map(|r| {
                let (id, revision, digest) = r.map_err(sql)?;
                Ok(RevisionSummary {
                    id: uuid::Uuid::parse_str(&id).map_err(|_| "STORE_CORRUPT_ID")?,
                    revision,
                    digest,
                })
            })
            .collect()
    }
}
fn digest_valid(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn read_file(path: &Path, max: u64) -> Result<Vec<u8>> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    let f = o.open(path).map_err(|_| "ASSET_READ_IO")?;
    if !f.metadata().map_err(|_| "ASSET_READ_IO")?.is_file() {
        return Err("ASSET_NOT_REGULAR_FILE".into());
    }
    let mut bytes = Vec::new();
    f.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "ASSET_READ_IO")?;
    if bytes.len() as u64 > max {
        return Err("ASSET_SIZE_LIMIT".into());
    }
    Ok(bytes)
}
#[cfg(target_os = "linux")]
fn check_filesystem(path: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let path =
        std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| "INVALID_STORE_PATH")?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: CString remains alive and stat points to a writable statfs object.
    if unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err("STORE_FILESYSTEM_CHECK_FAILED".into());
    }
    // SAFETY: successful statfs initialized the object.
    let kind = unsafe { stat.assume_init() }.f_type as u64;
    if ![
        0xef53, 0x9123683e, 0x58465342, 0x01021994, 0x794c7630, 0x2fc12fc1, 0xf2f52010,
    ]
    .contains(&kind)
    {
        return Err("STORE_FILESYSTEM_UNSUPPORTED: verified local filesystem required".into());
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn check_filesystem(_: &Path) -> Result<()> {
    Err("STORE_PLATFORM_UNAVAILABLE: initial supported platform is Linux".into())
}

pub mod journal;

pub mod lease;

pub mod validation;

pub mod approval;
pub mod checkpoint;
pub mod preparation;
pub mod preparation_control;
pub mod snapshot;

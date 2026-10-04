//! WP-14 local maintenance: pinned resource installation, cache pruning and
//! legacy job import. No test contacts a real service or collection.
use linguist_application::{cache, legacy_jobs, resources};
use linguist_config::{ConfigFile, Effective, Registry, ResolveOptions, resolve};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("lab-maint-app-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn settings(dir: &Path, flags: &[(&str, Value)]) -> Effective {
    let mut map = BTreeMap::from([
        (
            "storage.resource_dir".to_owned(),
            json!(dir.join("resources")),
        ),
        ("storage.cache_dir".to_owned(), json!(dir.join("cache"))),
        ("storage.state_dir".to_owned(), json!(dir.join("state"))),
        ("storage.temp_dir".to_owned(), json!(dir.join("tmp"))),
        ("storage.free_space_reserve_mb".to_owned(), json!(64)),
    ]);
    for (k, v) in flags {
        map.insert((*k).to_owned(), v.clone());
    }
    resolve(
        &Registry::builtin(),
        &ConfigFile::default(),
        &ResolveOptions {
            flags: map,
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

fn env() -> BTreeMap<String, String> {
    BTreeMap::new()
}

fn sha(bytes: &[u8]) -> String {
    linguist_core::canonical::asset_digest(bytes)
}

fn request(resource: &str, source: &str, digest: &str, execute: bool) -> resources::InstallRequest {
    resources::InstallRequest {
        resource: resource.into(),
        source: source.into(),
        version: "4.1.0".into(),
        sha256: digest.into(),
        license: "Apache-2.0".into(),
        destination: None,
        execute,
    }
}

fn zip(entries: &[(&str, &[u8], Option<u32>)], symlink: Option<(&str, &str)>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes, mode) in entries {
        let mut options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if let Some(mode) = mode {
            options = options.unix_permissions(*mode);
        }
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    if let Some((name, target)) = symlink {
        writer
            .add_symlink(name, target, zip::write::SimpleFileOptions::default())
            .unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn pinned_local_pack_installs_once_with_a_verified_receipt() {
    let dir = Dir::new();
    let s = settings(&dir.0, &[]);
    let pack = dir.0.join("jpn.traineddata");
    std::fs::write(&pack, b"tesseract pack bytes").unwrap();
    let digest = sha(b"tesseract pack bytes");
    let source = pack.to_str().unwrap();
    let preview = resources::install(
        &s,
        &env(),
        &request("tesseract:jpn", source, &digest, false),
    )
    .unwrap();
    assert_eq!(preview["executed"], false);
    assert!(!dir.0.join("resources").exists(), "preview writes nothing");
    let receipt =
        resources::install(&s, &env(), &request("tesseract:jpn", source, &digest, true)).unwrap();
    assert_eq!(receipt["executed"], true);
    let installed = dir.0.join("resources/tessdata/jpn.traineddata");
    assert_eq!(std::fs::read(&installed).unwrap(), b"tesseract pack bytes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&installed).unwrap().permissions().mode() & 0o111,
            0,
            "never executable"
        );
    }
    assert_eq!(receipt["receipt"]["license"], "Apache-2.0");
    assert_eq!(receipt["receipt"]["sha256"], digest);
    assert_eq!(receipt["receipt"]["downloaded"], false);
    // The same pinned request is an idempotent no-op; another version never overwrites.
    let again =
        resources::install(&s, &env(), &request("tesseract:jpn", source, &digest, true)).unwrap();
    assert_eq!(again["already_installed"], true);
    let mut newer = request("tesseract:jpn", source, &digest, true);
    newer.version = "5.0.0".into();
    assert!(
        resources::install(&s, &env(), &newer)
            .unwrap_err()
            .starts_with("RESOURCE_DESTINATION_EXISTS")
    );
    let listed = resources::list(&s, &env(), None, false, false).unwrap();
    assert_eq!(listed["installed"][0]["status"], "verified");
    assert_eq!(listed["installed"][0]["resource"], "tesseract:jpn");
    assert!(
        listed["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["resource"] == "tesseract:jpn" && r["installed_in_resource_dir"] == true)
    );
    std::fs::write(&installed, b"tampered").unwrap();
    let listed = resources::list(&s, &env(), Some("tesseract:"), true, false).unwrap();
    assert_eq!(listed["installed"][0]["status"], "modified");
    assert_eq!(listed["required"], Value::Null);
}

#[test]
fn checksum_license_type_and_destination_are_checked_before_install() {
    let dir = Dir::new();
    let s = settings(&dir.0, &[]);
    let pack = dir.0.join("eng.traineddata");
    std::fs::write(&pack, b"english").unwrap();
    let source = pack.to_str().unwrap();
    let wrong = sha(b"something else");
    let error = resources::install(&s, &env(), &request("tesseract:eng", source, &wrong, true))
        .unwrap_err();
    assert!(error.starts_with("RESOURCE_CHECKSUM_MISMATCH"), "{error}");
    assert!(!dir.0.join("resources/tessdata/eng.traineddata").exists());
    assert!(
        std::fs::read_dir(dir.0.join("resources"))
            .unwrap()
            .all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".staging")),
        "staging removed"
    );
    let mut unlicensed = request("tesseract:eng", source, &sha(b"english"), true);
    unlicensed.license = "Proprietary".into();
    assert!(
        resources::install(&s, &env(), &unlicensed)
            .unwrap_err()
            .starts_with("RESOURCE_LICENSE_UNRECOGNIZED")
    );
    assert!(
        resources::install(
            &s,
            &env(),
            &request("tesseract:jpn", source, &sha(b"english"), true)
        )
        .unwrap_err()
        .starts_with("RESOURCE_TYPE_MISMATCH")
    );
    assert!(
        resources::install(&s, &env(), &request("tesseract:eng", source, "abc", true))
            .unwrap_err()
            .starts_with("RESOURCE_SHA256_INVALID")
    );
    let mut outside = request("file:notes", source, &sha(b"english"), true);
    outside.destination = Some(dir.0.join("elsewhere"));
    assert!(
        resources::install(&s, &env(), &outside)
            .unwrap_err()
            .starts_with("RESOURCE_DESTINATION_OUTSIDE_RESOURCE_DIR")
    );
    let small = settings(&dir.0, &[("resources.max_download_mb", json!(1))]);
    let big = dir.0.join("big.bin");
    std::fs::write(&big, vec![0u8; 1024 * 1024 + 1]).unwrap();
    let error = resources::install(
        &small,
        &env(),
        &request(
            "file:big",
            big.to_str().unwrap(),
            &sha(&vec![0u8; 1024 * 1024 + 1]),
            true,
        ),
    )
    .unwrap_err();
    assert!(error.starts_with("RESOURCE_DOWNLOAD_LIMIT"), "{error}");
}

#[test]
fn offline_and_unlisted_hosts_block_downloads_before_any_request() {
    let dir = Dir::new();
    let offline = settings(&dir.0, &[("network.offline", json!(true))]);
    let digest = sha(b"x");
    let error = resources::install(
        &offline,
        &env(),
        &request(
            "tesseract:jpn",
            "https://github.com/tesseract-ocr/tessdata/raw/main/jpn.traineddata",
            &digest,
            false,
        ),
    )
    .unwrap_err();
    assert!(error.starts_with("RESOURCE_OFFLINE"), "{error}");
    let error = resources::install(
        &offline,
        &env(),
        &request("ollama:gemma3:12b", "ollama-registry", &digest, false),
    )
    .unwrap_err();
    assert!(error.starts_with("RESOURCE_OFFLINE"), "{error}");
    let online = settings(&dir.0, &[]);
    let error = resources::install(
        &online,
        &env(),
        &request("file:x", "https://example.org/x.bin", &digest, false),
    )
    .unwrap_err();
    assert!(error.starts_with("RESOURCE_HOST_NOT_ALLOWED"), "{error}");
    let error = resources::install(
        &online,
        &env(),
        &request("file:x", "https://user:pw@github.com/x.bin", &digest, false),
    )
    .unwrap_err();
    assert!(error.starts_with("RESOURCE_SOURCE_CREDENTIALS"), "{error}");
    let preview = resources::install(
        &online,
        &env(),
        &request(
            "tesseract:jpn",
            "https://github.com/tesseract-ocr/tessdata/raw/main/jpn.traineddata",
            &digest,
            false,
        ),
    )
    .unwrap();
    assert_eq!(preview["network"], true);
    assert_eq!(preview["executed"], false);
}

#[test]
fn unsafe_archives_are_refused_and_safe_ones_extract_without_exec_bits() {
    let dir = Dir::new();
    let s = settings(&dir.0, &[]);
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "traversal",
            zip(&[("../evil.txt", b"x", None)], None),
            "RESOURCE_ARCHIVE_UNSAFE_PATH",
        ),
        (
            "absolute",
            zip(&[("/etc/evil", b"x", None)], None),
            "RESOURCE_ARCHIVE_UNSAFE_PATH",
        ),
        (
            "symlink",
            zip(&[("voice.onnx", b"x", None)], Some(("link", "/etc/passwd"))),
            "RESOURCE_ARCHIVE_SYMLINK",
        ),
        (
            "unpacked",
            zip(&[("voice.onnx", &vec![7u8; 2 * 1024 * 1024], None)], None),
            "RESOURCE_UNPACKED_LIMIT",
        ),
    ];
    let limited = settings(&dir.0, &[("resources.max_unpacked_mb", json!(1))]);
    for (name, bytes, code) in cases {
        let path = dir.0.join(format!("{name}.zip"));
        std::fs::write(&path, &bytes).unwrap();
        let s = if name == "unpacked" { &limited } else { &s };
        let error = resources::install(
            s,
            &env(),
            &request(
                &format!("piper:{name}"),
                path.to_str().unwrap(),
                &sha(&bytes),
                true,
            ),
        )
        .unwrap_err();
        assert!(error.starts_with(code), "{name}: {error}");
        assert!(
            !dir.0.join("resources/piper").join(name).exists(),
            "{name}: nothing installed"
        );
    }
    let bytes = zip(
        &[
            ("voice.onnx", b"model", Some(0o755)),
            ("voice.onnx.json", b"{}", None),
            ("install.sh", b"#!/bin/sh\nrm -rf /\n", Some(0o755)),
        ],
        None,
    );
    let path = dir.0.join("voice.zip");
    std::fs::write(&path, &bytes).unwrap();
    let receipt = resources::install(
        &s,
        &env(),
        &request("piper:voice", path.to_str().unwrap(), &sha(&bytes), true),
    )
    .unwrap();
    let install = PathBuf::from(receipt["receipt"]["install_path"].as_str().unwrap());
    assert_eq!(install, dir.0.join("resources/piper/voice/4.1.0"));
    assert_eq!(receipt["receipt"]["files"].as_array().unwrap().len(), 3);
    #[cfg(unix)]
    for file in ["voice.onnx", "install.sh"] {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(install.join(file))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0,
            "{file} is data only"
        );
    }
    assert_eq!(
        std::fs::read(install.join("install.sh")).unwrap(),
        b"#!/bin/sh\nrm -rf /\n"
    );
}

/// Serve canned HTTP responses on loopback, one per accepted connection.
fn serve(
    responses: Vec<(&'static str, Vec<u8>)>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for (head, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 8192];
            let n = stream.read(&mut buffer).unwrap();
            seen.push(
                String::from_utf8_lossy(&buffer[..n])
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            );
            let header = format!(
                "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
        }
        seen
    });
    (address, handle)
}

#[test]
fn loopback_download_is_streamed_hashed_and_redirects_are_policy_checked() {
    let dir = Dir::new();
    let s = settings(&dir.0, &[]);
    let body = b"downloaded voice".to_vec();
    let (address, server) = serve(vec![(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream",
        body.clone(),
    )]);
    let url = format!("{address}/voices/voice.onnx");
    let receipt =
        resources::install(&s, &env(), &request("piper:voice", &url, &sha(&body), true)).unwrap();
    assert_eq!(receipt["receipt"]["downloaded"], true);
    assert_eq!(
        std::fs::read(dir.0.join("resources/piper/voice/4.1.0/voice.onnx")).unwrap(),
        body
    );
    assert_eq!(
        server.join().unwrap(),
        vec!["GET /voices/voice.onnx HTTP/1.1"]
    );
    let (address, server) = serve(vec![(
        "HTTP/1.1 302 Found\r\nLocation: https://example.org/stolen.onnx",
        Vec::new(),
    )]);
    let error = resources::install(
        &s,
        &env(),
        &request(
            "piper:other",
            &format!("{address}/other.onnx"),
            &sha(&body),
            true,
        ),
    )
    .unwrap_err();
    assert!(error.starts_with("RESOURCE_HOST_NOT_ALLOWED"), "{error}");
    server.join().unwrap();
    assert!(!dir.0.join("resources/piper/other").exists());
}

#[test]
fn ollama_models_are_pulled_explicitly_and_digest_verified() {
    let dir = Dir::new();
    let digest = "a".repeat(64);
    let tags =
        json!({"models":[{"name":"gemma3:12b","model":"gemma3:12b","digest":digest,"size":42}]})
            .to_string()
            .into_bytes();
    let (address, server) = serve(vec![
        (
            "HTTP/1.1 200 OK\r\nContent-Type: application/json",
            b"{\"status\":\"success\"}".to_vec(),
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Type: application/json",
            tags.clone(),
        ),
    ]);
    let s = settings(&dir.0, &[("llm.endpoint", json!(address))]);
    let receipt = resources::install(
        &s,
        &env(),
        &request("ollama:gemma3:12b", "ollama-registry", &digest, true),
    )
    .unwrap();
    assert_eq!(receipt["receipt"]["sha256"], digest);
    let seen = server.join().unwrap();
    assert_eq!(
        seen,
        vec!["POST /api/pull HTTP/1.1", "GET /api/tags HTTP/1.1"]
    );
    let (address, server) = serve(vec![
        (
            "HTTP/1.1 200 OK\r\nContent-Type: application/json",
            b"{}".to_vec(),
        ),
        ("HTTP/1.1 200 OK\r\nContent-Type: application/json", tags),
    ]);
    let s = settings(&dir.0, &[("llm.endpoint", json!(address))]);
    let mut other = request(
        "ollama:gemma3:12b",
        "ollama-registry",
        &"b".repeat(64),
        true,
    );
    other.version = "other".into();
    let error = resources::install(&s, &env(), &other).unwrap_err();
    assert!(error.starts_with("RESOURCE_CHECKSUM_MISMATCH"), "{error}");
    server.join().unwrap();
}

fn provider_entry(
    dir: &Path,
    service: &str,
    key_seed: &str,
    bytes: &[u8],
    fetched_at: u64,
) -> String {
    let key = sha(key_seed.as_bytes());
    let root = dir.join("cache/provider-v1").join(service);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(format!("{key}.body")), bytes).unwrap();
    std::fs::write(
        root.join(format!("{key}.json")),
        json!({"schema":"linguist-provider-cache-v1","url":"https://jisho.org/x","final_url":"https://jisho.org/x","mime":"application/json","fetched_at":fetched_at,"sha256":sha(bytes),"bytes":bytes.len()}).to_string(),
    )
    .unwrap();
    key
}

#[test]
fn provider_cache_prunes_by_retention_and_budget_but_never_unmanaged_files() {
    let dir = Dir::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let old = provider_entry(
        &dir.0,
        "dictionary",
        "old",
        b"old response",
        now - 40 * 86_400,
    );
    let recent = provider_entry(&dir.0, "dictionary", "recent", &[1u8; 2000], now - 86_400);
    let newest = provider_entry(&dir.0, "kanji", "newest", &[2u8; 2000], now - 60);
    std::fs::write(
        dir.0.join("cache/provider-v1/dictionary/notes.txt"),
        b"mine",
    )
    .unwrap();
    let s = settings(&dir.0, &[]);
    let status = cache::status(&s, &env(), None).unwrap();
    assert_eq!(
        status["provider_cache"]["services"]["dictionary"]["entries"],
        2
    );
    assert_eq!(
        status["provider_cache"]["services"]["dictionary"]["older_than_retention"],
        1
    );
    assert_eq!(status["store_assets"]["state"], "absent");
    let preview = cache::prune(&s, &env(), None, None, None, false).unwrap();
    let keys: Vec<&str> = preview["provider_cache"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec![old.as_str()]);
    let receipt = cache::prune(&s, &env(), None, None, None, true).unwrap();
    assert_eq!(receipt["executed"], true);
    let dictionary = dir.0.join("cache/provider-v1/dictionary");
    assert!(
        !dictionary.join(format!("{old}.json")).exists()
            && !dictionary.join(format!("{old}.body")).exists()
    );
    assert!(dictionary.join(format!("{recent}.body")).exists());
    assert!(
        dictionary.join("notes.txt").exists(),
        "unmanaged files are never deleted"
    );
    // A 0 MiB budget removes the remaining entries oldest first; --provider scopes it.
    let scoped = cache::prune(&s, &env(), Some("kanji"), Some(365), Some(0), true).unwrap();
    assert_eq!(scoped["provider_cache"]["removed"][0]["key"], newest);
    assert_eq!(scoped["store_assets"], Value::Null);
    assert!(dictionary.join(format!("{recent}.body")).exists());
    assert!(
        cache::prune(&s, &env(), Some("bogus"), None, None, false)
            .unwrap_err()
            .starts_with("CACHE_PROVIDER_UNKNOWN")
    );
}

#[test]
fn cache_prune_rechecks_active_readers_and_store_reachability() {
    let dir = Dir::new();
    let s = settings(&dir.0, &[("cache.unreferenced_retention_days", json!(0))]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    provider_entry(&dir.0, "image", "stale", b"stale", now - 90 * 86_400);
    let state = dir.0.join("state");
    let mut store = linguist_store::Store::open(&state).unwrap();
    let orphan = store.publish_asset(b"orphan bytes", 1024).unwrap();
    let file = std::fs::File::options()
        .write(true)
        .open(state.join("assets").join(&orphan))
        .unwrap();
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 3600))
        .unwrap();
    let mut reader = linguist_store::Store::open(&state).unwrap();
    let token = reader
        .acquire_lease(
            &linguist_store::lease::Resource::JobWorker(uuid::Uuid::new_v4()),
            60,
        )
        .unwrap();
    let status = cache::status(&s, &env(), None).unwrap();
    assert_eq!(status["store_assets"]["unreferenced_files"], 1);
    assert!(
        status["store_assets"]["prune_blockers"][0]
            .as_str()
            .unwrap()
            .starts_with("GC_BLOCKED_BY_ACTIVE_LEASE")
    );
    let error = cache::prune(&s, &env(), None, None, None, true).unwrap_err();
    assert!(error.starts_with("GC_BLOCKED_BY_ACTIVE_LEASE"), "{error}");
    assert!(
        dir.0
            .join("cache/provider-v1/image")
            .read_dir()
            .unwrap()
            .count()
            == 2,
        "provider cache untouched while blocked"
    );
    reader.release_lease(&token).unwrap();
    let receipt = cache::prune(&s, &env(), None, None, None, true).unwrap();
    assert_eq!(receipt["store_assets"]["unlinked"][0], orphan);
    assert_eq!(
        receipt["provider_cache"]["removed"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(receipt["anki_media_touched"], false);
}

fn legacy_database(path: &Path) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE batch_jobs (id TEXT PRIMARY KEY, deck_key TEXT NOT NULL, deck_name TEXT NOT NULL, status TEXT NOT NULL, dry_run INTEGER NOT NULL DEFAULT 0, settings_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, last_error TEXT NOT NULL DEFAULT '');
         CREATE TABLE batch_items (id INTEGER PRIMARY KEY AUTOINCREMENT, job_id TEXT NOT NULL REFERENCES batch_jobs(id) ON DELETE CASCADE, ordinal INTEGER NOT NULL, note_id INTEGER NOT NULL, word TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', attempts INTEGER NOT NULL DEFAULT 0, next_attempt_at TEXT, artifact_path TEXT, snapshot_id TEXT, result_note_id INTEGER, last_error TEXT NOT NULL DEFAULT '', started_at TEXT, finished_at TEXT, updated_at TEXT NOT NULL, UNIQUE(job_id, note_id));
         INSERT INTO batch_jobs VALUES('batch-1','japanese_vocab','森','running',0,'{\"model\":\"x\"}','2026-01-01','2026-01-01',NULL,NULL,'');
         INSERT INTO batch_jobs VALUES('batch-2','english_vocab','Moon','rolling_back',1,'not json','2026-01-02','2026-01-02',NULL,NULL,'');",
    )
    .unwrap();
    for (ordinal, status) in [
        "pending",
        "processing",
        "processed",
        "committing",
        "completed",
        "failed",
        "rollback_failed",
        "mystery",
    ]
    .iter()
    .enumerate()
    {
        db.execute(
            "INSERT INTO batch_items(job_id,ordinal,note_id,word,status,attempts,snapshot_id,result_note_id,updated_at) VALUES('batch-1',?1,?2,'語',?3,1,'snap-1',?4,'t')",
            rusqlite::params![ordinal as i64, 1_000 + ordinal as i64, status, if *status == "completed" { Some(5_000i64) } else { None }],
        )
        .unwrap();
    }
    db
}

#[test]
fn legacy_jobs_translate_read_only_and_never_assume_commits_absent() {
    let dir = Dir::new();
    let source = dir.0.join("batch_jobs.sqlite3");
    let db = legacy_database(&source);
    // Rows still live only in the WAL while the legacy connection stays open.
    let read = legacy_jobs::read_source(&source).unwrap();
    assert!(
        read.wal.is_some(),
        "the WAL is captured alongside the database"
    );
    let before: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let report = legacy_jobs::translate(&read, &dir.0.join("tmp")).unwrap();
    let after: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n != "tmp")
        .collect();
    assert_eq!(before, after, "nothing written next to the source");
    drop(db);
    let jobs = report["jobs"].as_array().unwrap();
    assert_eq!(jobs[0]["state"], "paused");
    assert_eq!(jobs[0]["runnable"], false);
    assert_eq!(jobs[1]["state"], "rollback_incomplete");
    assert_eq!(jobs[1]["legacy_settings"], Value::Null);
    let states: Vec<&str> = jobs[0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["state"].as_str().unwrap())
        .collect();
    assert_eq!(
        states,
        vec![
            "pending",
            "pending",
            "prepared_not_applied",
            "requires_review",
            "applied_historical",
            "failed",
            "requires_review",
            "unsupported"
        ]
    );
    let review: Vec<&str> = report["requires_review"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        review,
        vec![
            "LEGACY_COMMIT_OUTCOME_UNKNOWN",
            "LEGACY_ROLLBACK_OUTCOME_UNKNOWN",
            "LEGACY_ITEM_STATE_UNKNOWN"
        ]
    );
    assert_eq!(jobs[0]["items"][4]["result_note_id"], "5000");
    assert_eq!(report["dispatches"], false);
    assert!(
        report["unsupported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["code"] == "LEGACY_JOB_SETTINGS_INVALID")
    );
    // Persisting keeps the original bytes as an asset and is idempotent.
    let mut store = linguist_store::Store::open(&dir.0.join("state")).unwrap();
    let (record, created) = store
        .record_legacy_job_import(&read.database, &report, 1)
        .unwrap();
    assert!(created);
    assert_eq!(
        store
            .asset(&record.source_sha256, 100 * 1024 * 1024)
            .unwrap(),
        read.database
    );
    assert!(
        !store
            .record_legacy_job_import(&read.database, &report, 2)
            .unwrap()
            .1
    );
    let census = store.asset_census().unwrap();
    assert!(
        census.files.iter().all(|f| f.reachable),
        "imports are GC roots"
    );
}

#[test]
fn non_legacy_databases_are_refused() {
    let dir = Dir::new();
    let not_sqlite = dir.0.join("x.sqlite3");
    std::fs::write(&not_sqlite, b"hello").unwrap();
    assert!(
        legacy_jobs::read_source(&not_sqlite)
            .err()
            .unwrap()
            .starts_with("LEGACY_JOBS_NOT_SQLITE")
    );
    let other = dir.0.join("other.sqlite3");
    rusqlite::Connection::open(&other)
        .unwrap()
        .execute_batch("CREATE TABLE t(x);")
        .unwrap();
    let read = legacy_jobs::read_source(&other).unwrap();
    assert!(
        legacy_jobs::translate(&read, &dir.0.join("tmp"))
            .unwrap_err()
            .starts_with("LEGACY_JOBS_SCHEMA_UNSUPPORTED")
    );
}

#[test]
fn lookup_filters_change_only_the_dictionary_query() {
    let dir = Dir::new();
    let term = |flags: &[(&str, Value)], text: &str| {
        linguist_application::dictionary::lookup_term(text, &settings(&dir.0, flags))
    };
    let decomposed = "e\u{301}cole (n.)";
    assert_eq!(term(&[], decomposed).unwrap(), "\u{e9}cole (n.)");
    assert_eq!(
        term(&[("filters.normalize_unicode", json!(false))], decomposed).unwrap(),
        decomposed
    );
    assert_eq!(
        term(
            &[("filters.remove_parentheses", json!(true))],
            "食べる（たべる）"
        )
        .unwrap(),
        "食べる"
    );
    assert_eq!(
        term(&[("filters.clean_word_only", json!(true))], "  run-up, 2x!").unwrap(),
        "run up x"
    );
    assert!(
        term(&[("filters.remove_parentheses", json!(true))], "(only)")
            .unwrap_err()
            .starts_with("DICTIONARY_LOOKUP_TERM_EMPTY")
    );
}

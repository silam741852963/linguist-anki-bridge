//! Shared fake native port and fixtures for apply, restore and split tests.
#![allow(dead_code, unused_imports)]
pub use linguist_application::{
    apply::{
        ApplyPort, ApplyRequest, Effect, MutationRequest, NativeStatus, ObservedCard, ObservedDeck,
        ObservedMedia, ObservedNote, OwnerToken, ReconcileRequest, apply_item, apply_items,
        content_digest, current_state_digest, local_proposal, preview, reconcile,
    },
    backup::{
        CheckpointExporter, CheckpointRequest, ExportClaim, ExportRequest, PortFailure,
        ScopePreference, create_checkpoint,
    },
    checkpoint::{CoverageRequirement, PackageLimits, ScopeCard, ScopeManifest, ScopeMedia},
    model_install::ObservedModel,
};
pub use linguist_core::{
    LearningDocument,
    approval::ApprovalRequest,
    canonical,
    document::Task,
    model::{self, ManagedModel},
    records::*,
    render,
};
pub use linguist_store::{
    Store,
    lease::{LeaseToken, Resource},
};
use prost::Message;
use sha1::Digest as _;
pub use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
    time::Duration,
};
pub use uuid::Uuid;

// ---------- verified checkpoint fixture (same shape as checkpoint_writes.rs) ----------

#[derive(Clone, PartialEq, Message)]
pub struct MediaEntries {
    #[prost(message, repeated, tag = "1")]
    pub entries: Vec<MediaEntry>,
}
#[derive(Clone, PartialEq, Message)]
pub struct MediaEntry {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(uint32, tag = "2")]
    size: u32,
    #[prost(bytes, tag = "3")]
    sha1: Vec<u8>,
}
pub const CHECKPOINT_MEDIA: &[u8] = b"disposable voice bytes";

pub fn sha1_hex(bytes: &[u8]) -> String {
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes))
}

pub fn collection() -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("lab-apply-collection-{}", Uuid::new_v4()));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE col(id INTEGER PRIMARY KEY, ver INTEGER, models TEXT); INSERT INTO col VALUES (1,18,'{}'); CREATE TABLE notes(id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT); INSERT INTO notes VALUES (10,1,'word'); CREATE TABLE cards(id INTEGER PRIMARY KEY,nid INTEGER,did INTEGER,ord INTEGER,queue INTEGER,due INTEGER,ivl INTEGER,factor INTEGER,reps INTEGER,lapses INTEGER); INSERT INTO cards VALUES (20,10,1,0,2,5,3,2500,1,0); CREATE TABLE revlog(id INTEGER PRIMARY KEY,cid INTEGER,ease INTEGER,ivl INTEGER,lastIvl INTEGER); INSERT INTO revlog VALUES (30,20,3,3,0); CREATE TABLE graves(usn INTEGER,oid INTEGER,type INTEGER); CREATE TABLE notetypes(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO notetypes VALUES (1,'Basic');").unwrap();
    drop(connection);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    bytes
}

pub fn package() -> Vec<u8> {
    let map = MediaEntries {
        entries: vec![MediaEntry {
            name: "voice.ogg".into(),
            size: CHECKPOINT_MEDIA.len() as u32,
            sha1: sha1::Sha1::digest(CHECKPOINT_MEDIA).to_vec(),
        }],
    }
    .encode_to_vec();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut entry = |name: &str, bytes: &[u8]| {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    };
    entry("meta", &[8, 3]);
    entry(
        "collection.anki21b",
        &zstd::encode_all(collection().as_slice(), 0).unwrap(),
    );
    entry("collection.anki2", &collection());
    entry("media", &zstd::encode_all(map.as_slice(), 0).unwrap());
    entry("0", &zstd::encode_all(CHECKPOINT_MEDIA, 0).unwrap());
    zip.finish().unwrap().into_inner()
}

pub struct Exporter;
impl CheckpointExporter for Exporter {
    fn export_checkpoint(&mut self, request: &ExportRequest) -> Result<ExportClaim, PortFailure> {
        let bytes = package();
        std::fs::write(&request.destination, &bytes).unwrap();
        Ok(ExportClaim {
            path: request.destination.clone(),
            size_bytes: bytes.len() as u64,
            sha256: sha256(&bytes),
        })
    }
}

pub fn binding() -> CollectionBinding {
    CollectionBinding {
        endpoint: "http://127.0.0.1:8765".into(),
        profile_fingerprint: "a".repeat(64),
        path_fingerprint: "b".repeat(64),
        bridge_id: Uuid::from_u128(1),
        lineage_id: Uuid::from_u128(2),
        session_epoch: Uuid::from_u128(3),
        capability_digest: format!("lab-jcs-v1:lab-native-capabilities-v1:{}", "c".repeat(64)),
    }
}

// ---------- plan fixture ----------

pub const V2_MODEL_ID: i64 = 1001;
pub const BASIC_MODEL_ID: i64 = 1;
pub const V2_SOURCE_DIGEST: &str =
    "f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0";
pub const GRAMMAR_MODEL_ID: i64 = 1002;
pub const GRAMMAR_DIGEST: &str = "d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0";
pub const BASIC_SOURCE_DIGEST: &str =
    "e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0";
pub const TARGET_DECK: i64 = 500;
pub const HOME_DECK: i64 = 400;
pub const AUDIO: &[u8] = b"OggS approved disposable audio bytes";

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Create,
    CreateWithMedia,
    Update,
    /// Update whose original Audio field references `orig.ogg`.
    UpdateMedia,
    Migrate,
}

pub const ORIGINAL_AUDIO: &[u8] = b"OggS original source pronunciation";

pub fn source_fields(kind: Kind) -> BTreeMap<String, String> {
    match kind {
        Kind::Migrate => [("Front", "食べる"), ("Back", "to consume")]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        _ => {
            let mut fields: BTreeMap<String, String> = model::vocabulary()
                .fields
                .iter()
                .map(|f| (f.clone(), String::new()))
                .collect();
            fields.insert("Expression".into(), "食べる".into());
            fields.insert("Meaning".into(), "to consume".into());
            fields.insert("Language".into(), "ja".into());
            if kind == Kind::UpdateMedia {
                fields.insert("Audio".into(), "[sound:orig.ogg]".into());
            }
            fields
        }
    }
}

pub struct Setup {
    pub root: PathBuf,
    pub store: Store,
    pub token: LeaseToken,
    pub checkpoint: Uuid,
    pub plan: PlanRevision,
    pub digest: &'static str,
    pub approval: Uuid,
    pub item: Uuid,
}
impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn document(kind: Kind, store: &mut Store) -> LearningDocument {
    let mut doc = LearningDocument::from_json(include_bytes!(
        "../../../../contracts/v2/fixtures/vocabulary.json"
    ))
    .unwrap();
    doc.tags = vec!["linguist".into()];
    if kind == Kind::CreateWithMedia {
        let digest = store.publish_asset(AUDIO, 1024 * 1024).unwrap();
        doc.media.push(MediaAsset {
            filename: format!("lab-{}.ogg", &digest[..16]),
            digest,
            original_filename: None,
            size_bytes: AUDIO.len() as u64,
            mime: "audio/ogg".into(),
            owner: MediaOwner::App,
            role: MediaRole::Audio,
            source_id: None,
            attribution: "user".into(),
            license: None,
        });
    }
    if matches!(kind, Kind::Update | Kind::UpdateMedia | Kind::Migrate) {
        let fields = source_fields(kind);
        let bytes = canonical::bytes(&fields).unwrap();
        let digest = store.publish_asset(&bytes, 1024 * 1024).unwrap();
        let source_id = Uuid::new_v4();
        doc.sources.push(SourceRecord {
            id: source_id,
            kind: "anki_read_capture_v2".into(),
            location: "anki_note:10".into(),
            digest: digest.clone(),
            text: None,
            fields: fields.clone(),
            model_manifest: if kind == Kind::Migrate {
                BASIC_SOURCE_DIGEST.into()
            } else {
                V2_SOURCE_DIGEST.into()
            },
            template_manifest: None,
            captured_at_unix_seconds: Some(1),
            tags: vec!["old".into()],
            cards: vec![],
            media_refs: vec![],
        });
        doc.archives.push(SourceArchive {
            id: Uuid::new_v4(),
            source_id,
            digest: digest.clone(),
            original_text: None,
            original_fields: fields,
            asset_digests: vec![digest],
        });
        if kind == Kind::Migrate {
            doc.task_maps.push(SourceTaskMap {
                schema_version: 1,
                source_id,
                source_model_digest: BASIC_SOURCE_DIGEST.into(),
                target_model: TargetModelKind::Vocabulary,
                entries: vec![SourceTaskMapEntry {
                    source_ordinal: 0,
                    target_task: Task::Comprehension,
                    target_ordinal: 0,
                }],
            });
        }
    }
    doc
}

pub fn setup(kind: Kind) -> Setup {
    setup_with(kind, |_| {})
}

/// Like `setup`, with a reviewed edit to the document before rendering.
pub fn setup_with(kind: Kind, edit: impl FnOnce(&mut LearningDocument)) -> Setup {
    let root = std::env::temp_dir().join(format!("lab-apply-{}", Uuid::new_v4()));
    for dir in ["out", "restore", "scratch"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let mut store = Store::open(&root.join("state")).unwrap();
    let token = store
        .acquire_lease(&Resource::CollectionWriter(Uuid::from_u128(2)), 300)
        .unwrap();
    let checkpoint = create_checkpoint(
        &mut store,
        &token,
        &mut Exporter,
        CheckpointRequest {
            binding: binding(),
            scope: ScopeManifest {
                schema_version: 1,
                requirement: CoverageRequirement {
                    scheduling: true,
                    media: true,
                    schema: true,
                },
                note_ids: vec![10],
                cards: vec![ScopeCard {
                    card_id: 20,
                    note_id: 10,
                    reps: 1,
                    review_count: 1,
                }],
                model_ids: vec![BASIC_MODEL_ID],
                media: vec![ScopeMedia {
                    name: "voice.ogg".into(),
                    sha1: sha1_hex(CHECKPOINT_MEDIA),
                }],
            },
            preference: ScopePreference::Affected,
            output: root.join("out").join("a.colpkg"),
            group_id: None,
            protected_manifest_digest: "protected-v1".into(),
            restore_target: root.join("restore"),
            limits: PackageLimits {
                max_package_bytes: 4 * 1024 * 1024,
                max_collection_bytes: 4 * 1024 * 1024,
                max_media_bytes: 1024 * 1024,
                max_media_map_bytes: 1024 * 1024,
                max_entries: 100,
                timeout: Duration::from_secs(10),
                scratch_dir: root.join("scratch"),
            },
            now_ms: 1_000_000,
        },
    )
    .map_err(|e| e.code)
    .unwrap()
    .record
    .receipt
    .id;
    let mut doc = document(kind, &mut store);
    edit(&mut doc);
    let source = doc
        .sources
        .first()
        .map(|s| s.fields.clone())
        .unwrap_or_default();
    let rendered = render::render(&doc, &source).unwrap();
    let mut values = BTreeMap::new();
    values.insert(
        "purposes.japanese_vocab.target_deck".to_owned(),
        serde_json::json!("Japanese::Vocab"),
    );
    let plan = PlanRevision {
        grammar_groups: vec![],
        schema_version: 2,
        id: Uuid::new_v4(),
        revision: 1,
        parent_digest: None,
        settings: ResolvedSettings {
            semantic_fingerprint: String::new(),
            execution_fingerprint: String::new(),
            version: 2,
            values,
            provenance: BTreeMap::new(),
            resource_hashes: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            fingerprint: "fixture".into(),
        },
        binding: Some(binding()),
        source_digest: "fixture".into(),
        selection: None,
        documents: vec![doc],
        rendered: vec![rendered],
        review_decisions: vec![],
    };
    finish_setup(plan, root, store, token, checkpoint)
}

pub fn finish_setup(
    plan: PlanRevision,
    root: PathBuf,
    mut store: Store,
    token: LeaseToken,
    checkpoint: Uuid,
) -> Setup {
    let digest = store.publish_revision(&plan).unwrap();
    let evidence = linguist_core::plan_validation::inspect(&plan).unwrap();
    let warnings: Vec<String> = evidence.items[0]
        .issues
        .iter()
        .filter(|i| i.severity == linguist_core::Severity::Warning)
        .map(|i| i.code.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let approval = store
        .approve_revision(&ApprovalRequest {
            plan_id: plan.id,
            revision: 1,
            digest: digest.clone(),
            item_ids: None,
            actor: "reviewer".into(),
            accepted_warnings: warnings,
        })
        .unwrap()
        .unwrap()
        .id;
    let item = plan.documents[0].id;
    let digest: &'static str = Box::leak(digest.into_boxed_str());
    Setup {
        root,
        store,
        token,
        checkpoint,
        plan,
        digest,
        approval,
        item,
    }
}

impl Setup {
    pub fn request(&self) -> ApplyRequest<'static> {
        ApplyRequest {
            apply: true,
            plan_id: self.plan.id,
            revision: 1,
            digest: self.digest,
            item_id: self.item,
            approval_id: self.approval,
            checkpoint_id: self.checkpoint,
            group_id: None,
            protected_manifest_digest: "protected-v1",
            reuse_max_age_seconds: 600,
            max_package_bytes: 4 * 1024 * 1024,
            max_media_bytes: 1024 * 1024,
            accept_schema_change: false,
            now_ms: 1_010_000,
        }
    }
    pub fn journal(&self, id: Uuid) -> OperationJournal {
        self.store.journal(id).unwrap().journal
    }
}

// ---------- fake native port ----------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fault {
    None,
    /// Native refuses before any effect.
    Reject,
    /// Effect happens, response lost; ledger says verified.
    LoseAfterEffect,
    /// Request never reached the ledger; no effect.
    LoseBeforeEffect,
    /// Effect happens, ledger row is unknown.
    UnknownAfterEffect,
    /// No effect and ledger row is unknown.
    UnknownNoEffect,
    /// The user studies the card inside the mutation window.
    StudyDuringMutation,
    /// Native migration replaces the retained card with a fresh one.
    ReplaceCard,
    /// Local state database is locked after Anki accepted the write.
    HoldLock,
    /// Effect happens but the operation stays queued past the deadline.
    Pending,
}

pub struct Anki {
    pub binding: CollectionBinding,
    pub variants: Vec<String>,
    pub notes: BTreeMap<i64, ObservedNote>,
    pub models: Vec<ObservedModel>,
    pub decks: Vec<ObservedDeck>,
    pub media: BTreeMap<String, ObservedMedia>,
    pub ledger: BTreeMap<Uuid, NativeStatus>,
    pub fault: Fault,
    pub main_mutations: usize,
    pub media_mutations: usize,
    pub next_id: i64,
    pub owners: u64,
    pub ended: u64,
    pub state_db: PathBuf,
    pub lock: Option<rusqlite::Connection>,
    /// Main mutations that pass before `fault` arms.
    pub skip: usize,
    /// Bytes behind `media`, for the pre-write archive.
    pub media_bytes: BTreeMap<String, Vec<u8>>,
    /// Main mutation variants in dispatch order.
    pub dispatched: Vec<String>,
}

pub fn observed_model(model: &ManagedModel, id: i64) -> ObservedModel {
    let mut templates = model.templates.clone();
    templates.sort_by_key(|t| t.ordinal);
    ObservedModel {
        id,
        name: model.name.clone(),
        fields: model.fields.clone(),
        templates,
        css: model.css.clone(),
    }
}

pub fn studied_card() -> ObservedCard {
    ObservedCard {
        id: 20,
        ordinal: 0,
        deck_id: HOME_DECK,
        original_deck_id: 0,
        scheduler: [("due", "5"), ("ivl", "3"), ("reps", "1")]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        history_digest: "1".repeat(64),
        review_count: 1,
    }
}

impl Anki {
    pub fn new(s: &Setup, kind: Kind) -> Self {
        let mut notes = BTreeMap::new();
        if matches!(kind, Kind::Update | Kind::UpdateMedia | Kind::Migrate) {
            notes.insert(
                10,
                ObservedNote {
                    id: 10,
                    model_id: if kind == Kind::Migrate {
                        BASIC_MODEL_ID
                    } else {
                        V2_MODEL_ID
                    },
                    model_name: if kind == Kind::Migrate {
                        "Basic".into()
                    } else {
                        model::vocabulary().name
                    },
                    model_manifest_digest: if kind == Kind::Migrate {
                        BASIC_SOURCE_DIGEST.into()
                    } else {
                        V2_SOURCE_DIGEST.into()
                    },
                    fields: source_fields(kind),
                    tags: vec!["old".into()],
                    cards: vec![studied_card()],
                },
            );
        }
        Self {
            binding: binding(),
            variants: vec![
                "store_media".into(),
                "create_note".into(),
                "update_note".into(),
                "restore_note".into(),
                "delete_unstudied_created_note".into(),
            ],
            notes,
            models: vec![observed_model(&model::vocabulary(), V2_MODEL_ID)],
            decks: vec![
                ObservedDeck {
                    id: TARGET_DECK,
                    name: "Japanese::Vocab".into(),
                    filtered: false,
                },
                ObservedDeck {
                    id: HOME_DECK,
                    name: "Default".into(),
                    filtered: false,
                },
            ],
            media: BTreeMap::new(),
            ledger: BTreeMap::new(),
            fault: Fault::None,
            main_mutations: 0,
            media_mutations: 0,
            next_id: 9000,
            owners: 0,
            ended: 0,
            state_db: s.root.join("state").join("state.sqlite3"),
            lock: None,
            skip: 0,
            media_bytes: BTreeMap::new(),
            dispatched: vec![],
        }
    }
    pub fn model_digest(id: i64) -> String {
        match id {
            BASIC_MODEL_ID => BASIC_SOURCE_DIGEST.into(),
            GRAMMAR_MODEL_ID => GRAMMAR_DIGEST.into(),
            _ => V2_SOURCE_DIGEST.into(),
        }
    }
    /// Put one media file with bytes into the fake collection.
    pub fn put_media(&mut self, name: &str, bytes: &[u8]) {
        self.media.insert(
            name.into(),
            ObservedMedia {
                filename: name.into(),
                sha256: sha256(bytes),
                size_bytes: bytes.len() as u64,
            },
        );
        self.media_bytes.insert(name.into(), bytes.to_vec());
    }
    pub fn mutations(&self) -> usize {
        self.main_mutations + self.media_mutations
    }
    pub fn id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }
    pub fn card_ordinals(fields: &BTreeMap<String, String>) -> Vec<u16> {
        if fields.contains_key("Pattern") {
            let mut out = vec![0];
            if fields.get("EnableApplication").map(String::as_str) == Some("1") {
                out.push(1);
            }
            return out;
        }
        if fields.contains_key("Front") {
            return vec![0];
        }
        let mut out = vec![0];
        if fields.get("EnableProduction").map(String::as_str) == Some("1") {
            out.push(1);
        }
        if fields.get("EnableSpelling").map(String::as_str) == Some("1") {
            out.push(2);
        }
        out
    }
    /// Apply one effect to the fake collection. Returns a refusal reason.
    pub fn effect(&mut self, effect: &Effect) -> Option<String> {
        match effect {
            Effect::StoreMedia {
                filename,
                sha256,
                size_bytes,
                ..
            } => {
                if self
                    .media
                    .get(filename)
                    .is_some_and(|m| &m.sha256 != sha256)
                {
                    return Some("media_collision".into());
                }
                self.media.insert(
                    filename.clone(),
                    ObservedMedia {
                        filename: filename.clone(),
                        sha256: sha256.clone(),
                        size_bytes: *size_bytes,
                    },
                );
                None
            }
            Effect::CreateNote { envelope } => {
                let body = &envelope["body"];
                let marker = body["marker_tag"].as_str().unwrap();
                if self
                    .notes
                    .values()
                    .any(|n| n.tags.iter().any(|t| t == marker))
                {
                    return Some("marker_exists".into());
                }
                let fields: BTreeMap<String, String> =
                    serde_json::from_value(body["fields"].clone()).unwrap();
                let deck: i64 = body["deck_id"].as_str().unwrap().parse().unwrap();
                let model = self
                    .models
                    .iter()
                    .find(|m| m.name == body["model_name"].as_str().unwrap())
                    .cloned()
                    .unwrap();
                let id = self.id();
                let cards = Self::card_ordinals(&fields)
                    .into_iter()
                    .map(|ordinal| ObservedCard {
                        id: self.id(),
                        ordinal,
                        deck_id: deck,
                        original_deck_id: 0,
                        scheduler: BTreeMap::new(),
                        history_digest: "0".repeat(64),
                        review_count: 0,
                    })
                    .collect();
                self.notes.insert(
                    id,
                    ObservedNote {
                        id,
                        model_id: model.id,
                        model_name: model.name.clone(),
                        model_manifest_digest: Self::model_digest(model.id),
                        fields,
                        tags: serde_json::from_value(body["tags"].clone()).unwrap(),
                        cards,
                    },
                );
                None
            }
            Effect::UpdateNote(update) => {
                let note = self.notes.get(&update.note_id).cloned()?;
                if content_digest(&note).unwrap() != update.expected_pre_digest {
                    return Some("precondition_mismatch".into());
                }
                let mut note = note;
                if let Some(migration) = &update.migration {
                    note.model_id = migration.target_model_id;
                    note.model_name = migration.target_model_name.clone();
                    note.model_manifest_digest = Self::model_digest(migration.target_model_id);
                    for card in &mut note.cards {
                        card.ordinal = migration
                            .ordinal_map
                            .iter()
                            .find(|m| m.source == card.ordinal)
                            .unwrap()
                            .target;
                    }
                }
                note.fields = update.fields.clone();
                note.tags.extend(update.add_tags.iter().cloned());
                for ordinal in Self::card_ordinals(&note.fields) {
                    if !note.cards.iter().any(|c| c.ordinal == ordinal) {
                        let id = self.id();
                        note.cards.push(ObservedCard {
                            id,
                            ordinal,
                            deck_id: update.deck_id,
                            original_deck_id: 0,
                            scheduler: BTreeMap::new(),
                            history_digest: "0".repeat(64),
                            review_count: 0,
                        });
                    }
                }
                for card in &mut note.cards {
                    card.deck_id = update.deck_id;
                }
                self.notes.insert(note.id, note);
                None
            }
            Effect::RestoreNote(restore) => {
                let mut note = self.notes.get(&restore.note_id).cloned()?;
                if content_digest(&note).unwrap() != restore.expected_pre_digest {
                    return Some("precondition_mismatch".into());
                }
                if note.cards.iter().any(|c| c.original_deck_id != 0) {
                    return Some("filtered_deck".into());
                }
                if let Some(migration) = &restore.migration {
                    let removed: BTreeSet<i64> = note
                        .cards
                        .iter()
                        .filter(|c| !migration.ordinal_map.iter().any(|m| m.source == c.ordinal))
                        .map(|c| c.id)
                        .collect();
                    if removed != restore.removed_card_ids.iter().copied().collect()
                        || note
                            .cards
                            .iter()
                            .any(|c| removed.contains(&c.id) && c.review_count > 0)
                    {
                        return Some("reverse_mapping_refused".into());
                    }
                    note.cards.retain(|c| !removed.contains(&c.id));
                    note.model_id = migration.target_model_id;
                    note.model_name = migration.target_model_name.clone();
                    note.model_manifest_digest = Self::model_digest(migration.target_model_id);
                    for card in &mut note.cards {
                        card.ordinal = migration
                            .ordinal_map
                            .iter()
                            .find(|m| m.source == card.ordinal)
                            .unwrap()
                            .target;
                    }
                } else if !restore.removed_card_ids.is_empty() {
                    return Some("removal_without_migration".into());
                }
                note.fields = restore.fields.clone();
                note.tags = restore.tags.clone();
                for card in &mut note.cards {
                    match restore.card_decks.iter().find(|c| c.card_id == card.id) {
                        Some(target) => card.deck_id = target.deck_id,
                        None => return Some("card_deck_missing".into()),
                    }
                }
                self.notes.insert(note.id, note);
                None
            }
            Effect::DeleteUnstudiedCreatedNote {
                note_id,
                expected_pre_digest,
            } => {
                let note = self.notes.get(note_id).cloned()?;
                if &content_digest(&note).unwrap() != expected_pre_digest
                    || note.cards.iter().any(|c| c.review_count > 0)
                {
                    return Some("precondition_mismatch".into());
                }
                self.notes.remove(note_id);
                None
            }
        }
    }
    pub fn study(&mut self) {
        for note in self.notes.values_mut() {
            for card in &mut note.cards {
                if card.review_count > 0 {
                    card.review_count += 1;
                    card.scheduler
                        .insert("reps".into(), card.review_count.to_string());
                    card.history_digest = "2".repeat(64);
                }
            }
        }
    }
}

impl ApplyPort for Anki {
    fn execution_binding(&mut self) -> Result<CollectionBinding, String> {
        Ok(self.binding.clone())
    }
    fn mutation_variants(&mut self) -> Result<Vec<String>, String> {
        Ok(self.variants.clone())
    }
    fn begin(&mut self, _: &CollectionBinding, _: &str) -> Result<OwnerToken, String> {
        self.owners += 1;
        Ok(OwnerToken {
            token: Uuid::new_v4(),
            fence: self.owners,
        })
    }
    fn end(&mut self, _: &OwnerToken) -> Result<(), String> {
        self.ended += 1;
        Ok(())
    }
    fn note(&mut self, id: i64) -> Result<Option<ObservedNote>, String> {
        Ok(self.notes.get(&id).cloned())
    }
    fn notes_tagged(&mut self, tag: &str) -> Result<Vec<ObservedNote>, String> {
        Ok(self
            .notes
            .values()
            .filter(|n| n.tags.iter().any(|t| t == tag))
            .cloned()
            .collect())
    }
    fn models_named(&mut self, name: &str) -> Result<Vec<ObservedModel>, String> {
        Ok(self
            .models
            .iter()
            .filter(|m| m.name == name)
            .cloned()
            .collect())
    }
    fn deck(&mut self, name: &str) -> Result<Option<ObservedDeck>, String> {
        Ok(self.decks.iter().find(|d| d.name == name).cloned())
    }
    fn media(&mut self, filename: &str) -> Result<Option<ObservedMedia>, String> {
        Ok(self.media.get(filename).cloned())
    }
    fn media_bytes(&mut self, filename: &str, _max: u64) -> Result<Option<Vec<u8>>, String> {
        Ok(self.media_bytes.get(filename).cloned())
    }
    fn mutate(&mut self, request: &MutationRequest) -> Result<NativeStatus, PortFailure> {
        assert_eq!(
            request.payload_digest,
            request.effect.payload_digest().unwrap()
        );
        if let Some(existing) = self.ledger.get(&request.operation_id) {
            // Same UUID and payload deduplicates; it never re-executes.
            return Ok(existing.clone());
        }
        if matches!(request.effect, Effect::StoreMedia { .. }) {
            self.media_mutations += 1;
            let status = match self.effect(&request.effect) {
                None => NativeStatus::Verified,
                Some(reason) => NativeStatus::FailedBeforeWrite { reason },
            };
            self.ledger.insert(request.operation_id, status.clone());
            return Ok(status);
        }
        self.main_mutations += 1;
        self.dispatched.push(request.effect.variant().into());
        let fault = if self.skip > 0 {
            self.skip -= 1;
            Fault::None
        } else {
            std::mem::replace(&mut self.fault, Fault::None)
        };
        match fault {
            Fault::Reject => {
                let status = NativeStatus::FailedBeforeWrite {
                    reason: "preflight_rejected".into(),
                };
                self.ledger.insert(request.operation_id, status.clone());
                return Ok(status);
            }
            Fault::LoseBeforeEffect => {
                return Err(PortFailure::Unknown("transport_ambiguous".into()));
            }
            Fault::UnknownNoEffect => {
                self.ledger.insert(
                    request.operation_id,
                    NativeStatus::Unknown {
                        reason: "worker_crash".into(),
                    },
                );
                return Err(PortFailure::Unknown("worker_crash".into()));
            }
            _ => {}
        }
        let status = match self.effect(&request.effect) {
            None => NativeStatus::Verified,
            Some(reason) => NativeStatus::FailedBeforeWrite { reason },
        };
        match fault {
            Fault::StudyDuringMutation => self.study(),
            Fault::ReplaceCard => {
                for note in self.notes.values_mut() {
                    for card in &mut note.cards {
                        if card.id == 20 {
                            card.id = 7777;
                        }
                    }
                }
            }
            Fault::HoldLock => {
                let connection = rusqlite::Connection::open(&self.state_db).unwrap();
                connection.execute_batch("BEGIN IMMEDIATE;").unwrap();
                self.lock = Some(connection);
            }
            _ => {}
        }
        match fault {
            Fault::LoseAfterEffect => {
                self.ledger.insert(request.operation_id, status);
                Err(PortFailure::Unknown("transport_ambiguous".into()))
            }
            Fault::UnknownAfterEffect => {
                self.ledger.insert(
                    request.operation_id,
                    NativeStatus::Unknown {
                        reason: "native_observation_incomplete".into(),
                    },
                );
                Err(PortFailure::Unknown("native_observation_incomplete".into()))
            }
            Fault::Pending => {
                self.ledger
                    .insert(request.operation_id, NativeStatus::Queued);
                Ok(NativeStatus::Queued)
            }
            _ => {
                self.ledger.insert(request.operation_id, status.clone());
                Ok(status)
            }
        }
    }
    fn status(&mut self, operation_id: Uuid) -> Result<NativeStatus, String> {
        Ok(self
            .ledger
            .get(&operation_id)
            .cloned()
            .unwrap_or(NativeStatus::Absent))
    }
}

pub fn reconcile_request(operation: Uuid, apply: bool) -> ReconcileRequest {
    ReconcileRequest {
        operation_id: operation,
        apply,
        rebind: None,
    }
}

pub fn marker_notes(anki: &Anki) -> usize {
    anki.notes
        .values()
        .filter(|n| n.tags.iter().any(|t| t.starts_with("lab_op_")))
        .count()
}

// ---------- scope-driven checkpoints for restore and split tests ----------

/// Disposable collection that contains exactly the scope's notes, cards,
/// review logs and note types.
pub fn collection_for(scope: &ScopeManifest) -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("lab-scope-collection-{}", Uuid::new_v4()));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE col(id INTEGER PRIMARY KEY, ver INTEGER, models TEXT); INSERT INTO col VALUES (1,18,'{}'); CREATE TABLE notes(id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT); CREATE TABLE cards(id INTEGER PRIMARY KEY,nid INTEGER,did INTEGER,ord INTEGER,queue INTEGER,due INTEGER,ivl INTEGER,factor INTEGER,reps INTEGER,lapses INTEGER); CREATE TABLE revlog(id INTEGER PRIMARY KEY,cid INTEGER,ease INTEGER,ivl INTEGER,lastIvl INTEGER); CREATE TABLE graves(usn INTEGER,oid INTEGER,type INTEGER); CREATE TABLE notetypes(id INTEGER PRIMARY KEY, name TEXT);").unwrap();
    for note in &scope.note_ids {
        connection
            .execute("INSERT INTO notes VALUES (?1,1,'x')", [note])
            .unwrap();
    }
    let mut log = 1;
    for card in &scope.cards {
        connection
            .execute(
                "INSERT INTO cards VALUES (?1,?2,1,0,2,5,3,2500,?3,0)",
                rusqlite::params![card.card_id, card.note_id, card.reps],
            )
            .unwrap();
        for _ in 0..card.review_count {
            connection
                .execute(
                    "INSERT INTO revlog VALUES (?1,?2,3,3,0)",
                    rusqlite::params![log, card.card_id],
                )
                .unwrap();
            log += 1;
        }
    }
    for model in &scope.model_ids {
        connection
            .execute("INSERT INTO notetypes VALUES (?1,'m')", [model])
            .unwrap();
    }
    drop(connection);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    bytes
}

pub struct ScopedExporter(pub ScopeManifest);
impl CheckpointExporter for ScopedExporter {
    fn export_checkpoint(&mut self, request: &ExportRequest) -> Result<ExportClaim, PortFailure> {
        let map = MediaEntries {
            entries: vec![MediaEntry {
                name: "voice.ogg".into(),
                size: CHECKPOINT_MEDIA.len() as u32,
                sha1: sha1::Sha1::digest(CHECKPOINT_MEDIA).to_vec(),
            }],
        }
        .encode_to_vec();
        let collection = collection_for(&self.0);
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let mut entry = |name: &str, bytes: &[u8]| {
            zip.start_file(name, options).unwrap();
            zip.write_all(bytes).unwrap();
        };
        entry("meta", &[8, 3]);
        entry(
            "collection.anki21b",
            &zstd::encode_all(collection.as_slice(), 0).unwrap(),
        );
        entry("collection.anki2", &collection);
        entry("media", &zstd::encode_all(map.as_slice(), 0).unwrap());
        entry("0", &zstd::encode_all(CHECKPOINT_MEDIA, 0).unwrap());
        let bytes = zip.finish().unwrap().into_inner();
        std::fs::write(&request.destination, &bytes).unwrap();
        Ok(ExportClaim {
            path: request.destination.clone(),
            size_bytes: bytes.len() as u64,
            sha256: sha256(&bytes),
        })
    }
}

/// Verified collection checkpoint covering these notes, all their cards and
/// the given note types, created at `now_ms`.
pub fn checkpoint_for(s: &mut Setup, notes: &[&ObservedNote], models: &[i64], now_ms: u64) -> Uuid {
    let mut note_ids: Vec<i64> = notes.iter().map(|n| n.id).collect();
    note_ids.sort();
    let mut cards: Vec<ScopeCard> = notes
        .iter()
        .flat_map(|n| {
            n.cards.iter().map(|c| ScopeCard {
                card_id: c.id,
                note_id: n.id,
                reps: c.review_count as u32,
                review_count: c.review_count as u32,
            })
        })
        .collect();
    cards.sort_by_key(|c| c.card_id);
    let mut model_ids = models.to_vec();
    model_ids.sort();
    model_ids.dedup();
    let scope = ScopeManifest {
        schema_version: 1,
        requirement: CoverageRequirement {
            scheduling: true,
            media: true,
            schema: true,
        },
        note_ids,
        cards,
        model_ids,
        media: vec![ScopeMedia {
            name: "voice.ogg".into(),
            sha1: sha1_hex(CHECKPOINT_MEDIA),
        }],
    };
    let tag = Uuid::new_v4();
    let restore = s.root.join(format!("restore-{tag}"));
    std::fs::create_dir_all(&restore).unwrap();
    create_checkpoint(
        &mut s.store,
        &s.token,
        &mut ScopedExporter(scope.clone()),
        CheckpointRequest {
            binding: binding(),
            scope,
            preference: ScopePreference::Affected,
            output: s.root.join("out").join(format!("{tag}.colpkg")),
            group_id: None,
            protected_manifest_digest: "protected-v1".into(),
            restore_target: restore,
            limits: PackageLimits {
                max_package_bytes: 4 * 1024 * 1024,
                max_collection_bytes: 4 * 1024 * 1024,
                max_media_bytes: 1024 * 1024,
                max_media_map_bytes: 1024 * 1024,
                max_entries: 100,
                timeout: Duration::from_secs(10),
                scratch_dir: s.root.join("scratch"),
            },
            now_ms,
        },
    )
    .map_err(|e| e.code)
    .unwrap()
    .record
    .receipt
    .id
}

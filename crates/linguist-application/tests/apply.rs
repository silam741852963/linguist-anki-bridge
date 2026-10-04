//! ALG-APPLY/ALG-MIGRATE/ALG-RECONCILE over a fake native port. These tests
//! prove orchestration and journal rules only; native scheduling preservation
//! is shown separately in disposable Anki (scripts/verify-native-apply.py).
use linguist_application::{
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
use linguist_core::{
    LearningDocument,
    approval::ApprovalRequest,
    canonical,
    document::Task,
    model::{self, ManagedModel},
    records::*,
    render,
};
use linguist_store::{
    Store,
    lease::{LeaseToken, Resource},
};
use prost::Message;
use sha1::Digest as _;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
    time::Duration,
};
use uuid::Uuid;

// ---------- verified checkpoint fixture (same shape as checkpoint_writes.rs) ----------

#[derive(Clone, PartialEq, Message)]
struct MediaEntries {
    #[prost(message, repeated, tag = "1")]
    entries: Vec<MediaEntry>,
}
#[derive(Clone, PartialEq, Message)]
struct MediaEntry {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(uint32, tag = "2")]
    size: u32,
    #[prost(bytes, tag = "3")]
    sha1: Vec<u8>,
}
const CHECKPOINT_MEDIA: &[u8] = b"disposable voice bytes";

fn sha1_hex(bytes: &[u8]) -> String {
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes))
}

fn collection() -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("lab-apply-collection-{}", Uuid::new_v4()));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE col(id INTEGER PRIMARY KEY, ver INTEGER, models TEXT); INSERT INTO col VALUES (1,18,'{}'); CREATE TABLE notes(id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT); INSERT INTO notes VALUES (10,1,'word'); CREATE TABLE cards(id INTEGER PRIMARY KEY,nid INTEGER,did INTEGER,ord INTEGER,queue INTEGER,due INTEGER,ivl INTEGER,factor INTEGER,reps INTEGER,lapses INTEGER); INSERT INTO cards VALUES (20,10,1,0,2,5,3,2500,1,0); CREATE TABLE revlog(id INTEGER PRIMARY KEY,cid INTEGER,ease INTEGER,ivl INTEGER,lastIvl INTEGER); INSERT INTO revlog VALUES (30,20,3,3,0); CREATE TABLE graves(usn INTEGER,oid INTEGER,type INTEGER); CREATE TABLE notetypes(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO notetypes VALUES (1,'Basic');").unwrap();
    drop(connection);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    bytes
}

fn package() -> Vec<u8> {
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

struct Exporter;
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

fn binding() -> CollectionBinding {
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

const V2_MODEL_ID: i64 = 1001;
const BASIC_MODEL_ID: i64 = 1;
const V2_SOURCE_DIGEST: &str = "f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0";
const BASIC_SOURCE_DIGEST: &str =
    "e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0";
const TARGET_DECK: i64 = 500;
const HOME_DECK: i64 = 400;
const AUDIO: &[u8] = b"OggS approved disposable audio bytes";

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Create,
    CreateWithMedia,
    Update,
    Migrate,
}

fn source_fields(kind: Kind) -> BTreeMap<String, String> {
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
            fields
        }
    }
}

struct Setup {
    root: PathBuf,
    store: Store,
    token: LeaseToken,
    checkpoint: Uuid,
    plan: PlanRevision,
    digest: &'static str,
    approval: Uuid,
    item: Uuid,
}
impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn document(kind: Kind, store: &mut Store) -> LearningDocument {
    let mut doc = LearningDocument::from_json(include_bytes!(
        "../../../contracts/v2/fixtures/vocabulary.json"
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
    if matches!(kind, Kind::Update | Kind::Migrate) {
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

fn setup(kind: Kind) -> Setup {
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
    let doc = document(kind, &mut store);
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

fn finish_setup(
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
    fn request(&self) -> ApplyRequest<'static> {
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
    fn journal(&self, id: Uuid) -> OperationJournal {
        self.store.journal(id).unwrap().journal
    }
}

// ---------- fake native port ----------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fault {
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

struct Anki {
    binding: CollectionBinding,
    variants: Vec<String>,
    notes: BTreeMap<i64, ObservedNote>,
    models: Vec<ObservedModel>,
    decks: Vec<ObservedDeck>,
    media: BTreeMap<String, ObservedMedia>,
    ledger: BTreeMap<Uuid, NativeStatus>,
    fault: Fault,
    main_mutations: usize,
    media_mutations: usize,
    next_id: i64,
    owners: u64,
    ended: u64,
    state_db: PathBuf,
    lock: Option<rusqlite::Connection>,
}

fn observed_model(model: &ManagedModel, id: i64) -> ObservedModel {
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

fn studied_card() -> ObservedCard {
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
    fn new(s: &Setup, kind: Kind) -> Self {
        let mut notes = BTreeMap::new();
        if matches!(kind, Kind::Update | Kind::Migrate) {
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
        }
    }
    fn mutations(&self) -> usize {
        self.main_mutations + self.media_mutations
    }
    fn id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }
    fn card_ordinals(fields: &BTreeMap<String, String>) -> Vec<u16> {
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
    fn effect(&mut self, effect: &Effect) -> Option<String> {
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
                        model_id: V2_MODEL_ID,
                        model_name: model::vocabulary().name,
                        model_manifest_digest: V2_SOURCE_DIGEST.into(),
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
                    note.model_manifest_digest = V2_SOURCE_DIGEST.into();
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
        }
    }
    fn study(&mut self) {
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
        let fault = std::mem::replace(&mut self.fault, Fault::None);
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

fn reconcile_request(operation: Uuid, apply: bool) -> ReconcileRequest {
    ReconcileRequest {
        operation_id: operation,
        apply,
        rebind: None,
    }
}

fn marker_notes(anki: &Anki) -> usize {
    anki.notes
        .values()
        .filter(|n| n.tags.iter().any(|t| t.starts_with("lab_op_")))
        .count()
}

// ---------- tests ----------

#[test]
fn create_commits_with_marker_snapshot_receipt_and_owner_release() {
    let mut s = setup(Kind::CreateWithMedia);
    let mut anki = Anki::new(&s, Kind::CreateWithMedia);
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    assert!(outcome.next_command.is_none() && outcome.receipt_digest.is_some());
    assert_eq!((anki.main_mutations, anki.media_mutations), (1, 1));
    assert_eq!(anki.owners, 1);
    assert_eq!(anki.ended, 1);
    let note = &anki.notes[&outcome.note_id.unwrap()];
    assert_eq!(note.cards.len(), 1);
    assert_eq!(note.cards[0].deck_id, TARGET_DECK);
    let operation = outcome.operation_id.unwrap();
    let journal = s.journal(operation);
    assert_eq!(journal.steps.len(), 2);
    assert_eq!(journal.steps[0].action, "store_media");
    assert!(
        journal
            .steps
            .iter()
            .all(|step| step.state == StepState::Verified)
    );
    let marker = format!("lab_op_{}", journal.steps[1].id.simple());
    assert!(note.tags.contains(&marker) && note.tags.contains(&"linguist".to_owned()));
    let snapshot = s.store.snapshot(outcome.snapshot_id.unwrap()).unwrap();
    assert!(snapshot.snapshot.originals.is_empty());
    let receipt = snapshot.after.unwrap();
    assert_eq!(receipt.operation_id, journal.steps[1].id);
    assert_eq!(
        receipt.readback.unwrap().note_ids[0],
        linguist_core::document::AnkiId::try_from(note.id.to_string()).unwrap()
    );
    // A committed item cannot be applied again.
    let request = s.request();
    let again = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(again.starts_with("APPLY_ALREADY_COMMITTED"), "{again}");
    assert_eq!(anki.mutations(), 2);
}

#[test]
fn missing_apply_flag_and_preview_have_zero_effects() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    let mut request = s.request();
    request.apply = false;
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_FLAG_REQUIRED"));
    let items = preview(&s.store, s.plan.id, 1, &[]).unwrap();
    assert_eq!(items.len(), 1);
    assert!(
        items[0].approved && items[0].content_ready,
        "{:?}",
        items[0]
    );
    assert_eq!(items[0].action, "create");
    assert!(items[0].blockers.is_empty(), "{:?}", items[0].blockers);
    assert_eq!(anki.mutations(), 0);
    assert_eq!(anki.owners, 0);
    assert_eq!(s.store.pending_journal_count().unwrap(), 0);
}

type Adjust = dyn Fn(&mut ApplyRequest, &mut Anki);

#[test]
fn authority_identity_and_checkpoint_refusals_mutate_nothing() {
    let mut s = setup(Kind::Create);
    let digest = s.digest;
    let cases: Vec<(Box<Adjust>, &str)> = vec![
        (
            Box::new(|r, _| r.digest = "lab-jcs-v1:plan:0"),
            "APPLY_DIGEST_MISMATCH",
        ),
        (
            Box::new(|r, _| r.approval_id = Uuid::new_v4()),
            "APPROVAL_NOT_FOUND",
        ),
        (
            Box::new(|r, _| r.item_id = Uuid::new_v4()),
            "APPLY_APPROVAL_MISMATCH",
        ),
        (
            Box::new(|_, a| a.binding.profile_fingerprint = "d".repeat(64)),
            "APPLY_IDENTITY_MISMATCH",
        ),
        (
            Box::new(|_, a| a.binding.lineage_id = Uuid::from_u128(99)),
            "APPLY_IDENTITY_MISMATCH",
        ),
        (
            Box::new(|r, _| r.checkpoint_id = Uuid::new_v4()),
            "CHECKPOINT_NOT_FOUND",
        ),
        (
            // A changed session epoch no longer matches the checkpoint binding.
            Box::new(|_, a| a.binding.session_epoch = Uuid::from_u128(4)),
            "CHECKPOINT_BINDING_MISMATCH",
        ),
        (
            Box::new(|r, _| r.reuse_max_age_seconds = 1),
            "CHECKPOINT_REUSE_REJECTED",
        ),
        (
            Box::new(|_, a| a.variants.retain(|v| v != "create_note")),
            "CAPABILITY_UNAVAILABLE",
        ),
        (Box::new(|_, a| a.models.clear()), "APPLY_MODEL_MISSING"),
        (
            Box::new(|_, a| a.models[0].css.push_str("/* changed */")),
            "MODEL_NAME_COLLISION",
        ),
        (
            Box::new(|_, a| a.decks.clear()),
            "APPLY_TARGET_DECK_MISSING",
        ),
    ];
    for (mutate, expected) in cases {
        let mut anki = Anki::new(&s, Kind::Create);
        let mut request = s.request();
        request.digest = digest;
        mutate(&mut request, &mut anki);
        let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
        assert!(error.contains(expected), "{expected}: {error}");
        assert_eq!(anki.mutations(), 0, "{expected}");
    }
    assert_eq!(s.store.pending_journal_count().unwrap(), 0);
}

#[test]
fn weak_binding_stale_revision_and_remote_bridge_are_refused() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.binding.endpoint = "http://192.0.2.1:8765".into();
    let mut plan = s.plan.clone();
    plan.binding = Some(anki.binding.clone());
    // Remote endpoint: plan and bridge agree, but writes need loopback.
    let mut remote = s.plan.clone();
    remote.id = Uuid::new_v4();
    remote.binding = Some(anki.binding.clone());
    let digest = s.store.publish_revision(&remote).unwrap();
    let approval = s
        .store
        .approve_revision(&ApprovalRequest {
            plan_id: remote.id,
            revision: 1,
            digest: digest.clone(),
            item_ids: None,
            actor: "reviewer".into(),
            accepted_warnings: vec![],
        })
        .unwrap()
        .unwrap()
        .id;
    let mut request = s.request();
    request.plan_id = remote.id;
    request.digest = &digest;
    request.approval_id = approval;
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(
        error.starts_with("APPLY_REMOTE_MUTATION_UNAVAILABLE"),
        "{error}"
    );
    // Weak binding.
    let mut weak = s.plan.clone();
    weak.id = Uuid::new_v4();
    weak.binding = None;
    let digest = s.store.publish_revision(&weak).unwrap();
    let approval = s
        .store
        .approve_revision(&ApprovalRequest {
            plan_id: weak.id,
            revision: 1,
            digest: digest.clone(),
            item_ids: None,
            actor: "reviewer".into(),
            accepted_warnings: vec![],
        })
        .unwrap()
        .unwrap()
        .id;
    let mut anki = Anki::new(&s, Kind::Create);
    let mut request = s.request();
    request.plan_id = weak.id;
    request.digest = &digest;
    request.approval_id = approval;
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_BINDING_WEAK"), "{error}");
    // A newer revision makes the approved one stale.
    let mut next = s.plan.clone();
    next.revision = 2;
    next.parent_digest = Some(s.digest.to_owned());
    s.store.publish_revision(&next).unwrap();
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_REVISION_STALE"), "{error}");
    assert_eq!(anki.mutations(), 0);
}

#[test]
fn second_writer_is_rejected_by_the_collection_lease() {
    let s = setup(Kind::Create);
    let mut other = Store::open(&s.root.join("state")).unwrap();
    let error = other
        .acquire_lease(&Resource::CollectionWriter(Uuid::from_u128(2)), 300)
        .unwrap_err();
    assert!(error.contains("LEASE"), "{error}");
}

#[test]
fn source_edit_between_preparation_and_apply_conflicts_and_preserves_original() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    anki.notes
        .get_mut(&10)
        .unwrap()
        .fields
        .insert("Meaning".into(), "user edit".into());
    let before = anki.notes[&10].clone();
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_SOURCE_CONFLICT"), "{error}");
    assert!(error.contains("fields"));
    assert_eq!(anki.notes[&10], before);
    assert_eq!(anki.mutations(), 0);
    // A model change since preparation is also a conflict.
    let mut anki = Anki::new(&s, Kind::Update);
    anki.notes.get_mut(&10).unwrap().model_manifest_digest = "9".repeat(64);
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.contains("model"), "{error}");
}

#[test]
fn study_between_preparation_and_apply_is_captured_fresh_and_preserved() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    anki.study();
    let studied = anki.notes[&10].cards[0].clone();
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    let note = &anki.notes[&10];
    assert!(note.fields["Meaning"].contains("to eat"));
    assert!(note.tags.contains(&"old".to_owned()) && note.tags.contains(&"linguist".to_owned()));
    assert!(!note.tags.iter().any(|t| t.starts_with("lab_op_")));
    let card = &note.cards[0];
    assert_eq!(card.id, 20);
    assert_eq!(card.deck_id, TARGET_DECK);
    assert_eq!(card.scheduler, studied.scheduler);
    assert_eq!(card.history_digest, studied.history_digest);
    let snapshot = s.store.snapshot(outcome.snapshot_id.unwrap()).unwrap();
    let original = &snapshot.snapshot.originals[0];
    assert_eq!(original.fields["Meaning"], "to consume");
    assert_eq!(original.cards[0].scheduler, studied.scheduler);
    assert_eq!(original.cards[0].history_digest, studied.history_digest);
    assert_eq!(snapshot.after.unwrap().readback.unwrap().card_ids.len(), 1);
}

#[test]
fn study_during_mutation_needs_recovery_without_success_claim() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    anki.fault = Fault::StudyDuringMutation;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    assert!(
        outcome
            .issues
            .iter()
            .any(|i| i.code == "APPLY_READBACK_MISMATCH")
    );
    assert!(outcome.next_command.unwrap().contains("recover reconcile"));
    let snapshot = s.store.snapshot(outcome.snapshot_id.unwrap()).unwrap();
    assert!(snapshot.after.is_none());
    // Reconciliation cannot adopt a state that differs from the desired post-state.
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(outcome.operation_id.unwrap(), true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::NeedsRecovery);
    assert_eq!(rec.steps[0].action, "review");
    assert_eq!(anki.main_mutations, 1);
}

#[test]
fn timeout_after_accepted_create_reconciles_to_one_note() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::LoseAfterEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    let operation = outcome.operation_id.unwrap();
    assert_eq!(s.journal(operation).steps[0].state, StepState::Unknown);
    // A new attempt for the same item is blocked until reconciliation.
    let request = s.request();
    let blocked = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(blocked.starts_with("APPLY_RECOVERY_REQUIRED"), "{blocked}");
    // Read-only proposal first.
    let proposal = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, false),
    )
    .unwrap();
    assert_eq!(proposal.state, OperationState::NeedsRecovery);
    assert_eq!(proposal.steps[0].action, "adopt");
    assert!(!proposal.applied);
    assert_eq!(
        local_proposal(&s.store, operation).unwrap().next_live_check,
        "native_status_and_readback"
    );
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert!(rec.receipt_digest.is_some());
    assert_eq!(marker_notes(&anki), 1);
    assert_eq!(anki.main_mutations, 1);
    // Reconciling again is a no-op on a committed operation.
    let again = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(again.state, OperationState::Committed);
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn lost_request_without_ledger_row_resubmits_the_same_uuid_once() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::LoseBeforeEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    let operation = outcome.operation_id.unwrap();
    let step = s.journal(operation).steps[0].id;
    let proposal = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, false),
    )
    .unwrap();
    assert_eq!(proposal.steps[0].action, "resubmit_same_uuid");
    assert_eq!(anki.main_mutations, 1);
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert!(
        rec.issues
            .iter()
            .any(|i| i.code == "APPLY_RESUBMIT_SAME_UUID")
    );
    assert_eq!(anki.ledger.len(), 1);
    assert!(anki.ledger.contains_key(&step));
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn unknown_add_with_exact_marker_candidate_is_adopted() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::UnknownAfterEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert_eq!(rec.steps[0].action, "adopt");
    assert_eq!(anki.main_mutations, 1);
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn unknown_status_without_candidates_never_resubmits() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::UnknownNoEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    for _ in 0..2 {
        let rec = reconcile(
            &mut s.store,
            &s.token,
            &mut anki,
            &reconcile_request(operation, true),
        )
        .unwrap();
        assert_eq!(rec.state, OperationState::NeedsRecovery);
        assert_eq!(rec.steps[0].evidence, "absent_unproven");
    }
    let issues = s.journal(operation).issues;
    assert_eq!(
        issues
            .iter()
            .filter(|i| i.code == "APPLY_ABSENCE_UNPROVEN")
            .count(),
        1
    );
    assert_eq!(anki.main_mutations, 1);
    assert_eq!(marker_notes(&anki), 0);
}

#[test]
fn copied_marker_candidates_block_for_review() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::LoseAfterEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    let copy = anki.notes.values().next().unwrap().clone();
    let mut copy = copy;
    copy.id = 1;
    anki.notes.insert(1, copy);
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::NeedsRecovery);
    assert_eq!(rec.steps[0].evidence, "partial_or_conflict");
    assert_eq!(anki.main_mutations, 1);
}

#[test]
fn disk_full_after_native_acceptance_stops_and_recovery_discovers_result() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    anki.fault = Fault::HoldLock;
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(
        error.starts_with("APPLY_LOCAL_DURABILITY_FAILED"),
        "{error}"
    );
    anki.lock = None;
    assert_eq!(anki.main_mutations, 1);
    let pending = s.store.pending_journals(10).unwrap();
    assert_eq!(pending.len(), 1);
    let journal = &pending[0].journal;
    assert_eq!(journal.steps[0].state, StepState::RequestStarted);
    let snapshot = s.store.snapshot(journal.snapshot_id).unwrap();
    assert_eq!(
        snapshot.snapshot.originals[0].fields["Meaning"],
        "to consume"
    );
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(journal.id, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert_eq!(anki.main_mutations, 1);
    // The original snapshot is untouched; the receipt is stored beside it.
    let after = s.store.snapshot(journal.snapshot_id).unwrap();
    assert_eq!(after.snapshot, snapshot.snapshot);
    assert!(after.after.is_some());
}

#[test]
fn media_collision_never_overwrites_and_identical_media_is_reused() {
    let mut s = setup(Kind::CreateWithMedia);
    let filename = s.plan.documents[0].media[0].filename.clone();
    let mut anki = Anki::new(&s, Kind::CreateWithMedia);
    anki.media.insert(
        filename.clone(),
        ObservedMedia {
            filename: filename.clone(),
            sha256: "9".repeat(64),
            size_bytes: 3,
        },
    );
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(
        error.starts_with("APPLY_MEDIA_FILENAME_COLLISION"),
        "{error}"
    );
    assert_eq!(anki.media[&filename].sha256, "9".repeat(64));
    assert_eq!(anki.mutations(), 0);
    let mut anki = Anki::new(&s, Kind::CreateWithMedia);
    anki.media.insert(
        filename.clone(),
        ObservedMedia {
            filename: filename.clone(),
            sha256: sha256(AUDIO),
            size_bytes: AUDIO.len() as u64,
        },
    );
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::Committed);
    assert_eq!((anki.main_mutations, anki.media_mutations), (1, 0));
    assert_eq!(s.journal(outcome.operation_id.unwrap()).steps.len(), 1);
}

#[test]
fn rejected_create_fails_before_write_and_allows_a_new_attempt() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::Reject;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::FailedBeforeWrite);
    assert!(outcome.next_command.is_none());
    assert_eq!(s.store.pending_journal_count().unwrap(), 0);
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::Committed);
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn rejection_after_verified_media_is_a_known_partial_that_can_be_superseded() {
    let mut s = setup(Kind::CreateWithMedia);
    let mut anki = Anki::new(&s, Kind::CreateWithMedia);
    anki.fault = Fault::Reject;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    assert!(
        outcome
            .issues
            .iter()
            .any(|i| i.code == "APPLY_KNOWN_PARTIAL")
    );
    // The stored media is reused by the superseding attempt; nothing is deleted.
    let next = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(next.state, OperationState::Committed);
    assert_eq!(anki.media_mutations, 1);
    assert_eq!(anki.media.len(), 1);
}

#[test]
fn pending_native_operation_is_waited_on_not_resent() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::Pending;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    let operation = outcome.operation_id.unwrap();
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    // The fake applied the effect while still reporting queued: the read-back
    // matches, so the status alone does not block adoption.
    assert_eq!(rec.steps[0].native_status, Some(NativeStatus::Queued));
    assert_eq!(rec.steps[0].action, "wait");
    assert_eq!(rec.state, OperationState::NeedsRecovery);
    assert_eq!(anki.main_mutations, 1);
    anki.ledger
        .insert(s.journal(operation).steps[0].id, NativeStatus::Verified);
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
}

#[test]
fn migration_requires_accepted_schema_change_and_complete_mapping() {
    let mut s = setup(Kind::Migrate);
    let mut anki = Anki::new(&s, Kind::Migrate);
    let request = s.request();
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(
        error.starts_with("APPLY_SCHEMA_CHANGE_NOT_ACCEPTED"),
        "{error}"
    );
    let mut request = s.request();
    request.accept_schema_change = true;
    // A second, unmapped source card would be deleted by the native change.
    let mut extra = studied_card();
    extra.id = 21;
    extra.ordinal = 1;
    anki.notes.get_mut(&10).unwrap().cards.push(extra);
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_MIGRATION_DROPS_CARD"), "{error}");
    // Filtered-deck membership blocks migration.
    let mut anki = Anki::new(&s, Kind::Migrate);
    anki.notes.get_mut(&10).unwrap().cards[0].original_deck_id = HOME_DECK;
    let error = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap_err();
    assert!(error.starts_with("APPLY_FILTERED_DECK_BLOCKS"), "{error}");
    assert_eq!(anki.mutations(), 0);
}

#[test]
fn mapped_migration_retains_card_id_history_and_scheduling() {
    let mut s = setup(Kind::Migrate);
    let mut anki = Anki::new(&s, Kind::Migrate);
    let before = anki.notes[&10].cards[0].clone();
    let mut request = s.request();
    request.accept_schema_change = true;
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    let note = &anki.notes[&10];
    assert_eq!(note.model_id, V2_MODEL_ID);
    let card = &note.cards[0];
    assert_eq!((card.id, card.ordinal), (20, 0));
    assert_eq!(card.scheduler, before.scheduler);
    assert_eq!(card.history_digest, before.history_digest);
    let journal = s.journal(outcome.operation_id.unwrap());
    let record = s.store.apply_operation(journal.id).unwrap();
    assert_eq!(record.intent["action"], "migrate");
    assert_eq!(
        record.intent["steps"][0]["effect"]["migration"]["ordinal_map"][0]["target"],
        0
    );
}

#[test]
fn partial_native_migration_with_replaced_card_needs_recovery() {
    let mut s = setup(Kind::Migrate);
    let mut anki = Anki::new(&s, Kind::Migrate);
    anki.fault = Fault::ReplaceCard;
    let mut request = s.request();
    request.accept_schema_change = true;
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert_eq!(outcome.state, OperationState::NeedsRecovery);
    assert!(
        outcome
            .issues
            .iter()
            .any(|i| i.code == "APPLY_READBACK_MISMATCH")
    );
    assert!(
        s.store
            .snapshot(outcome.snapshot_id.unwrap())
            .unwrap()
            .after
            .is_none()
    );
}

#[test]
fn profile_switch_or_session_change_stops_reconciliation_until_rebind() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.fault = Fault::LoseAfterEffect;
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    // Profile switch: different collection identity.
    anki.binding.profile_fingerprint = "d".repeat(64);
    let error = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap_err();
    assert!(error.starts_with("APPLY_IDENTITY_MISMATCH"), "{error}");
    // Same collection, new session epoch.
    anki.binding = binding();
    anki.binding.session_epoch = Uuid::from_u128(44);
    let error = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap_err();
    assert!(error.starts_with("SESSION_CHANGED"), "{error}");
    let record = s.store.apply_operation(operation).unwrap();
    let intent: linguist_application::apply::ApplyIntent =
        serde_json::from_value(record.intent).unwrap();
    let observed = current_state_digest(&mut anki, &intent).unwrap();
    let mut decision = ResumeBindingDecision {
        schema_version: 1,
        operation_id: operation,
        approval_digest: s.digest.to_owned(),
        old_binding: binding(),
        new_binding: anki.binding.clone(),
        observed_state_digest: "0".repeat(64),
        actor: "operator".into(),
        decided_at: "unix-seconds:1".into(),
        scope: ResumeBindingScope::ContinueOperation,
    };
    let stale = ReconcileRequest {
        operation_id: operation,
        apply: true,
        rebind: Some(decision.clone()),
    };
    let error = reconcile(&mut s.store, &s.token, &mut anki, &stale).unwrap_err();
    assert!(error.starts_with("BINDING_DECISION_STALE"), "{error}");
    decision.observed_state_digest = observed;
    // --rebind without the current invocation's --apply is refused.
    let unauthorized = ReconcileRequest {
        operation_id: operation,
        apply: false,
        rebind: Some(decision.clone()),
    };
    let error = reconcile(&mut s.store, &s.token, &mut anki, &unauthorized).unwrap_err();
    assert!(error.starts_with("APPLY_FLAG_REQUIRED"), "{error}");
    let request = ReconcileRequest {
        operation_id: operation,
        apply: true,
        rebind: Some(decision),
    };
    let rec = reconcile(&mut s.store, &s.token, &mut anki, &request).unwrap();
    assert!(rec.rebound);
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert_eq!(s.store.binding_decisions(operation).unwrap().len(), 1);
    // The receipt carries the rebound epoch and is accepted by the store.
    let snapshot = s.store.snapshot(s.journal(operation).snapshot_id).unwrap();
    assert_eq!(snapshot.after.unwrap().session_epoch, Uuid::from_u128(44));
    assert_eq!(anki.main_mutations, 1);
    assert_eq!(marker_notes(&anki), 1);
}

#[test]
fn batch_stops_on_identity_fault_and_continues_after_item_failures() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    anki.binding.lineage_id = Uuid::from_u128(77);
    let first = s.request();
    let second = s.request();
    let results = apply_items(&mut s.store, &s.token, &mut anki, &[first, second]);
    assert_eq!(results.len(), 1);
    assert!(
        results[0]
            .as_ref()
            .unwrap_err()
            .1
            .starts_with("APPLY_IDENTITY_MISMATCH")
    );
    let mut anki = Anki::new(&s, Kind::Create);
    let mut first = s.request();
    first.approval_id = Uuid::new_v4();
    let second = s.request();
    let results = apply_items(&mut s.store, &s.token, &mut anki, &[first, second]);
    assert_eq!(results.len(), 2);
    assert!(results[0].is_err());
    assert_eq!(
        results[1].as_ref().unwrap().state,
        OperationState::Committed
    );
}

#[test]
fn intent_records_and_binding_decisions_are_immutable() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    let db = rusqlite::Connection::open(s.root.join("state").join("state.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE apply_operations SET created_ms=0", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM apply_operations", []).is_err());
    drop(db);
    let records = s
        .store
        .apply_operations_for_item(s.plan.id, s.item)
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation_id, operation);
    // An intent cannot be recorded for an unapproved item.
    let mut forged = records[0].clone();
    forged.operation_id = Uuid::new_v4();
    forged.item_id = Uuid::new_v4();
    assert_eq!(
        s.store.publish_apply_operation(&forged).unwrap_err(),
        "APPLY_OPERATION_APPROVAL_CONFLICT"
    );
}

/// Shared vector with addons/linguist_bridge/tests/test_effects.py: the native
/// critical section must compute the identical precondition digest.
#[test]
fn precondition_digest_matches_the_companion_vector() {
    let note = ObservedNote {
        id: 1700000000001,
        model_id: 1700000000002,
        model_name: "Linguist Vocabulary v2".into(),
        model_manifest_digest: "ab".repeat(32),
        fields: [("Expression", "食べる"), ("Meaning", "to \"eat\"\n")]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        tags: vec!["zeta".into(), "alpha".into(), "zeta".into()],
        cards: vec![
            ObservedCard {
                id: 1700000000004,
                ordinal: 1,
                deck_id: 1,
                original_deck_id: 0,
                scheduler: BTreeMap::new(),
                history_digest: String::new(),
                review_count: 0,
            },
            ObservedCard {
                id: 1700000000003,
                ordinal: 0,
                deck_id: 1,
                original_deck_id: 0,
                scheduler: BTreeMap::new(),
                history_digest: String::new(),
                review_count: 3,
            },
        ],
    };
    assert_eq!(
        content_digest(&note).unwrap(),
        "lab-jcs-v1:lab-apply-precondition-v1:719d9ff2eac8a52ebe85cc0ed0c38c91a46ceafb852850fc65536615488c314c"
    );
}

#[test]
fn crash_after_receipt_before_commit_finalizes_with_the_stored_receipt() {
    let mut s = setup(Kind::Create);
    let mut anki = Anki::new(&s, Kind::Create);
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, &mut anki, &request).unwrap();
    let operation = outcome.operation_id.unwrap();
    let snapshot = outcome.snapshot_id.unwrap();
    let stored = s.store.snapshot(snapshot).unwrap().after.unwrap();
    // Simulate a crash between the receipt write and the commit event. Test-only
    // tampering removes the final committed event, which the store otherwise forbids.
    let db = rusqlite::Connection::open(s.root.join("state").join("state.sqlite3")).unwrap();
    let head: u32 = db
        .query_row(
            "SELECT sequence FROM journal_heads WHERE operation=?1",
            [operation.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch("DROP TRIGGER journal_events_no_delete;")
        .unwrap();
    db.execute(
        "UPDATE journal_heads SET sequence=?1 WHERE operation=?2",
        rusqlite::params![head - 1, operation.to_string()],
    )
    .unwrap();
    db.execute(
        "DELETE FROM journal_events WHERE operation=?1 AND sequence=?2",
        rusqlite::params![operation.to_string(), head],
    )
    .unwrap();
    drop(db);
    assert_eq!(s.journal(operation).state, OperationState::Verifying);
    let rec = reconcile(
        &mut s.store,
        &s.token,
        &mut anki,
        &reconcile_request(operation, true),
    )
    .unwrap();
    assert_eq!(rec.state, OperationState::Committed, "{:?}", rec.issues);
    assert_eq!(s.store.snapshot(snapshot).unwrap().after.unwrap(), stored);
    assert_eq!(anki.main_mutations, 1);
}

//! Offline contract tooling. Collection commands are added only with verified native safety.
use clap::{Args, CommandFactory, Parser, Subcommand};
use linguist_core::{LearningDocument, canonical, model, render, validation};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::ExitCode,
    sync::atomic::{AtomicU8, Ordering},
};
mod diagnostics;
mod maintenance;
mod recovery_live;
#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
enum OutputMode {
    Human,
    Json,
    Jsonl,
}
impl OutputMode {
    fn from_setting(value: &serde_json::Value) -> Result<Self, String> {
        match value.as_str() {
            Some("text") => Ok(Self::Human),
            Some("json") => Ok(Self::Json),
            Some("jsonl") => Ok(Self::Jsonl),
            _ => Err("INVALID_OUTPUT_FORMAT".into()),
        }
    }
}
static OUTPUT_MODE: AtomicU8 = AtomicU8::new(0);
fn set_output_mode(mode: OutputMode) {
    OUTPUT_MODE.store(mode as u8, Ordering::Relaxed);
}
fn output_mode() -> OutputMode {
    match OUTPUT_MODE.load(Ordering::Relaxed) {
        1 => OutputMode::Json,
        2 => OutputMode::Jsonl,
        _ => OutputMode::Human,
    }
}
#[derive(Parser)]
#[command(
    name = "linguist-anki-bridge",
    version,
    about = "CLI for reviewing vocabulary and grammar card content"
)]
struct Cli {
    /// Explicit configuration file; otherwise LAB_CONFIG/XDG is used.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    profile: Option<String>,
    #[arg(long, global = true)]
    purpose: Option<String>,
    /// Typed setting override: KEY=VALUE (arrays/maps use JSON).
    #[arg(long = "set", global = true)]
    settings: Vec<String>,
    /// Forbid internet providers; loopback services remain governed by their adapters.
    #[arg(long, global = true)]
    offline: bool,
    /// Result format; specify before the command to distinguish file output paths.
    #[arg(long, value_enum)]
    output: Option<OutputMode>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Preview an approved plan's apply preflight; collection writes need the verified native adapter.
    Apply {
        plan: uuid::Uuid,
        #[arg(long)]
        revision: Option<u32>,
        #[arg(long)]
        digest: Option<String>,
        #[arg(long = "item-id")]
        item_ids: Vec<uuid::Uuid>,
        /// Explicitly accept the schema/full-sync warning of a mapped note-type migration.
        #[arg(long)]
        accept_schema_change: bool,
        /// Preview one reviewed grammar split group (children first, then the anchor).
        #[arg(long = "split-group", conflicts_with = "item_ids")]
        split_group: Option<uuid::Uuid>,
        #[arg(long)]
        apply: bool,
    },
    /// List, show, export and preview restoring local snapshots; restore writes need the verified native adapter.
    Snapshots {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// Inspect and prune unreferenced local cache; never touches Anki media or history.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    /// List and explicitly install pinned local resources (packs, voices, models).
    Resources {
        #[command(subcommand)]
        command: ResourceCommand,
    },
    /// Queue preparation inputs and inspect durable local progress.
    Jobs {
        #[command(subcommand)]
        command: JobCommand,
    },
    /// Print shell completions without reading configuration or contacting services.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Prepare vocabulary content; never applies to Anki.
    Vocab {
        #[command(subcommand)]
        command: PrepareCommand,
    },
    /// Prepare grammar content; never applies to Anki.
    Grammar {
        #[command(subcommand)]
        command: PrepareCommand,
    },
    /// Inspect durable local recovery evidence without retrying effects.
    Recover {
        #[command(subcommand)]
        command: RecoveryCommand,
    },
    /// Inspect immutable plan revisions in existing local state.
    Plans {
        #[command(subcommand)]
        command: PlanCommand,
    },
    /// Inspect, validate or initialize local configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Inspect the current implementation and collection safety status.
    Doctor {
        #[arg(long, conflicts_with_all = ["ollama", "bridge"])]
        local: bool,
        /// Probe only the configured local Ollama model metadata; never load or pull.
        #[arg(long, conflicts_with = "bridge")]
        ollama: bool,
        /// Inspect the native companion's declaration without authorizing writes.
        #[arg(long)]
        bridge: bool,
    },
    /// Read Anki deck names, IDs and requested counts.
    Decks {
        #[command(subcommand)]
        command: DeckCommand,
    },
    /// Read Anki notes; private field values are shown only by show.
    Notes {
        #[command(subcommand)]
        command: NoteCommand,
    },
    /// Offline domain contract tools; these do not create or approve plans.
    Document {
        #[command(subcommand)]
        command: DocumentCommand,
    },
    /// Print the fixed managed model manifests.
    Models {
        #[command(subcommand)]
        command: Option<ModelCommand>,
    },
    /// Verified checkpoints: preview/create, list receipts and verify packages.
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
}
#[derive(Subcommand)]
enum BackupCommand {
    /// Preview checkpoint coverage and path; --apply needs the verified native export.
    Create {
        /// affected or collection; schema actions always escalate to collection.
        #[arg(long)]
        scope: String,
        /// Create-new .colpkg path whose parent directory exists; defaults to a
        /// new name in storage.backup_dir.
        #[arg(long)]
        output: Option<PathBuf>,
        /// JSON scope manifest (notes, cards, models, media) the package must cover.
        #[arg(long)]
        scope_manifest: Option<PathBuf>,
        #[arg(long)]
        apply: bool,
    },
    /// List stored checkpoint receipts, newest first.
    List {
        #[arg(long)]
        scope: Option<String>,
        /// RFC 3339 UTC time or YYYY-MM-DD.
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Verify a receipt ID or package file; a restore test uses a disposable directory.
    Verify {
        backup: String,
        #[arg(long)]
        restore_test_target: Option<PathBuf>,
        /// Scope manifest for an unregistered package file.
        #[arg(long)]
        scope_manifest: Option<PathBuf>,
    },
    /// Check the current .colpkg container and declared media without restoring it.
    Inspect { file: PathBuf },
}
#[derive(Subcommand)]
enum SnapshotCommand {
    List {
        #[arg(long)]
        note_id: Option<String>,
        #[arg(long)]
        job: Option<uuid::Uuid>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        cursor: Option<String>,
    },
    Show {
        snapshot: uuid::Uuid,
    },
    /// Preview what restoring this apply snapshot would reverse.
    Restore {
        snapshot: uuid::Uuid,
        /// Observed-state-bound restore decision (JSON) to validate.
        #[arg(long)]
        decision: Option<PathBuf>,
        #[arg(long)]
        apply: bool,
    },
    Export {
        snapshot: uuid::Uuid,
        #[arg(long)]
        output: PathBuf,
    },
}
#[derive(Subcommand)]
enum CacheCommand {
    /// Totals, reachable/protected/unreferenced bytes and retention policy.
    Status {
        /// Only this provider cache (dictionary, kanji, image, tts).
        #[arg(long)]
        provider: Option<String>,
    },
    /// Preview pruning; --execute deletes only unreferenced local cache.
    Prune {
        /// Override cache.unreferenced_retention_days.
        #[arg(long)]
        age_days: Option<u32>,
        /// Override cache.max_size_mb.
        #[arg(long)]
        budget_mb: Option<u64>,
        /// Only this provider cache; store assets are left alone.
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        execute: bool,
    },
}
#[derive(Subcommand)]
enum ResourceCommand {
    List {
        #[arg(long)]
        installed: bool,
        #[arg(long)]
        required: bool,
        #[arg(long)]
        resource: Option<String>,
    },
    Install {
        resource: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        sha256: String,
        /// SPDX license identifier of the artifact.
        #[arg(long)]
        license: String,
        /// New directory inside storage.resource_dir (directory kinds only).
        #[arg(long)]
        destination: Option<PathBuf>,
        /// Download/copy, verify and install; otherwise preview only.
        #[arg(long)]
        execute: bool,
    },
}
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum JobModeArg {
    Prepare,
    Simulate,
    Apply,
}
impl JobModeArg {
    fn mode(self) -> linguist_core::records::JobMode {
        match self {
            Self::Prepare => linguist_core::records::JobMode::Prepare,
            Self::Simulate => linguist_core::records::JobMode::Simulate,
            Self::Apply => linguist_core::records::JobMode::Apply,
        }
    }
}
#[derive(Subcommand)]
enum JobCommand {
    /// Record a retry envelope for eligible failed items; never retries unknown outcomes.
    Retry {
        job: uuid::Uuid,
        #[arg(long = "item-id", conflicts_with = "failed")]
        item_ids: Vec<uuid::Uuid>,
        #[arg(
            long,
            conflicts_with = "item_ids",
            required_unless_present = "item_ids"
        )]
        failed: bool,
        #[arg(long)]
        apply: bool,
    },
    Rollback {
        job: uuid::Uuid,
        #[arg(long = "item-id")]
        item_ids: Vec<uuid::Uuid>,
        #[arg(long)]
        apply: bool,
    },
    /// Preview or record a local tombstone of a terminal job; nothing in Anki is deleted.
    Delete {
        job: uuid::Uuid,
        #[arg(long)]
        execute: bool,
    },
    /// Preview interrupted source reads; --execute records recovery without retrying them.
    Recover {
        job: uuid::Uuid,
        #[arg(long, default_value_t = 0)]
        after_index: u32,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: Option<u32>,
        #[arg(long)]
        actor: Option<String>,
        #[arg(long)]
        execute: bool,
    },
    /// Inspect verified local checkpoint/control pages; never reads Anki.
    Audit {
        job: uuid::Uuid,
        #[arg(long, default_value_t = 0)]
        after_checkpoint: u32,
        #[arg(long, default_value_t = 0)]
        after_control: u32,
        #[arg(long,value_parser=clap::value_parser!(u32).range(1..=1000))]
        limit: Option<u32>,
        #[arg(long)]
        live: bool,
    },
    /// Upgrade existing local job storage after preserving a verified backup, or
    /// import a legacy batch-job database as read-only history (--legacy).
    Migrate {
        /// Legacy batch_jobs.sqlite3 to translate (never written).
        #[arg(long, conflicts_with_all = ["list_imports", "show_import"])]
        legacy: Option<PathBuf>,
        /// Keep the original database and translation in local state.
        #[arg(long, requires = "legacy")]
        execute: bool,
        #[arg(long)]
        list_imports: bool,
        #[arg(long, conflicts_with = "list_imports")]
        show_import: Option<uuid::Uuid>,
    },
    /// Request a durable pause; dispatched reads finish their accounting.
    Pause {
        job: uuid::Uuid,
    },
    /// Clear a pause request and run with the job's frozen settings.
    Resume {
        job: uuid::Uuid,
        /// Required on every resume of an apply job.
        #[arg(long)]
        apply: bool,
    },
    /// Prevent future dispatch; retain all checkpoints and assets.
    Cancel {
        job: uuid::Uuid,
    },
    /// Run a job with its frozen settings: prepare captures sources; apply needs --apply.
    Run {
        job: uuid::Uuid,
        /// Required on every run of an apply job; refused for prepare and simulate jobs.
        #[arg(long)]
        apply: bool,
    },
    /// Freeze a prepare selection, or an approved plan revision for simulate/apply; never runs it.
    Create {
        #[command(flatten)]
        selector: NoteSelector,
        /// Queue the first N query/deck matches in frozen order.
        #[arg(long, conflicts_with = "note_ids", requires = "NoteSelector", value_parser = clap::value_parser!(u64).range(1..=100000))]
        limit: Option<u64>,
        #[arg(long, value_enum, default_value = "prepare")]
        mode: JobModeArg,
        /// Approved plan for simulate/apply jobs.
        #[arg(long, conflicts_with = "NoteSelector")]
        plan: Option<uuid::Uuid>,
        #[arg(long, requires = "plan")]
        revision: Option<u32>,
        #[arg(long, requires = "plan")]
        digest: Option<String>,
        /// Approval to use; defaults to the only approval of the exact revision and digest.
        #[arg(long, requires = "plan")]
        approval: Option<uuid::Uuid>,
        /// Verified checkpoint; required for apply jobs.
        #[arg(long, requires = "plan")]
        checkpoint: Option<uuid::Uuid>,
        /// Protected-manifest digest checked against the checkpoint.
        #[arg(long, requires = "checkpoint")]
        protected_manifest: Option<String>,
        /// Plan items to include, in plan order; default every approved item.
        #[arg(long = "item-id", requires = "plan")]
        item_ids: Vec<uuid::Uuid>,
        #[arg(long, requires = "plan")]
        accept_schema_change: bool,
    },
    List {
        #[arg(long)]
        after: Option<uuid::Uuid>,
        #[arg(long,value_parser=clap::value_parser!(u32).range(1..=10000))]
        limit: Option<u32>,
        #[arg(long, value_enum)]
        mode: Option<JobModeArg>,
        /// Include tombstoned jobs.
        #[arg(long)]
        include_deleted: bool,
    },
    Show {
        job: uuid::Uuid,
    },
    Items {
        job: uuid::Uuid,
        /// Resume at this zero-based position in the frozen input order.
        #[arg(long, default_value_t = 0)]
        after_index: u32,
        #[arg(long,value_parser=clap::value_parser!(u32).range(1..=10000))]
        limit: Option<u32>,
    },
}
#[derive(Subcommand)]
enum PrepareCommand {
    /// Capture selected existing notes into one review-required source draft; never applies.
    Revamp {
        #[command(flatten)]
        selector: NoteSelector,
        /// Prepare the first N query/deck matches in frozen order.
        #[arg(long, conflicts_with = "note_ids", requires = "NoteSelector", value_parser = clap::value_parser!(u64).range(1..=100000))]
        limit: Option<u64>,
    },
    /// Prepare one authored card from flags, or structured records from a file.
    Add {
        #[arg(long)]
        /// UTF-8 input file; use - for noninteractive stdin.
        document: Option<PathBuf>,
        /// Explicit input framing: JSON, JSONL, or simple vocabulary/grammar CSV.
        #[arg(long, value_enum, requires = "document")]
        format: Option<AddFormat>,
        #[command(flatten)]
        inline: Box<InlineAdd>,
    },
}
#[derive(Args)]
struct InlineAdd {
    /// Vocabulary expression; use with --meaning, --sense-key and --target-language.
    #[arg(long)]
    expression: Option<String>,
    /// Grammar pattern; use with --meaning, --formation, --use-key and --target-language.
    #[arg(long)]
    pattern: Option<String>,
    #[arg(long)]
    meaning: Option<String>,
    #[arg(long)]
    sense_key: Option<String>,
    #[arg(long)]
    formation: Option<String>,
    #[arg(long)]
    use_key: Option<String>,
    #[arg(long)]
    target_language: Option<String>,
    #[arg(long)]
    explanation_language: Option<String>,
    #[arg(long)]
    reading: Option<String>,
    #[arg(long)]
    pronunciation: Option<String>,
    #[arg(long)]
    usage: Option<String>,
    /// One authored example in the target language.
    #[arg(long)]
    example_sentence: Option<String>,
    /// Translation paired with --example-sentence.
    #[arg(long)]
    example_translation: Option<String>,
    #[arg(long)]
    production_prompt: Option<String>,
    #[arg(long)]
    spelling_prompt: Option<String>,
    #[arg(long)]
    recognition_prompt: Option<String>,
    #[arg(long)]
    exercise_prompt: Option<String>,
    #[arg(long)]
    exercise_answer: Option<String>,
    #[arg(long)]
    context: Option<String>,
    #[arg(long)]
    personal_notes: Option<String>,
    #[arg(long)]
    source_summary: Option<String>,
    /// Add a tag to the prepared card; may be repeated.
    #[arg(long = "tag")]
    tags: Vec<String>,
}
impl InlineAdd {
    fn present(&self) -> bool {
        self.expression.is_some()
            || self.pattern.is_some()
            || self.meaning.is_some()
            || self.sense_key.is_some()
            || self.formation.is_some()
            || self.use_key.is_some()
            || self.target_language.is_some()
            || self.explanation_language.is_some()
            || self.reading.is_some()
            || self.pronunciation.is_some()
            || self.usage.is_some()
            || self.example_sentence.is_some()
            || self.example_translation.is_some()
            || self.production_prompt.is_some()
            || self.spelling_prompt.is_some()
            || self.recognition_prompt.is_some()
            || self.exercise_prompt.is_some()
            || self.exercise_answer.is_some()
            || self.context.is_some()
            || self.personal_notes.is_some()
            || self.source_summary.is_some()
            || !self.tags.is_empty()
    }
    fn into_input(
        self,
        kind: linguist_application::Kind,
    ) -> Result<linguist_application::AddInput, String> {
        let required = |value: Option<String>, flag: &str| {
            value.ok_or_else(|| format!("INPUT_INLINE_REQUIRED: --{flag}"))
        };
        let target_language =
            linguist_core::Language::try_from(required(self.target_language, "target-language")?)
                .map_err(|_| "INPUT_INLINE_LANGUAGE_INVALID")?;
        let explanation_language = self
            .explanation_language
            .map(linguist_core::Language::try_from)
            .transpose()
            .map_err(|_| "INPUT_INLINE_LANGUAGE_INVALID")?;
        let context = self.context.unwrap_or_default();
        let personal_notes = self.personal_notes.unwrap_or_default();
        let source_summary = self.source_summary.unwrap_or_default();
        let tags = self.tags;
        let examples = match (self.example_sentence, self.example_translation) {
            (None, None) => vec![],
            (Some(sentence), Some(translation)) => vec![linguist_core::Example {
                sentence,
                translation,
                provenance: linguist_core::Provenance::User,
                evidence_ids: vec![],
            }],
            _ => return Err("INPUT_INLINE_EXAMPLE_PAIR_REQUIRED".into()),
        };
        match kind {
            linguist_application::Kind::Vocabulary => {
                if self.pattern.is_some()
                    || self.formation.is_some()
                    || self.use_key.is_some()
                    || self.recognition_prompt.is_some()
                    || self.exercise_prompt.is_some()
                    || self.exercise_answer.is_some()
                {
                    return Err(
                        "INPUT_INLINE_KIND_CONFLICT: grammar-only flag on vocabulary add".into(),
                    );
                }
                Ok(linguist_application::AddInput::Vocabulary {
                    schema_version: 2,
                    target_language,
                    explanation_language,
                    body: linguist_application::VocabularyInput {
                        expression: required(self.expression, "expression")?,
                        meaning: required(self.meaning, "meaning")?,
                        sense_key: required(self.sense_key, "sense-key")?,
                        reading: self.reading.unwrap_or_default(),
                        pronunciation: self.pronunciation.unwrap_or_default(),
                        usage: self.usage.unwrap_or_default(),
                        examples,
                        dictionary: vec![],
                        kanji: String::new(),
                        production_prompt: self.production_prompt.unwrap_or_default(),
                        spelling_prompt: self.spelling_prompt.unwrap_or_default(),
                    },
                    requested_tasks: None,
                    context,
                    personal_notes,
                    source_summary,
                    tags,
                })
            }
            linguist_application::Kind::Grammar => {
                if self.expression.is_some()
                    || self.sense_key.is_some()
                    || self.reading.is_some()
                    || self.pronunciation.is_some()
                    || self.production_prompt.is_some()
                    || self.spelling_prompt.is_some()
                {
                    return Err(
                        "INPUT_INLINE_KIND_CONFLICT: vocabulary-only flag on grammar add".into(),
                    );
                }
                Ok(linguist_application::AddInput::Grammar {
                    schema_version: 2,
                    target_language,
                    explanation_language,
                    body: linguist_core::Grammar {
                        pattern: required(self.pattern, "pattern")?,
                        use_key: required(self.use_key, "use-key")?,
                        meaning: required(self.meaning, "meaning")?,
                        formation: required(self.formation, "formation")?,
                        recognition_prompt: self.recognition_prompt.unwrap_or_default(),
                        examples,
                        usage: self.usage.unwrap_or_default(),
                        exercise_prompt: self.exercise_prompt.unwrap_or_default(),
                        exercise_answer: self.exercise_answer.unwrap_or_default(),
                    },
                    requested_tasks: None,
                    context,
                    personal_notes,
                    source_summary,
                    tags,
                })
            }
        }
    }
}
#[derive(Clone, Copy, clap::ValueEnum)]
enum AddFormat {
    Json,
    Jsonl,
    Csv,
}
#[derive(Subcommand)]
enum DeckCommand {
    Map {
        purpose: String,
        #[arg(long)]
        source_deck: String,
        #[arg(long)]
        target_deck: Option<String>,
        #[arg(long)]
        source_model: String,
        #[arg(long)]
        fields: PathBuf,
        #[arg(long)]
        task_map: Option<PathBuf>,
        #[arg(long = "ocr-language")]
        ocr_languages: Vec<String>,
    },
    Unmap {
        purpose: String,
    },
    List {
        #[arg(long)]
        counts: bool,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        name_contains: Option<String>,
    },
    Show {
        deck: String,
    },
}
#[derive(Subcommand)]
enum ModelCommand {
    List,
    Inspect {
        model: String,
    },
    /// Preview a fixed v2 model for a purpose; native installation is gated.
    Install {
        purpose: String,
        #[arg(long)]
        apply: bool,
    },
    /// Print builtin manifests without contacting Anki.
    Builtin,
}
#[derive(Args)]
#[group(multiple = false)]
struct NoteSelector {
    #[arg(long = "note-id")]
    note_ids: Vec<String>,
    #[arg(long)]
    query: Option<String>,
    #[arg(long)]
    deck: Option<String>,
}
#[derive(Subcommand)]
enum NoteCommand {
    List {
        #[command(flatten)]
        selector: NoteSelector,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        cursor: Option<String>,
    },
    Show {
        note_id: String,
        /// Read referenced media bytes and report hashes/sizes, without exporting their content.
        #[arg(long)]
        media: bool,
    },
    Count {
        #[command(flatten)]
        selector: NoteSelector,
    },
}
#[derive(Subcommand)]
enum RecoveryCommand {
    Reconcile {
        operation: uuid::Uuid,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        rebind: bool,
    },
    Inspect {
        #[arg(conflicts_with = "pending", required_unless_present = "pending")]
        operation: Option<uuid::Uuid>,
        #[arg(long)]
        pending: bool,
        /// Read native ledger status without reconciling or retrying effects.
        #[arg(long)]
        live: bool,
    },
}
#[derive(Subcommand)]
enum PlanCommand {
    /// Rerun one stage (dictionary|enrichment|generation); previews unless executed.
    Regenerate {
        plan: uuid::Uuid,
        #[arg(long)]
        base_revision: u32,
        #[arg(long)]
        digest: String,
        #[arg(long)]
        item_id: Option<uuid::Uuid>,
        #[arg(long)]
        stage: String,
        /// Owned field to clear and regenerate (repeatable); default protects them.
        #[arg(long)]
        overwrite: Vec<String>,
        /// Freeze the current settings and execute; without it only a preview is shown.
        #[arg(long)]
        use_current_settings: bool,
    },
    /// Generate one review-only supplement with explicit current settings.
    Generate {
        plan: uuid::Uuid,
        #[arg(long)]
        item_id: uuid::Uuid,
        #[arg(long)]
        base_revision: u32,
        #[arg(long)]
        digest: String,
        /// Freeze current configuration into the child revision (including --set overrides).
        #[arg(long)]
        use_current_settings: bool,
    },
    /// Read managed v2 duplicate candidates for one authored add item; never clears apply.
    DuplicateCandidates {
        plan: uuid::Uuid,
        #[arg(long)]
        item_id: uuid::Uuid,
        #[arg(long)]
        revision: Option<u32>,
        /// Record found candidates as a duplicate review in a new revision.
        #[arg(long, requires = "digest")]
        record: bool,
        /// Exact digest of the latest revision being extended (with --record).
        #[arg(long)]
        digest: Option<String>,
    },
    /// Split a retained grammar source into authored units with one explicit anchor.
    SplitGrammar {
        plan: uuid::Uuid,
        #[arg(
            long,
            required_unless_present = "template",
            conflicts_with = "template"
        )]
        request: Option<PathBuf>,
        /// Print an editable split request built from this item's reviewed OCR segmentation.
        #[arg(long)]
        template: Option<uuid::Uuid>,
    },
    /// Enrich a retained vocabulary draft using its frozen dictionary policy.
    Enrich {
        plan: uuid::Uuid,
        #[arg(long)]
        base_revision: u32,
        #[arg(long)]
        digest: String,
    },
    /// Export a portable JSON bundle; private source archives require explicit inclusion.
    Export {
        plan: uuid::Uuid,
        #[arg(long)]
        revision: Option<u32>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "v2")]
        format: String,
        #[arg(long)]
        include_private_archives: bool,
    },
    /// Resolve a known review issue using a fingerprint-bound typed decision.
    Resolve {
        plan: uuid::Uuid,
        issue: String,
        #[arg(long)]
        decision: PathBuf,
    },
    /// Record content approval; collection apply still requires separate authorization.
    Approve {
        plan: uuid::Uuid,
        #[arg(long)]
        revision: u32,
        #[arg(long)]
        digest: String,
        #[arg(long)]
        actor: String,
        #[arg(long = "item-id")]
        item_ids: Vec<uuid::Uuid>,
        #[arg(long = "accept-warning")]
        accepted_warnings: Vec<String>,
    },
    /// Validate saved content and persist digest-bound evidence.
    Validate {
        plan: uuid::Uuid,
        #[arg(long)]
        revision: Option<u32>,
        #[arg(long)]
        live: bool,
        #[arg(long, requires = "live")]
        after_index: Option<u32>,
        #[arg(long, requires = "live", value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: Option<u32>,
    },
    /// Apply a typed JSON patch and preserve an immutable parent revision.
    Edit {
        plan: uuid::Uuid,
        #[arg(long)]
        base_revision: u32,
        #[arg(long, required_unless_present = "editor", conflicts_with = "editor")]
        patch: Option<PathBuf>,
        /// Edit a private typed draft in editing.editor_argv, VISUAL or EDITOR.
        #[arg(long)]
        editor: bool,
        #[arg(long)]
        save_draft: bool,
    },
    /// Compare saved revisions; --live adds a bounded read-only source check.
    Diff {
        plan: uuid::Uuid,
        #[arg(long)]
        from_revision: u32,
        #[arg(long)]
        revision: Option<u32>,
        #[arg(long)]
        live: bool,
        #[arg(long, requires = "live")]
        after_index: Option<u32>,
        #[arg(long, requires = "live", value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: Option<u32>,
    },
    List {
        #[arg(long)]
        limit: Option<u32>,
        /// Only latest revisions in this state: ready|needs_review|invalid.
        #[arg(long)]
        status: Option<String>,
        /// Only plans of this workflow: vocab_add|vocab_revamp|grammar_add|grammar_revamp|mixed.
        #[arg(long)]
        workflow: Option<String>,
    },
    Show {
        plan: uuid::Uuid,
        #[arg(long)]
        revision: Option<u32>,
        /// Inspect only this document from the selected revision.
        #[arg(long)]
        item: Option<uuid::Uuid>,
        /// Show a bounded issue page with exact decision identities and templates.
        #[arg(long)]
        issues_only: bool,
        #[arg(long, requires = "issues_only")]
        after_index: Option<u32>,
        #[arg(long, requires = "issues_only", value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: Option<u32>,
    },
}
#[derive(Subcommand)]
enum ConfigCommand {
    /// Convert a legacy Python YAML or native JSON config into a candidate TOML
    /// plus per-key report; the source and live config are never written.
    Import {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Replace this import's own candidate/report files.
        #[arg(long)]
        replace: bool,
    },
    /// Check the config version (v2 is current), or activate an imported
    /// candidate with --from-import after accepting each unresolved key.
    Migrate {
        #[arg(long, conflicts_with = "from_import")]
        output: Option<PathBuf>,
        /// Candidate written by `config import`.
        #[arg(long)]
        from_import: Option<PathBuf>,
        /// Explicitly accept one blocking legacy key (repeat for each).
        #[arg(long = "accept-unresolved", requires = "from_import")]
        accept_unresolved: Vec<String>,
        #[arg(long)]
        execute: bool,
    },
    /// Set a typed value in the base file or explicit profile/purpose scope.
    Set { key: String, value: String },
    /// Remove an override, restoring inheritance; missing overrides are a no-op.
    Unset { key: String },
    /// Preview removal of overrides; --execute saves with a backup.
    Reset {
        key: Option<String>,
        #[arg(long, conflicts_with = "key", required_unless_present = "key")]
        all: bool,
        #[arg(long)]
        execute: bool,
    },
    /// Write minimal version-2 TOML; replacing an existing file saves a private backup.
    Init {
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        replace: bool,
    },
    /// Show effective settings or builtin defaults; optional exact key/prefix.
    Show {
        key: Option<String>,
        #[arg(long, conflicts_with = "effective")]
        defaults: bool,
        #[arg(long)]
        effective: bool,
        #[arg(long)]
        provenance: bool,
    },
    /// Describe a registered setting or mapping pattern.
    Describe { key: String },
    /// Validate a candidate file or the effective configuration.
    Validate {
        #[arg(long)]
        file: Option<PathBuf>,
    },
}
#[derive(Subcommand)]
enum DocumentCommand {
    /// Validate a v2 JSON document and print structured issues.
    Validate { file: PathBuf },
    /// Render a validated v2 document as JSON; does not write to Anki.
    Render { file: PathBuf },
    /// Print the semantic document fingerprint.
    Digest { file: PathBuf },
}
fn read_document(
    path: &PathBuf,
    max_bytes: u64,
    max_chars: usize,
) -> Result<LearningDocument, String> {
    LearningDocument::from_json(&read_input(path, max_bytes, max_chars)?).map_err(|e| e.to_string())
}
fn read_input(path: &PathBuf, max_bytes: u64, max_chars: usize) -> Result<Vec<u8>, String> {
    use std::io::{IsTerminal, Read};
    let mut bytes = Vec::new();
    if path.as_os_str() == "-" {
        let stdin = std::io::stdin();
        if stdin.is_terminal() {
            return Err(
                "INPUT_STDIN_IS_TERMINAL: pipe or redirect one UTF-8 JSON record when using -"
                    .into(),
            );
        }
        stdin
            .lock()
            .take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("INPUT_IO: {e}"))?;
    } else {
        let file = std::fs::File::open(path).map_err(|e| format!("INPUT_IO: {e}"))?;
        if file.metadata().map_err(|e| format!("INPUT_IO: {e}"))?.len() > max_bytes {
            return Err("INPUT_TOO_LARGE: exceeds input.max_file_mb".into());
        }
        file.take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("INPUT_IO: {e}"))?;
    }
    if bytes.len() as u64 > max_bytes {
        return Err("INPUT_TOO_LARGE".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "INPUT_ENCODING")?;
    if text.chars().take(max_chars + 1).count() > max_chars {
        return Err("INPUT_RECORD_TOO_LARGE: exceeds input.max_record_chars".into());
    }
    Ok(bytes)
}
fn emit(value: &impl serde::Serialize) -> Result<(), String> {
    use std::io::Write;
    let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    let output = match output_mode() {
        OutputMode::Human => render_human(&value).into_bytes(),
        OutputMode::Json => serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
        OutputMode::Jsonl => serde_json::to_vec(&value).map_err(|e| e.to_string())?,
    };
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&output)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(output_write_error)
}
fn output_write_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::BrokenPipe {
        "OUTPUT_BROKEN_PIPE".into()
    } else {
        format!("OUTPUT_IO: {error}")
    }
}
fn render_human(value: &serde_json::Value) -> String {
    fn lines(prefix: &str, value: &serde_json::Value, output: &mut String) {
        if let Some(object) = value.as_object() {
            for (key, child) in object {
                let key = key.escape_debug().to_string();
                let path = if prefix.is_empty() {
                    key
                } else {
                    format!("{prefix}.{key}")
                };
                lines(&path, child, output);
            }
        } else {
            if diagnostics::color() {
                output.push_str("\u{1b}[36m");
                output.push_str(prefix);
                output.push_str("\u{1b}[0m");
            } else {
                output.push_str(prefix);
            }
            output.push_str(": ");
            output.push_str(&serde_json::to_string(value).expect("JSON value serializes"));
            output.push('\n');
        }
    }
    let mut output = String::new();
    lines("result", value, &mut output);
    output.pop();
    output
}
fn run(cli: Cli) -> Result<u8, String> {
    set_output_mode(cli.output.unwrap_or(OutputMode::Human));
    if let Command::Completions { shell } = &cli.command {
        use std::io::Write;
        let mut command = Cli::command();
        let name = command.get_name().to_owned();
        // Generate into memory: the generator may panic on writer errors. Handle
        // stdout errors ourselves through the normal CLI error contract.
        let mut script = Vec::new();
        clap_complete::generate(*shell, &mut command, name, &mut script);
        std::io::stdout()
            .lock()
            .write_all(&script)
            .map_err(output_write_error)?;
        return Ok(0);
    }
    if let Command::Config { command } = &cli.command {
        return run_config(&cli, command);
    }
    let settings = load_effective(&cli)?;
    use_settings_output(&cli, &settings)?;
    let max_bytes = settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024;
    let max_chars = settings.values["input.max_record_chars"].as_u64().unwrap() as usize;
    if let Command::Decks { command } = &cli.command {
        match command {
            DeckCommand::Map { .. } => return map_deck(&cli, command, max_bytes, max_chars),
            DeckCommand::Unmap { purpose } => return unmap_deck(&cli, purpose),
            _ => {}
        }
    }
    let vocab_command = matches!(cli.command, Command::Vocab { .. });
    if matches!(
        cli.command,
        Command::Vocab { .. }
            | Command::Grammar { .. }
            | Command::Jobs {
                command: JobCommand::Create { .. } | JobCommand::Run { .. }
            }
    ) {
        linguist_application::duplicate_candidates::require_policy_available(&settings)?;
    }
    if matches!(cli.command, Command::Apply { .. } | Command::Recover { .. }) {
        linguist_application::apply::require_native_adapter(&settings.values)?;
    }
    match &cli.command {
        Command::Cache {
            command: CacheCommand::Status { provider },
        } => return maintenance::cache_status(&settings, provider.as_deref()),
        Command::Cache {
            command:
                CacheCommand::Prune {
                    age_days,
                    budget_mb,
                    provider,
                    execute,
                },
        } => {
            return maintenance::cache_prune(
                &settings,
                provider.as_deref(),
                *age_days,
                *budget_mb,
                *execute,
            );
        }
        Command::Resources {
            command:
                ResourceCommand::List {
                    installed,
                    required,
                    resource,
                },
        } => {
            return maintenance::resources_list(
                &settings,
                *installed,
                *required,
                resource.as_deref(),
            );
        }
        Command::Resources {
            command:
                ResourceCommand::Install {
                    resource,
                    source,
                    version,
                    sha256,
                    license,
                    destination,
                    execute,
                },
        } => {
            return maintenance::resources_install(
                &settings,
                resource,
                source,
                version,
                sha256,
                license,
                destination.as_deref(),
                *execute,
            );
        }
        _ => {}
    }
    match cli.command {
        Command::Config { .. }
        | Command::Completions { .. }
        | Command::Cache { .. }
        | Command::Resources { .. } => unreachable!(),
        Command::Apply {
            plan,
            revision,
            digest,
            item_ids,
            accept_schema_change,
            split_group,
            apply,
        } => run_apply(
            &settings,
            plan,
            revision,
            digest,
            &item_ids,
            split_group,
            accept_schema_change,
            apply,
        ),
        Command::Snapshots { command } => {
            let env: BTreeMap<String, String> = std::env::vars().collect();
            let root = linguist_config::expand_path(
                settings.values["storage.state_dir"].as_str().unwrap(),
                &env,
            )?;
            if !root.is_absolute() {
                return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
            }
            let exists = match std::fs::symlink_metadata(&root) {
                Ok(_) => true,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                Err(_) => return Err("STORE_READ_IO".into()),
            };
            match command {
                SnapshotCommand::List {
                    note_id,
                    job,
                    status,
                    cursor,
                } => {
                    let note_id = note_id
                        .map(|id| linguist_core::AnkiId::try_from(id).map(String::from))
                        .transpose()?;
                    if status
                        .as_deref()
                        .is_some_and(|value| value != "unknown" && value != "receipt_recorded")
                    {
                        return Err("INVALID_SNAPSHOT_STATUS".into());
                    }
                    let after = cursor
                        .map(|value| {
                            uuid::Uuid::parse_str(&value)
                                .map_err(|_| "INVALID_SNAPSHOT_CURSOR".to_owned())
                        })
                        .transpose()?;
                    if !exists {
                        emit(
                            &serde_json::json!({"version":2,"snapshots":[],"next_cursor":null,"state_exists":false}),
                        )?;
                    } else {
                        let store = linguist_store::Store::read_only(&root)?;
                        let limit = settings.values["output.page_size"].as_u64().unwrap() as u32;
                        let (page, next_cursor) = store.snapshot_page(after, limit)?;
                        let mut snapshots = Vec::new();
                        for record in page {
                            if note_id.as_ref().is_some_and(|id| {
                                !record
                                    .snapshot
                                    .originals
                                    .iter()
                                    .any(|source| source.location == format!("anki_note:{id}"))
                            }) {
                                continue;
                            }
                            if status
                                .as_ref()
                                .is_some_and(|value| *value != record.post_state_status)
                            {
                                continue;
                            }
                            if let Some(job) = job {
                                let journal = match store.journal(record.snapshot.operation_id) {
                                    Ok(journal) => journal,
                                    Err(error) if error == "JOURNAL_NOT_FOUND" => continue,
                                    Err(error) => return Err(error),
                                };
                                if journal.journal.snapshot_id != record.snapshot.id
                                    || journal.journal.group_id != Some(job)
                                {
                                    continue;
                                }
                            }
                            snapshots.push(record);
                        }
                        emit(
                            &serde_json::json!({"version":2,"snapshots":snapshots,"next_cursor":next_cursor,"state_exists":true}),
                        )?;
                    }
                }
                SnapshotCommand::Show { snapshot } => {
                    if !exists {
                        return Err("SNAPSHOT_NOT_FOUND".into());
                    }
                    let store = linguist_store::Store::read_only(&root)?;
                    emit(&store.snapshot(snapshot)?)?;
                }
                SnapshotCommand::Restore {
                    snapshot,
                    decision,
                    apply,
                } => {
                    if !exists {
                        return Err("SNAPSHOT_NOT_FOUND".into());
                    }
                    let store = linguist_store::Store::read_only(&root)?;
                    let preview = linguist_application::restore::local_preview(&store, snapshot)?;
                    let decision = decision
                        .map(|path| -> Result<_, String> {
                            let bytes = read_input(&path, max_bytes, max_chars)?;
                            let decision: linguist_application::restore::RestoreDecision =
                                serde_json::from_slice(&bytes)
                                    .map_err(|_| "RESTORE_DECISION_INVALID".to_owned())?;
                            if decision.schema_version != 1 || decision.snapshot_id != snapshot {
                                return Err("RESTORE_DECISION_INVALID".into());
                            }
                            Ok(decision)
                        })
                        .transpose()?;
                    if apply {
                        // ALG-RESTORE is implemented over the native port, but no
                        // tested lab-native-v1 mutation transport exists.
                        return Err("CAPABILITY_UNAVAILABLE: snapshots restore --apply requires the verified native lab-native-v1 mutation adapter; no lease, checkpoint, journal or Anki request was made".into());
                    }
                    let blocked = !preview.blockers.is_empty();
                    emit(&serde_json::json!({
                        "schema_version": 2,
                        "mode": "preview",
                        "live_checked": false,
                        "restore": preview,
                        "decision_digest": decision.as_ref().map(|d| d.digest()).transpose()?,
                        "restore_requirements": [
                            "current invocation --apply",
                            "verified lab-native-v1 bridge on loopback with the same collection identity",
                            "live preview: conflicts against the recorded post-state, each decided in an observed-state-bound decision",
                            "verified checkpoint covering the note, its cards and, for a reverse note-type change, both models",
                            "unknown apply outcomes reconciled first",
                        ],
                        "preserved": [
                            "retained card IDs with current scheduling and review history, including later reviews",
                            "shared note types and collection media (never deleted)",
                            "created notes unless an unchanged, unstudied note is explicitly listed",
                        ],
                        "collection_writes_enabled": false,
                    }))?;
                    return Ok(if blocked { 4 } else { 0 });
                }
                SnapshotCommand::Export { snapshot, output } => {
                    if !exists {
                        return Err("SNAPSHOT_NOT_FOUND".into());
                    }
                    let store = linguist_store::Store::read_only(&root)?;
                    let receipt =
                        linguist_application::export::export_snapshot(&store, snapshot, &output)?;
                    emit(&receipt)?;
                }
            }
            Ok(0)
        }
        Command::Vocab {
            command:
                PrepareCommand::Add {
                    document,
                    format,
                    inline,
                },
        }
        | Command::Grammar {
            command:
                PrepareCommand::Add {
                    document,
                    format,
                    inline,
                },
        } => {
            let kind = if vocab_command {
                linguist_application::Kind::Vocabulary
            } else {
                linguist_application::Kind::Grammar
            };
            let environment = std::env::vars().collect();
            let Some(document) = document else {
                if !inline.present() {
                    return Err(
                        "INPUT_MODE_REQUIRED: supply --document or inline card fields".into(),
                    );
                }
                let input = inline.into_input(kind)?;
                let result = linguist_application::prepare_authored_inline(
                    input,
                    kind,
                    &settings,
                    &environment,
                )?;
                return emit_prepared(
                    &settings,
                    &environment,
                    &result,
                    result.plan_id,
                    result.ready,
                );
            };
            if inline.present() {
                return Err("INPUT_MODE_CONFLICT: --document and inline card fields".into());
            }
            let format = format.unwrap_or(AddFormat::Json);
            let bytes = read_input(
                &document,
                max_bytes,
                if matches!(format, AddFormat::Jsonl | AddFormat::Csv) {
                    max_bytes as usize
                } else {
                    max_chars
                },
            )?;
            match format {
                AddFormat::Json => {
                    let result = linguist_application::prepare_authored(
                        &bytes,
                        kind,
                        &settings,
                        &environment,
                    )?;
                    emit_prepared(
                        &settings,
                        &environment,
                        &result,
                        result.plan_id,
                        result.ready,
                    )
                }
                AddFormat::Jsonl => {
                    let result = linguist_application::prepare_authored_jsonl(
                        &bytes,
                        kind,
                        &settings,
                        &environment,
                    )?;
                    emit_prepared(
                        &settings,
                        &environment,
                        &result,
                        result.plan_id,
                        result.ready,
                    )
                }
                AddFormat::Csv => {
                    let result = linguist_application::prepare_authored_csv(
                        &bytes,
                        kind,
                        &settings,
                        &environment,
                    )?;
                    emit_prepared(
                        &settings,
                        &environment,
                        &result,
                        result.plan_id,
                        result.ready,
                    )
                }
            }
        }
        Command::Vocab {
            command: PrepareCommand::Revamp { selector, limit },
        }
        | Command::Grammar {
            command: PrepareCommand::Revamp { selector, limit },
        } => {
            let purpose = cli
                .purpose
                .as_deref()
                .ok_or("REVAMP_PURPOSE_REQUIRED: select --purpose")?;
            let correct_kind = if vocab_command {
                matches!(purpose, "japanese_vocab" | "english_vocab")
            } else {
                matches!(purpose, "japanese_grammar" | "english_grammar")
            };
            if !correct_kind {
                return Err("REVAMP_PURPOSE_KIND_INVALID".into());
            }
            note_query(&selector, None, &settings)?;
            let selector = if !selector.note_ids.is_empty() {
                linguist_application::revamp::SourceSelector::NoteIds(selector.note_ids)
            } else if let Some(query) = selector.query {
                linguist_application::revamp::SourceSelector::Query(query)
            } else {
                linguist_application::revamp::SourceSelector::Deck(
                    selector.deck.ok_or("NOTE_SELECTOR_REQUIRED")?,
                )
            };
            let client = anki_client(&settings)?;
            let results = linguist_application::revamp::prepare_source_selection_limited(
                &client,
                &settings,
                purpose,
                selector,
                &std::env::vars().collect(),
                limit,
            )?;
            let empty = results.is_empty();
            let result = if results.len() == 1 {
                serde_json::to_value(&results[0]).map_err(|e| e.to_string())?
            } else {
                serde_json::json!({"items":results,"item_count":results.len()})
            };
            let environment: BTreeMap<String, String> = std::env::vars().collect();
            let generation = match results.first() {
                Some(first) if settings.values["llm.enabled"] == true => Some(
                    generate_after_preparation(&settings, &environment, first.plan_id),
                ),
                _ => None,
            };
            emit(
                &serde_json::json!({"preparation_stage":if settings.values["dictionary.provider"] == "authored" || !vocab_command {"source_draft"} else {"source_dictionary_draft"},"result":result,"dictionary_enrichment_completed":settings.values["dictionary.provider"] != "authored" && vocab_command,"enrichment_completed":!empty,"generation":generation,"collection_writes_enabled":false}),
            )?;
            Ok(if empty { 0 } else { 4 })
        }
        Command::Recover {
            command:
                RecoveryCommand::Inspect {
                    operation,
                    pending: _,
                    live,
                },
        } => {
            let environment: BTreeMap<String, String> = std::env::vars().collect();
            let root = linguist_config::expand_path(
                settings.values["storage.state_dir"].as_str().unwrap(),
                &environment,
            )?;
            if !root.is_absolute() {
                return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
            }
            let store = match std::fs::symlink_metadata(&root) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err("STORE_READ_IO".into()),
                Ok(_) => Some(linguist_store::Store::read_only(&root)?),
            };
            let journals = match (store.as_ref(), operation) {
                (Some(store), Some(id)) => vec![store.journal(id)?],
                (Some(store), None) => store.pending_journals(
                    settings.values["output.page_size"].as_u64().unwrap() as u32,
                )?,
                (None, Some(_)) => return Err("JOURNAL_NOT_FOUND".into()),
                (None, None) => vec![],
            };
            let total_pending = store
                .as_ref()
                .map(|s| s.pending_journal_count())
                .transpose()?
                .unwrap_or(0);
            let live_report = if live && !journals.is_empty() {
                Some(match anki_client(&settings) {
                    Ok(client) => recovery_live::inspect(
                        &journals,
                        &client,
                        settings.values["anki.endpoint"].as_str().unwrap(),
                        settings.values["anki.read_batch_size"].as_u64().unwrap() as usize,
                    )?,
                    Err(error) => recovery_live::LiveReport::unavailable(error),
                })
            } else {
                None
            };
            let dependency_unavailable = live_report
                .as_ref()
                .is_some_and(recovery_live::LiveReport::dependency_unavailable);
            let native_status_checked = live_report
                .as_ref()
                .is_some_and(|report| report.status_reads_completed > 0);
            emit(
                &serde_json::json!({"version":2,"journals":journals,"total_pending":total_pending,"live_requested":live,"live_checked":false,"native_status_checked":native_status_checked,"live":live_report,"reconciliation_available":false,"state_exists":store.is_some()}),
            )?;
            Ok(if dependency_unavailable { 3 } else { 0 })
        }
        Command::Recover {
            command:
                RecoveryCommand::Reconcile {
                    operation,
                    apply,
                    rebind,
                },
        } => run_reconcile(&settings, operation, apply, rebind),
        Command::Jobs { command } => {
            if let JobCommand::Rollback {
                job,
                item_ids,
                apply,
            } = command
            {
                return run_rollback(&settings, job, &item_ids, apply);
            }
            let env: BTreeMap<String, String> = std::env::vars().collect();
            if let JobCommand::Create {
                selector,
                limit,
                mode,
                plan,
                revision,
                digest,
                approval,
                checkpoint,
                protected_manifest,
                item_ids,
                accept_schema_change,
            } = command
            {
                if mode != JobModeArg::Prepare {
                    return run_create_apply_job(
                        &settings,
                        &env,
                        mode,
                        plan.ok_or("JOB_PLAN_REQUIRED: simulate/apply jobs need --plan")?,
                        revision,
                        digest.ok_or(
                            "JOB_DIGEST_REQUIRED: select --digest of the approved revision",
                        )?,
                        approval,
                        checkpoint,
                        protected_manifest,
                        item_ids,
                        accept_schema_change,
                    );
                }
                if plan.is_some() {
                    return Err(
                        "JOB_MODE_ARGUMENT_CONFLICT: --plan needs --mode simulate or --mode apply"
                            .into(),
                    );
                }
                let purpose = cli
                    .purpose
                    .as_deref()
                    .ok_or("JOB_PURPOSE_REQUIRED: select --purpose")?;
                let selector = if !selector.note_ids.is_empty() {
                    linguist_application::revamp::SourceSelector::NoteIds(selector.note_ids)
                } else if let Some(query) = selector.query {
                    linguist_application::revamp::SourceSelector::Query(query)
                } else {
                    linguist_application::revamp::SourceSelector::Deck(
                        selector.deck.ok_or("JOB_SELECTOR_REQUIRED")?,
                    )
                };
                emit(&linguist_application::jobs::create_selected(
                    purpose, selector, limit, &settings, &env,
                )?)?;
                return Ok(0);
            }
            let root = linguist_config::expand_path(
                settings.values["storage.state_dir"].as_str().unwrap(),
                &env,
            )?;
            if !root.is_absolute() {
                return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
            }
            if let JobCommand::Migrate {
                legacy: Some(legacy),
                execute,
                ..
            } = &command
            {
                return maintenance::jobs_legacy(&settings, &root, legacy, *execute);
            }
            if let JobCommand::Migrate {
                list_imports,
                show_import,
                ..
            } = &command
                && (*list_imports || show_import.is_some())
            {
                return maintenance::jobs_legacy_list(&root, *show_import);
            }
            if let JobCommand::Migrate { .. } = command {
                std::fs::symlink_metadata(&root).map_err(|_| "STORE_NOT_FOUND")?;
                let _ = linguist_store::Store::open_existing(&root)?;
                emit(
                    &serde_json::json!({"schema_version":2,"state_ready":true,"writes_enabled":false}),
                )?;
                return Ok(0);
            }
            if let JobCommand::Recover {
                job,
                after_index,
                limit,
                actor,
                execute,
            } = command
            {
                let limit =
                    limit.unwrap_or(settings.values["output.page_size"].as_u64().unwrap() as u32);
                emit(&linguist_application::jobs::recover(
                    &root,
                    job,
                    after_index,
                    limit,
                    actor.as_deref(),
                    execute,
                )?)?;
                return Ok(0);
            }
            if let Some(code) = run_job_command(&root, &command)? {
                return Ok(code);
            }
            let control = match &command {
                JobCommand::Pause { job } => Some((
                    *job,
                    linguist_store::preparation_control::ControlAction::Pause,
                )),
                JobCommand::Resume { job, .. } => Some((
                    *job,
                    linguist_store::preparation_control::ControlAction::Resume,
                )),
                JobCommand::Cancel { job } => Some((
                    *job,
                    linguist_store::preparation_control::ControlAction::Cancel,
                )),
                _ => None,
            };
            if matches!(command, JobCommand::Resume { apply: true, .. }) {
                return Err("JOB_MODE_NEVER_WRITES: prepare jobs capture sources only".into());
            }
            if let Some((job, action)) = control {
                // Unknown/missing jobs must never initialize state for a control request.
                linguist_store::Store::read_only(&root)?.preparation_job(job)?;
                let mut writable = linguist_store::Store::open_existing(&root)?;
                let receipt = writable.request_preparation_control(job, action)?;
                if action == linguist_store::preparation_control::ControlAction::Resume {
                    let result = linguist_application::jobs::run(&root, job, &env)?;
                    let exit = result["exit_code"].as_u64().ok_or("JOB_RESULT_INVALID")? as u8;
                    emit(&result)?;
                    return Ok(exit);
                }
                // Confirmed only when no worker holds the lease and nothing is in flight.
                let ack = writable.acknowledge_idle_preparation_stop(job)?;
                emit(
                    &serde_json::json!({"schema_version":2,"job_id":job,"control":receipt,"stop_acknowledgement":ack,"worker_stopped_confirmed":ack.is_some(),"writes_enabled":false}),
                )?;
                return Ok(0);
            }
            if let JobCommand::Run { job, apply } = command {
                if apply {
                    return Err("JOB_MODE_NEVER_WRITES: prepare jobs capture sources only; create an apply job from an approved plan".into());
                }
                let result = linguist_application::jobs::run(&root, job, &env)?;
                let exit = result["exit_code"].as_u64().ok_or("JOB_RESULT_INVALID")? as u8;
                emit(&result)?;
                return Ok(exit);
            }
            let store = match std::fs::symlink_metadata(&root) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err("STORE_READ_IO".into()),
                Ok(_) => Some(linguist_store::Store::read_only(&root)?),
            };
            let page = settings.values["output.page_size"].as_u64().unwrap() as u32;
            match command {
                JobCommand::Audit {
                    job,
                    after_checkpoint,
                    after_control,
                    limit,
                    live,
                } => {
                    if live {
                        return Err("CAPABILITY_UNAVAILABLE: live job audit requires native collection verification".into());
                    }
                    emit(&linguist_application::jobs::audit(
                        store.as_ref().ok_or("PREPARATION_JOB_NOT_FOUND")?,
                        job,
                        after_checkpoint,
                        after_control,
                        limit.unwrap_or(page),
                    )?)?;
                }
                JobCommand::List {
                    after,
                    limit,
                    mode,
                    include_deleted,
                } => {
                    let listed = store
                        .as_ref()
                        .map(|s| {
                            s.list_jobs(
                                after,
                                limit.unwrap_or(page),
                                mode.map(JobModeArg::mode),
                                include_deleted,
                            )
                        })
                        .transpose()?
                        .unwrap_or_default();
                    let next = listed.last().map(|j| j.id);
                    let jobs: Vec<serde_json::Value> = listed
                        .iter()
                        .map(|j| {
                            let mut value = serde_json::to_value(j).unwrap();
                            if j.kind == "prepare" {
                                value["checkpoint_count"] = j.event_count.into();
                            }
                            value
                        })
                        .collect();
                    emit(
                        &serde_json::json!({"schema_version":2,"jobs":jobs,"next_cursor":next,"state_exists":store.is_some(),"execution_available":false}),
                    )?;
                }
                JobCommand::Show { job } => {
                    let definition = store
                        .as_ref()
                        .ok_or("PREPARATION_JOB_NOT_FOUND")?
                        .preparation_job(job)?;
                    let store = store.as_ref().unwrap();
                    emit(
                        &serde_json::json!({"schema_version":2,"mode":"prepare","definition":definition,"control":store.preparation_control(job)?,"stop_acknowledgement":store.preparation_stop_ack(job)?,"worker":store.job_worker_status(job)?,"tombstone":store.job_tombstone(job)?,"worker_liveness":"unverified","execution_available":false,"writes_enabled":false}),
                    )?;
                }
                JobCommand::Items {
                    job,
                    after_index,
                    limit,
                } => {
                    let items = store
                        .as_ref()
                        .ok_or("PREPARATION_JOB_NOT_FOUND")?
                        .preparation_items(job, after_index, limit.unwrap_or(page))?;
                    let next = items.last().map(|i| i.index + 1);
                    emit(
                        &serde_json::json!({"schema_version":2,"job_id":job,"items":items,"next_index":next,"worker_liveness":"unverified","execution_available":false}),
                    )?;
                }
                JobCommand::Create { .. }
                | JobCommand::Retry { .. }
                | JobCommand::Rollback { .. }
                | JobCommand::Delete { .. }
                | JobCommand::Recover { .. }
                | JobCommand::Run { .. }
                | JobCommand::Pause { .. }
                | JobCommand::Resume { .. }
                | JobCommand::Cancel { .. }
                | JobCommand::Migrate { .. } => unreachable!(),
            }
            Ok(0)
        }
        Command::Plans { command } => {
            let env: BTreeMap<String, String> = std::env::vars().collect();
            let root = linguist_config::expand_path(
                settings.values["storage.state_dir"].as_str().unwrap(),
                &env,
            )?;
            if !root.is_absolute() {
                return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
            }
            match std::fs::symlink_metadata(&root) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if matches!(command, PlanCommand::List { .. }) {
                        emit(
                            &serde_json::json!({"version":2,"revisions":[],"state_exists":false}),
                        )?;
                        return Ok(0);
                    }
                    return Err("PLAN_NOT_FOUND".into());
                }
                Err(_) => return Err("STORE_READ_IO".into()),
                Ok(_) => {}
            }
            let store = linguist_store::Store::read_only(&root)?;
            match command {
                PlanCommand::Regenerate {
                    plan,
                    base_revision,
                    digest,
                    item_id,
                    stage,
                    overwrite,
                    use_current_settings,
                } => {
                    use linguist_application::regenerate::{self, Stage};
                    let stage: Stage = stage.parse()?;
                    let overwrite: std::collections::BTreeSet<String> =
                        overwrite.into_iter().collect();
                    let base = store.revision(plan, base_revision)?;
                    if store.latest_revision(plan)? != base_revision
                        || base.approval_digest().map_err(|e| e.to_string())? != digest
                    {
                        return Err("REGENERATE_BASE_CONFLICT".into());
                    }
                    let items: Vec<uuid::Uuid> = item_id.into_iter().collect();
                    let preview = regenerate::preview(&base, &items, stage, &overwrite)?;
                    if !use_current_settings {
                        emit(&serde_json::json!({"preview":preview,"executed":false,
                            "next_command":format!("linguist-anki-bridge plans regenerate {plan} --base-revision {base_revision} --digest {digest} --stage {} --use-current-settings", serde_json::to_value(stage).unwrap().as_str().unwrap())}))?;
                        return Ok(0);
                    }
                    drop(store);
                    let environment: BTreeMap<String, String> = std::env::vars().collect();
                    let mut store = linguist_store::Store::open_existing(&root)?;
                    if stage == Stage::Generation {
                        let item = item_id.ok_or(
                            "REGENERATE_ITEM_REQUIRED: generation regenerates one --item-id",
                        )?;
                        let client =
                            linguist_application::ollama::transport::Client::from_settings(
                                &settings,
                                &environment,
                            )?;
                        let (result, item_preview) = regenerate::regenerate_generation(
                            &mut store,
                            &base,
                            &digest,
                            item,
                            &overwrite,
                            &settings,
                            &environment,
                            &client,
                        )?;
                        emit(
                            &serde_json::json!({"result":result,"preview":item_preview,"executed":true}),
                        )?;
                        return Ok(4);
                    }
                    let (child, preview) = regenerate::regenerate(
                        &mut store,
                        &base,
                        &digest,
                        &items,
                        stage,
                        &overwrite,
                        &settings,
                        &environment,
                        Default::default(),
                    )?;
                    let ready = child.documents.iter().all(linguist_core::validation::ready);
                    emit(
                        &serde_json::json!({"schema_version":2,"plan_id":plan,"revision":child.revision,
                        "digest":child.approval_digest().map_err(|e| e.to_string())?,"preview":preview,"executed":true,
                        "ready":ready,"apply_eligible":false,"writes_enabled":false}),
                    )?;
                    return Ok(if ready { 0 } else { 4 });
                }
                PlanCommand::Generate {
                    plan,
                    item_id,
                    base_revision,
                    digest,
                    use_current_settings,
                } => {
                    if !use_current_settings {
                        return Err("GENERATION_CURRENT_SETTINGS_ACK_REQUIRED: pass --use-current-settings to freeze the resolved configuration into a new revision".into());
                    }
                    let base = store.revision(plan, base_revision)?;
                    if store.latest_revision(plan)? != base_revision
                        || base.approval_digest().map_err(|e| e.to_string())? != digest
                    {
                        return Err("GENERATION_BASE_CONFLICT".into());
                    }
                    drop(store);
                    let client = linguist_application::ollama::transport::Client::from_settings(
                        &settings, &env,
                    )?;
                    let result = linguist_application::generation::publish_candidate(
                        &mut linguist_store::Store::open_existing(&root)?,
                        &base,
                        item_id,
                        &digest,
                        &settings,
                        &env,
                        &client,
                    )?;
                    emit(&result)?;
                    return Ok(4);
                }
                PlanCommand::DuplicateCandidates {
                    plan,
                    item_id,
                    revision,
                    record,
                    digest,
                } => {
                    let revision = revision
                        .map(Ok)
                        .unwrap_or_else(|| store.latest_revision(plan))?;
                    let base = store.revision(plan, revision)?;
                    let client = anki_client(&settings)?;
                    let report = linguist_application::duplicate_candidates::inspect(
                        &base,
                        item_id,
                        &client,
                        settings.values["selection.max_notes"].as_u64().unwrap() as usize,
                        max_chars,
                    )?;
                    if record {
                        drop(store);
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let child = linguist_application::duplicate_candidates::record(
                            &mut linguist_store::Store::open_existing(&root)?,
                            &base,
                            digest.as_deref().unwrap_or_default(),
                            &report,
                            now,
                        )?;
                        let recorded = child.as_ref().map(|child| {
                            serde_json::json!({"revision":child.revision,"digest":child.approval_digest().ok(),
                                "issue_id":format!("COLLECTION_DUPLICATE_REVIEW:{item_id}")})
                        });
                        emit(&serde_json::json!({"report":report,"recorded":recorded}))?;
                        return Ok(if child.is_some() { 4 } else { 0 });
                    }
                    emit(&report)?;
                }
                PlanCommand::SplitGrammar {
                    plan,
                    request,
                    template,
                } => {
                    if let Some(item) = template {
                        let latest = store.latest_revision(plan)?;
                        let base = store.revision(plan, latest)?;
                        let template = linguist_application::grammar::split_template(&base, item)?;
                        emit(&template)?;
                        return Ok(0);
                    }
                    let request = request.ok_or("GRAMMAR_SPLIT_REQUEST_REQUIRED")?;
                    let raw = read_input(&request, max_bytes, max_chars)?;
                    let request: linguist_application::grammar::SplitRequest =
                        canonical::parse(&raw).map_err(|e| e.to_string())?;
                    let base = store.revision(plan, request.base_revision)?;
                    drop(store);
                    let child = linguist_application::grammar::split(
                        &mut linguist_store::Store::open_existing(&root)?,
                        &base,
                        &request,
                        &raw,
                    )?;
                    emit(
                        &serde_json::json!({"schema_version":2,"plan_id":plan,"revision":child.revision,"digest":child.approval_digest().map_err(|e| e.to_string())?,"grammar_groups":child.grammar_groups,"ready":false,"apply_eligible":false,"writes_enabled":false}),
                    )?;
                    return Ok(4);
                }
                PlanCommand::Enrich {
                    plan,
                    base_revision,
                    digest,
                } => {
                    if store.latest_revision(plan)? != base_revision {
                        return Err("DICTIONARY_BASE_CONFLICT".into());
                    }
                    let base = store.revision(plan, base_revision)?;
                    if base.approval_digest().map_err(|e| e.to_string())? != digest {
                        return Err("DICTIONARY_BASE_CONFLICT".into());
                    }
                    drop(store);
                    let child = linguist_application::dictionary::enrich_revision(
                        &mut linguist_store::Store::open_existing(&root)?,
                        &base,
                        None,
                    )?;
                    emit(
                        &serde_json::json!({"schema_version":2,"plan_id":plan,"revision":child.revision,"digest":child.approval_digest().map_err(|e| e.to_string())?,"issues":child.documents.iter().map(|document|serde_json::json!({"document_id":document.id,"issues":document.issues})).collect::<Vec<_>>(),"ready":false,"apply_eligible":false,"writes_enabled":false}),
                    )?;
                    return Ok(4);
                }
                PlanCommand::List {
                    limit,
                    status,
                    workflow,
                } => {
                    let limit = limit
                        .unwrap_or(settings.values["output.page_size"].as_u64().unwrap() as u32);
                    if status
                        .as_deref()
                        .is_some_and(|s| !matches!(s, "ready" | "needs_review" | "invalid"))
                    {
                        return Err("INVALID_PLAN_STATUS_FILTER".into());
                    }
                    if workflow.as_deref().is_some_and(|w| {
                        !matches!(
                            w,
                            "vocab_add"
                                | "vocab_revamp"
                                | "grammar_add"
                                | "grammar_revamp"
                                | "mixed"
                        )
                    }) {
                        return Err("INVALID_PLAN_WORKFLOW_FILTER".into());
                    }
                    let revisions = store.list_revisions(limit)?;
                    if status.is_none() && workflow.is_none() {
                        emit(
                            &serde_json::json!({"version":2,"revisions":revisions,"state_exists":true}),
                        )?;
                        return Ok(0);
                    }
                    // Filters summarize latest revisions only; payloads are not emitted.
                    let mut plans = Vec::new();
                    let mut seen = std::collections::BTreeSet::new();
                    for summary in &revisions {
                        if !seen.insert(summary.id) {
                            continue;
                        }
                        let latest = store.latest_revision(summary.id)?;
                        let plan = store.revision(summary.id, latest)?;
                        let summary = linguist_application::review::summarize(&plan)
                            .map_err(|e| e.to_string())?;
                        if status.as_deref().is_some_and(|s| summary["status"] != s)
                            || workflow
                                .as_deref()
                                .is_some_and(|w| summary["workflow"] != w)
                        {
                            continue;
                        }
                        plans.push(summary);
                    }
                    emit(
                        &serde_json::json!({"version":2,"plans":plans,"state_exists":true,"filtered":true}),
                    )?;
                }
                PlanCommand::Export {
                    plan,
                    revision,
                    output,
                    format,
                    include_private_archives,
                } => {
                    if format == "v1" {
                        return Err(
                            "V1_UNREPRESENTABLE: v2 plan semantics require a v2 export".into()
                        );
                    }
                    if format != "v2" {
                        return Err("UNSUPPORTED_EXPORT_FORMAT".into());
                    }
                    let revision = revision
                        .map(Ok)
                        .unwrap_or_else(|| store.latest_revision(plan))?;
                    let plan = store.revision(plan, revision)?;
                    emit(&linguist_application::export::export_plan(
                        &store,
                        &plan,
                        &output,
                        include_private_archives,
                    )?)?;
                }
                PlanCommand::Resolve {
                    plan,
                    issue,
                    decision,
                } => {
                    let bytes = read_input(&decision, max_bytes, max_chars)?;
                    let request: linguist_core::review::ResolutionRequest =
                        canonical::parse(&bytes).map_err(|e| e.to_string())?;
                    if request.issue_id != issue {
                        return Err("REVIEW_ISSUE_CONFLICT".into());
                    }
                    if store.latest_revision(plan)? != request.base_revision {
                        return Err("REVIEW_BASE_CONFLICT".into());
                    }
                    let base = store.revision(plan, request.base_revision)?;
                    let seconds = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| "CLOCK_UNAVAILABLE")?
                        .as_secs();
                    let result = linguist_application::review::resolve(
                        &store,
                        &base,
                        &request,
                        format!("unix-seconds:{seconds}"),
                    )
                    .map_err(|e| e.to_string())?;
                    drop(store);
                    let digest =
                        linguist_store::Store::open(&root)?.publish_revision(&result.revision)?;
                    emit(
                        &serde_json::json!({"schema_version":2,"plan_id":plan,"revision":result.revision.revision,"digest":digest,"decision_id":result.decision_id,"ready":result.ready,"issues":result.revision.documents.iter().map(|doc|serde_json::json!({"document_id":doc.id,"issues":doc.issues})).collect::<Vec<_>>()}),
                    )?;
                    if !result.ready {
                        return Ok(4);
                    }
                }
                PlanCommand::Approve {
                    plan,
                    revision,
                    digest,
                    actor,
                    item_ids,
                    accepted_warnings,
                } => {
                    let request = linguist_core::approval::ApprovalRequest {
                        plan_id: plan,
                        revision,
                        digest,
                        actor,
                        accepted_warnings,
                        item_ids: if item_ids.is_empty() {
                            None
                        } else {
                            Some(item_ids)
                        },
                    };
                    let base = store.revision(plan, revision)?;
                    linguist_core::approval::build(&base, &request, String::new())
                        .map_err(|e| e.to_string())?;
                    if store.latest_revision(plan)? != revision {
                        return Err("APPROVAL_REVISION_CONFLICT".into());
                    }
                    drop(store);
                    let receipt = linguist_store::Store::open(&root)?.approve_revision(&request)?;
                    emit(
                        &serde_json::json!({"schema_version":2,"noop":receipt.is_none(),"receipt":receipt}),
                    )?;
                }
                PlanCommand::Validate {
                    plan,
                    revision,
                    live,
                    after_index,
                    limit,
                } => {
                    if live && after_index.unwrap_or(0) > 0 && revision.is_none() {
                        return Err("LIVE_VALIDATION_REVISION_REQUIRED_FOR_CURSOR".into());
                    }
                    let revision = revision
                        .map(Ok)
                        .unwrap_or_else(|| store.latest_revision(plan))?;
                    // Complete live reads before persisting local evidence; a transport error
                    // cannot leave a misleading partial live-validation receipt.
                    let base = store.revision(plan, revision)?;
                    let live_page = if live {
                        let client = anki_client(&settings)?;
                        Some(linguist_application::live_validation::inspect(
                            &store,
                            &base,
                            &client,
                            after_index.unwrap_or(0),
                            limit.unwrap_or(
                                settings.values["output.page_size"].as_u64().unwrap() as u32
                            ),
                        )?)
                    } else {
                        None
                    };
                    drop(store);
                    let receipt =
                        linguist_store::Store::open(&root)?.validate_revision(plan, revision)?;
                    let ready = receipt.evidence.content_ready
                        && live_page
                            .as_ref()
                            .is_none_or(|page| !page.source_conflicts && page.all_sources_checked);
                    if let Some(live_page) = live_page {
                        emit(
                            &serde_json::json!({"schema_version":2,"content":receipt,"live":live_page,"apply_eligible":false,"writes_enabled":false}),
                        )?;
                    } else {
                        emit(&receipt)?;
                    }
                    if !ready {
                        return Ok(4);
                    }
                }
                PlanCommand::Edit {
                    plan,
                    base_revision,
                    patch,
                    editor,
                    save_draft,
                } => {
                    if store.latest_revision(plan)? != base_revision {
                        return Err("PLAN_EDIT_BASE_CONFLICT: select the latest revision".into());
                    }
                    let base = store.revision(plan, base_revision)?;
                    let patch: linguist_core::editing::PlanPatch = if editor {
                        let environment: BTreeMap<String, String> = std::env::vars().collect();
                        let configured: Vec<String> = settings.values["editing.editor_argv"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let argv =
                            linguist_application::editor::editor_argv(&configured, &environment)?;
                        let temp_root = linguist_config::expand_path(
                            settings.values["storage.temp_dir"].as_str().unwrap(),
                            &environment,
                        )?;
                        let workspace = linguist_provider::process::TempDir::create_in(&temp_root)
                            .map_err(|_| "PLAN_EDIT_DRAFT_IO")?;
                        let path = workspace.0.join("plan-edit.json");
                        let draft = linguist_application::editor::draft(&base)?;
                        linguist_provider::process::write_private(&path, &draft)
                            .map_err(|_| "PLAN_EDIT_DRAFT_IO")?;
                        linguist_application::editor::launch(&argv, &path)?;
                        let edited = read_input(&path, max_bytes, max_chars)?;
                        if edited == draft {
                            emit(
                                &serde_json::json!({"version":2,"plan_id":plan,"revision":base_revision,"changed":false,"aborted":false}),
                            )?;
                            return Ok(0);
                        }
                        linguist_application::editor::parse(&edited)?
                    } else {
                        let bytes = read_input(
                            &patch.ok_or("PLAN_EDIT_PATCH_REQUIRED")?,
                            max_bytes,
                            max_chars,
                        )?;
                        canonical::parse(&bytes).map_err(|e| e.to_string())?
                    };
                    let result = linguist_core::editing::apply_patch(&base, &patch, save_draft)
                        .map_err(|e| e.to_string())?;
                    let digest = if result.changed {
                        drop(store);
                        linguist_store::Store::open(&root)?.publish_revision(&result.revision)?
                    } else {
                        base.approval_digest().map_err(|e| e.to_string())?
                    };
                    emit(
                        &serde_json::json!({"version":2,"plan_id":plan,"revision":result.revision.revision,"digest":digest,"changed":result.changed,"ready":result.ready,"invalidated_review_ids":result.invalidated_review_ids,"issues":result.revision.documents.iter().map(|doc| serde_json::json!({"document_id":doc.id,"issues":doc.issues})).collect::<Vec<_>>()}),
                    )?;
                    if result.changed && !result.ready {
                        return Ok(4);
                    }
                }
                PlanCommand::Diff {
                    plan,
                    from_revision,
                    revision,
                    live,
                    after_index,
                    limit,
                } => {
                    if live && after_index.unwrap_or(0) > 0 && revision.is_none() {
                        return Err("LIVE_DIFF_REVISION_REQUIRED_FOR_CURSOR".into());
                    }
                    let revision = revision
                        .map(Ok)
                        .unwrap_or_else(|| store.latest_revision(plan))?;
                    let before = store.revision(plan, from_revision)?;
                    let after = store.revision(plan, revision)?;
                    let captured =
                        linguist_application::plan_diff::revision_diff(&store, &before, &after)?;
                    if live {
                        let client = anki_client(&settings)?;
                        let page = linguist_application::live_validation::inspect(
                            &store,
                            &after,
                            &client,
                            after_index.unwrap_or(0),
                            limit.unwrap_or(
                                settings.values["output.page_size"].as_u64().unwrap() as u32
                            ),
                        )?;
                        let incomplete = !page.all_sources_checked;
                        let conflict = page.source_conflicts;
                        emit(
                            &serde_json::json!({"schema_version":2,"captured":captured,"live":page,"apply_eligible":false,"writes_enabled":false}),
                        )?;
                        if incomplete || conflict {
                            return Ok(4);
                        }
                    } else {
                        emit(&captured)?;
                    }
                }
                PlanCommand::Show {
                    plan,
                    revision,
                    item,
                    issues_only,
                    after_index,
                    limit,
                } => {
                    let revision = revision
                        .map(Ok)
                        .unwrap_or_else(|| store.latest_revision(plan))?;
                    let saved = store.revision(plan, revision)?;
                    if issues_only {
                        let limit =
                            limit.unwrap_or(
                                settings.values["output.page_size"].as_u64().unwrap() as u32
                            );
                        emit(&linguist_application::review::inspection::page(
                            &saved,
                            item,
                            after_index.unwrap_or(0),
                            limit,
                        )?)?;
                    } else if let Some(id) = item {
                        let document = saved
                            .documents
                            .iter()
                            .find(|document| document.id == id)
                            .ok_or("PLAN_DOCUMENT_NOT_FOUND")?;
                        emit(
                            &serde_json::json!({"schema_version":2,"plan_id":saved.id,"revision":saved.revision,"digest":saved.approval_digest().map_err(|e| e.to_string())?,"document":document,"archives_included":true,"native_verified":false}),
                        )?;
                    } else {
                        emit(&saved)?;
                    }
                }
            }
            Ok(0)
        }
        Command::Doctor {
            local,
            ollama,
            bridge,
        } => {
            if local {
                let environment: BTreeMap<String, String> = std::env::vars().collect();
                let resources = linguist_config::resources::inspect(&settings, &environment);
                let engines = local_engines(&settings, &environment);
                let required_missing = !resources.required_missing.is_empty()
                    || engines
                        .iter()
                        .any(|check| check["required"] == true && check["status"] != "available");
                emit(
                    &serde_json::json!({"version":2,"collection_writes_enabled":false,"native_bridge":"not_implemented","release_gates":"not_run","services_probed":false,"offline_requested":cli.offline,"local_resources":resources,"local_engines":engines}),
                )?;
                return Ok(if required_missing { 3 } else { 0 });
            }
            if ollama {
                let client = linguist_application::ollama::transport::Client::from_settings(
                    &settings,
                    &std::env::vars().collect(),
                )?;
                let evidence = client.model_evidence()?;
                emit(
                    &serde_json::json!({"version":2,"probe":"ollama_metadata","ollama":evidence,
                    "metadata_ready":true,"collection_writes_enabled":false,"release_gates":"not_run",
                    "raw_assets_persisted":false}),
                )?;
                return Ok(0);
            }
            if bridge {
                let inspection = anki_client(&settings)?.native_capabilities()?;
                emit(
                    &serde_json::json!({"version":2,"probe":"native_bridge","native_bridge":inspection,"release_gates":"not_run"}),
                )?;
                return Ok(0);
            }
            let client = anki_client(&settings)?;
            let capabilities = client.capabilities()?;
            let ready = capabilities.read_ready;
            emit(
                &serde_json::json!({"version":2,"anki":capabilities,"provider_resources_checked":false,"release_gates":"not_run"}),
            )?;
            Ok(if ready { 0 } else { 3 })
        }
        Command::Backup { command } => run_backup(command, &settings),
        Command::Models {
            command: None | Some(ModelCommand::Builtin),
        } => {
            emit(&vec![model::vocabulary(), model::grammar()])?;
            Ok(0)
        }
        Command::Models {
            command: Some(ModelCommand::List),
        } => {
            let client = anki_client(&settings)?;
            emit(
                &serde_json::json!({"version":2,"models":client.models()?,"managed_classification_verified":false}),
            )?;
            Ok(0)
        }
        Command::Models {
            command: Some(ModelCommand::Inspect { model }),
        } => {
            let client = anki_client(&settings)?;
            emit(&client.inspect_model(&model)?)?;
            Ok(0)
        }
        Command::Models {
            command: Some(ModelCommand::Install { purpose, apply }),
        } => {
            let target = match purpose.as_str() {
                "japanese_vocab" | "english_vocab" => model::vocabulary(),
                "japanese_grammar" | "english_grammar" => model::grammar(),
                _ => return Err("MODEL_PURPOSE_UNSUPPORTED".into()),
            };
            if apply {
                // The journaled installer (linguist_application::model_install) exists, but
                // no tested native install_model adapter does; nothing is dispatched.
                return Err("CAPABILITY_UNAVAILABLE: models install --apply requires the verified native install_model and export_checkpoint adapters; no lease, checkpoint, journal or Anki request was made".into());
            }
            let client = anki_client(&settings)?;
            let existing = client
                .models()?
                .into_iter()
                .find(|entry| entry.name == target.name);
            let (action, inspection) = if let Some(existing) = existing {
                let inspected = client.inspect_model(&existing.id)?;
                let exact = inspected
                    .compatibility
                    .iter()
                    .any(|comparison| comparison.name_matches && comparison.exact_content_match);
                (
                    if exact {
                        "reuse_requires_native_order_verification"
                    } else {
                        "name_collision"
                    },
                    Some(inspected),
                )
            } else {
                ("create", None)
            };
            client.check_profile()?;
            emit(
                &serde_json::json!({"schema_version":2,"purpose":purpose,"proposal":action,"target":target,"existing":inspection,"checkpoint_verified":false,"apply_requires":{"checkpoint":{"package_scope":"collection","scheduling":true,"media":true,"schema":true,"restoration_tested":true},"journal_before_call":true,"reconcile_by":"name+exact manifest+operation evidence"},"native_template_order_verified":false,"apply_eligible":false,"writes_enabled":false}),
            )?;
            Ok(if action == "name_collision" { 4 } else { 0 })
        }
        Command::Decks { command } => {
            if let DeckCommand::List { limit, .. } = &command {
                page_limit(*limit, &settings)?;
            }
            if let DeckCommand::Show { deck } = &command
                && (deck.chars().count() > max_chars || deck.chars().any(char::is_control))
            {
                return Err("INVALID_DECK_SELECTOR".into());
            }
            let client = anki_client(&settings)?;
            let decks = client.decks()?;
            match command {
                DeckCommand::Map { .. } | DeckCommand::Unmap { .. } => unreachable!(),
                DeckCommand::List {
                    counts,
                    limit,
                    cursor,
                    name_contains,
                } => {
                    let limit = page_limit(limit, &settings)?;
                    if name_contains.as_ref().is_some_and(|value| {
                        value.chars().any(char::is_control) || value.chars().count() > max_chars
                    }) {
                        return Err("INVALID_DECK_FILTER".into());
                    }
                    let decks = decks
                        .into_iter()
                        .filter(|deck| {
                            name_contains
                                .as_ref()
                                .is_none_or(|filter| deck.name.contains(filter))
                        })
                        .collect::<Vec<_>>();
                    let fingerprint =
                        canonical::digest("deck-selection", &decks).map_err(|e| e.to_string())?;
                    let start = if let Some(cursor) = cursor {
                        let (digest, last) = cursor.rsplit_once('/').ok_or("INVALID_CURSOR")?;
                        if digest != fingerprint {
                            return Err(
                                "DECK_SELECTION_CONFLICT: inventory changed; restart listing"
                                    .into(),
                            );
                        }
                        decks
                            .iter()
                            .position(|deck| deck.id == last)
                            .ok_or("INVALID_CURSOR")?
                            + 1
                    } else {
                        0
                    };
                    let total = decks.len();
                    let mut rows = Vec::new();
                    for deck in decks.iter().skip(start).take(limit) {
                        let count = if counts {
                            Some(client.counts(&linguist_anki::deck_query(&deck.name)?)?)
                        } else {
                            None
                        };
                        rows.push(serde_json::json!({"id":deck.id,"name":deck.name,"counts":count,"counts_include_subdecks":counts}));
                    }
                    let end = (start + rows.len()).min(total);
                    let next_cursor = if end < total {
                        Some(format!("{fingerprint}/{}", decks[end - 1].id))
                    } else {
                        None
                    };
                    emit(
                        &serde_json::json!({"version":2,"decks":rows,"total":total,"truncated":end<total,"next_cursor":next_cursor,"selection_digest":fingerprint}),
                    )?;
                }
                DeckCommand::Show { deck } => {
                    let deck = linguist_anki::select_name(decks, &deck)?;
                    let query = linguist_anki::deck_query(&deck.name)?;
                    let counts = client.counts(&query)?;
                    let note_ids = client.find_notes(&query)?;
                    if note_ids.len() != counts.note_count {
                        return Err("ANKI_DECK_SELECTION_CHANGED".into());
                    }
                    if note_ids.len() > 100000 {
                        return Err(
                            "ANKI_DECK_MODEL_SCAN_LIMIT: narrow the deck before inspection".into(),
                        );
                    }
                    let mut models = BTreeMap::<String, usize>::new();
                    for chunk in note_ids.chunks(100) {
                        for note in client.notes_info(chunk)? {
                            let name = note["modelName"].as_str().ok_or("ANKI_NOTE_INVALID")?;
                            *models.entry(name.to_owned()).or_default() += 1;
                        }
                    }
                    let mut subdecks = client
                        .decks()?
                        .into_iter()
                        .filter(|d| d.name.starts_with(&format!("{}::", deck.name)))
                        .collect::<Vec<_>>();
                    subdecks.sort_by(|a, b| a.name.cmp(&b.name));
                    let purpose_mappings =
                        deck_purpose_mappings(cli.config.as_deref(), &deck.name)?;
                    emit(
                        &serde_json::json!({"version":2,"deck":deck,"counts":counts,"counts_include_subdecks":true,"subdecks":subdecks,"models_by_note_count":models,"mixed_models":models.len()>1,"purpose_mappings":purpose_mappings,"source_model_manifest_checked":false}),
                    )?;
                }
            }
            Ok(0)
        }
        Command::Notes { command } => {
            match command {
                NoteCommand::Show { note_id, media } => {
                    linguist_anki::wire_id(&serde_json::json!(note_id))?;
                    let client = anki_client(&settings)?;
                    let rows = client.notes_info(std::slice::from_ref(&note_id))?;
                    let note = rows
                        .into_iter()
                        .next()
                        .filter(|v| v.get("noteId").is_some())
                        .ok_or("ANKI_NOTE_NOT_FOUND")?;
                    let note = linguist_anki::normalize_note_ids(note)?;
                    if note["noteId"] != note_id {
                        return Err("ANKI_NOTE_ID_CONFLICT".into());
                    }
                    let fields = note["fields"]
                        .as_object()
                        .ok_or("ANKI_NOTE_INVALID")?
                        .iter()
                        .map(|(name, field)| {
                            field["value"]
                                .as_str()
                                .map(|value| (name.clone(), value.to_owned()))
                                .ok_or("ANKI_NOTE_INVALID")
                        })
                        .collect::<Result<BTreeMap<_, _>, _>>()?;
                    if fields
                        .values()
                        .map(|value| value.chars().count())
                        .sum::<usize>()
                        > max_chars
                    {
                        return Err("INPUT_RECORD_TOO_LARGE".into());
                    }
                    let parsed_media = linguist_application::capture::discover_media(
                        &fields,
                        max_bytes.min(100 * 1024 * 1024),
                        10000,
                    )?;
                    let source_mapping = cli
                        .purpose
                        .as_deref()
                        .map(|purpose| {
                            linguist_application::mapping::map_purpose_fields(
                                &settings,
                                purpose,
                                note["modelName"].as_str().ok_or("ANKI_NOTE_INVALID")?,
                                &fields,
                            )
                        })
                        .transpose()?;
                    let cards = note["cards"]
                        .as_array()
                        .ok_or("ANKI_NOTE_INVALID")?
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .ok_or("ANKI_NOTE_INVALID".to_owned())
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let cards = client
                        .cards_info(&cards)?
                        .into_iter()
                        .map(linguist_anki::normalize_card_ids)
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut media_metadata = Vec::new();
                    if media {
                        let filenames = parsed_media
                            .references
                            .iter()
                            .map(|r| r.filename.as_str())
                            .collect::<std::collections::BTreeSet<_>>();
                        for filename in filenames {
                            let original = client.retrieve_media_file(filename)?;
                            media_metadata.push(match original {
                                Some(original) => serde_json::json!({"filename":original.filename,"exists":true,"digest":original.digest,"size_bytes":original.bytes.len(),"content_validated":false}),
                                None => serde_json::json!({"filename":filename,"exists":false,"content_validated":false}),
                            });
                        }
                    }
                    let media_bytes_checked = media
                        && parsed_media.issues.is_empty()
                        && media_metadata.iter().all(|item| item["exists"] == true);
                    emit(
                        &serde_json::json!({"version":2,"note":note,"cards":cards,"parsed_media":parsed_media,"source_mapping":source_mapping,"identity_confidence":"weak","media_metadata":media_metadata,"media_inspection_requested":media,"media_bytes_checked":media_bytes_checked,"media_content_validated":false,"native_history_checked":false}),
                    )?;
                }
                NoteCommand::Count { selector } => {
                    let query = note_query(&selector, cli.purpose.as_deref(), &settings)?;
                    linguist_application::selector::require_explicit_matches(
                        &selector.note_ids,
                        &selector.note_ids,
                    )?;
                    let client = anki_client(&settings)?;
                    if !selector.note_ids.is_empty() {
                        let found = client.find_notes(&query)?;
                        linguist_application::selector::require_explicit_matches(
                            &selector.note_ids,
                            &found,
                        )?;
                    }
                    emit(
                        &serde_json::json!({"version":2,"counts":client.counts(&query)?,"identity_confidence":"weak"}),
                    )?;
                }
                NoteCommand::List {
                    selector,
                    limit,
                    cursor,
                } => {
                    let query = note_query(&selector, cli.purpose.as_deref(), &settings)?;
                    let limit = page_limit(limit, &settings)?;
                    linguist_application::selector::require_explicit_matches(
                        &selector.note_ids,
                        &selector.note_ids,
                    )?;
                    if cursor
                        .as_ref()
                        .is_some_and(|c| c.rsplit_once('/').is_none())
                    {
                        return Err("INVALID_CURSOR".into());
                    }
                    let client = anki_client(&settings)?;
                    let ids = client.find_notes(&query)?;
                    linguist_application::selector::require_explicit_matches(
                        &selector.note_ids,
                        &ids,
                    )?;
                    let fingerprint = canonical::digest("note-selection", &(query, &ids))
                        .map_err(|e| e.to_string())?;
                    let start = if let Some(cursor) = cursor {
                        let (digest, last) = cursor.rsplit_once('/').ok_or("INVALID_CURSOR")?;
                        if digest != fingerprint {
                            return Err(
                                "NOTE_SELECTION_CONFLICT: selection changed; restart listing"
                                    .into(),
                            );
                        }
                        ids.iter()
                            .position(|id| id == last)
                            .ok_or("INVALID_CURSOR")?
                            + 1
                    } else {
                        0
                    };
                    let end = (start + limit).min(ids.len());
                    let page = &ids[start..end];
                    let notes = client.notes_info(page)?;
                    let mut summaries = Vec::new();
                    for row in notes {
                        let row = linguist_anki::normalize_note_ids(row)?;
                        let id = row
                            .get("noteId")
                            .and_then(serde_json::Value::as_str)
                            .ok_or("ANKI_NOTE_CAPTURE_CONFLICT")?;
                        if !page.iter().any(|v| v == id) {
                            return Err("ANKI_NOTE_CAPTURE_CONFLICT".into());
                        }
                        summaries.push(serde_json::json!({"note_id":id,"model_name":row["modelName"],"card_count":row["cards"].as_array().ok_or("ANKI_NOTE_INVALID")?.len()}));
                    }
                    let next_cursor = if end < ids.len() {
                        Some(format!("{fingerprint}/{}", ids[end - 1]))
                    } else {
                        None
                    };
                    emit(
                        &serde_json::json!({"version":2,"notes":summaries,"total_notes":ids.len(),"next_cursor":next_cursor,"selection_digest":fingerprint,"identity_confidence":"weak"}),
                    )?;
                }
            }
            Ok(0)
        }
        Command::Document { command } => match command {
            DocumentCommand::Validate { file } => {
                let doc = read_document(&file, max_bytes, max_chars)?;
                let issues = validation::validate(&doc);
                let ready = issues
                    .iter()
                    .all(|i| i.severity == validation::Severity::Warning);
                emit(
                    &serde_json::json!({"schema_version":2,"document_id":doc.id,"ready":ready,"issues":issues}),
                )?;
                Ok(if ready {
                    0
                } else if issues
                    .iter()
                    .any(|i| i.severity == validation::Severity::Error)
                {
                    2
                } else {
                    4
                })
            }
            DocumentCommand::Render { file } => {
                let doc = read_document(&file, max_bytes, max_chars)?;
                let rendered = render::render(&doc, &BTreeMap::new()).map_err(|e| e.to_string())?;
                emit(&rendered)?;
                Ok(0)
            }
            DocumentCommand::Digest { file } => {
                let doc = read_document(&file, max_bytes, max_chars)?;
                emit(
                    &serde_json::json!({"format":canonical::FORMAT,"digest":doc.semantic_digest().map_err(|e|e.to_string())?}),
                )?;
                Ok(0)
            }
        },
    }
}
fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => match error.kind() {
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                return match error.print() {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(io_error) if io_error.kind() == std::io::ErrorKind::BrokenPipe => {
                        ExitCode::SUCCESS
                    }
                    Err(_) => ExitCode::from(6),
                };
            }
            _ => {
                let _ =
                    writeln_error("USAGE: invalid command syntax; run linguist-anki-bridge --help");
                return ExitCode::from(2);
            }
        },
    };
    let started = std::time::Instant::now();
    let label = command_label();
    match run(cli) {
        Ok(code) => {
            diagnostics::record(&label, code, None, started.elapsed());
            ExitCode::from(code)
        }
        Err(message) if message == "OUTPUT_BROKEN_PIPE" => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln_error(&message);
            let code = error_exit(&message);
            diagnostics::record(&label, code, Some(&message), started.elapsed());
            ExitCode::from(code)
        }
    }
}
/// Subcommand names only (e.g. `jobs migrate`); argument values are never logged.
fn command_label() -> String {
    let Ok(matches) = Cli::command().try_get_matches_from(std::env::args_os()) else {
        return String::new();
    };
    let mut names = Vec::new();
    let mut current = &matches;
    while let Some((name, next)) = current.subcommand() {
        names.push(name.to_owned());
        current = next;
    }
    names.join(" ")
}
fn writeln_error(message: &str) -> std::io::Result<()> {
    use std::io::Write;
    writeln!(
        std::io::stderr(),
        "{}",
        serde_json::json!({"error":message})
    )
}

fn config_options(
    cli: &Cli,
    registry: &linguist_config::Registry,
) -> Result<linguist_config::ResolveOptions, String> {
    let mut flags = BTreeMap::new();
    for item in &cli.settings {
        let (key, value) = item
            .split_once('=')
            .ok_or("INVALID_OVERRIDE: expected KEY=VALUE")?;
        if flags
            .insert(key.to_owned(), registry.parse_value(key, value)?)
            .is_some()
        {
            return Err(format!("DUPLICATE_OVERRIDE: {key}"));
        }
    }
    Ok(linguist_config::ResolveOptions {
        profile: cli.profile.clone(),
        purpose: cli.purpose.clone(),
        environment: std::env::vars().collect(),
        flags,
    })
}
fn apply_offline_flag(
    cli: &Cli,
    options: &mut linguist_config::ResolveOptions,
) -> Result<(), String> {
    if cli.offline {
        if options.flags.contains_key("network.offline") {
            return Err("OFFLINE_OVERRIDE_CONFLICT: --offline and --set network.offline".into());
        }
        options
            .flags
            .insert("network.offline".into(), serde_json::json!(true));
    }
    Ok(())
}
fn use_settings_output(cli: &Cli, settings: &linguist_config::Effective) -> Result<(), String> {
    diagnostics::configure(settings);
    if cli.output.is_none() {
        set_output_mode(OutputMode::from_setting(&settings.values["output.format"])?);
    }
    Ok(())
}
fn initial_config_setup(
    registry: &linguist_config::Registry,
    environment: &BTreeMap<String, String>,
) -> Result<Vec<serde_json::Value>, String> {
    let candidate = linguist_config::ConfigFile::parse("[config]\nversion = 2\n", registry)?;
    linguist_config::resolve(
        registry,
        &candidate,
        &linguist_config::ResolveOptions::default(),
    )?;
    let mut setup = Vec::new();
    for purpose in linguist_config::builtin_purposes() {
        let effective = linguist_config::resolve(
            registry,
            &candidate,
            &linguist_config::ResolveOptions {
                purpose: Some(purpose.clone()),
                environment: environment.clone(),
                ..Default::default()
            },
        )?;
        let key = |name: &str| format!("purposes.{purpose}.{name}");
        let missing = |names: &[&str]| {
            names
                .iter()
                .map(|name| key(name))
                .filter(|name| {
                    effective.values.get(name).is_none_or(|value| {
                        value.is_null()
                            || value.as_object().is_some_and(serde_json::Map::is_empty)
                            || value.as_array().is_some_and(Vec::is_empty)
                    })
                })
                .collect::<Vec<_>>()
        };
        let resources = linguist_config::resources::inspect(&effective, environment);
        setup.push(serde_json::json!({
            "purpose":purpose,
            "target_language":effective.values[&key("target_language")],
            "add_missing_mapping_keys":missing(&["target_deck"]),
            "revamp_missing_mapping_keys":missing(&["source_deck","source_model","fields","card_tasks"]),
            "model_candidate":effective.values["llm.model"],
            "model_verified":false,
            "required_missing_local_resources":resources.required_missing,
            "runtime_resources_checked":false
        }));
    }
    Ok(setup)
}
fn run_config(cli: &Cli, command: &ConfigCommand) -> Result<u8, String> {
    use linguist_config::{ConfigFile, Registry, config_path, resolve};
    let registry = Registry::builtin();
    match command {
        ConfigCommand::Describe { key } => {
            let entry = registry.lookup(key).map_err(|_| {
                let names = registry.suggestions(key);
                if names.is_empty() {
                    format!("UNKNOWN_SETTING: {key}")
                } else {
                    format!(
                        "UNKNOWN_SETTING: {key}; nearest valid names: {}",
                        names.join(", ")
                    )
                }
            })?;
            let mut description = serde_json::to_value(entry).map_err(|e| e.to_string())?;
            description["version"] = serde_json::json!(2);
            description["resolved_key"] = serde_json::json!(key);
            description["cross_field_checks"] =
                serde_json::json!(linguist_config::cross_field_checks(key));
            description["coverage"] = serde_json::json!(linguist_config::coverage::row(key));
            emit(&description)?;
            return Ok(0);
        }
        ConfigCommand::Show {
            key,
            defaults: true,
            ..
        } => {
            let mut values = registry.defaults();
            select_values(&mut values, key.as_deref())?;
            emit(&serde_json::json!({"version":2,"values":values}))?;
            return Ok(0);
        }
        _ => {}
    }
    if let ConfigCommand::Import {
        file,
        output,
        replace,
    } = command
    {
        if !cli.settings.is_empty() || cli.profile.is_some() || cli.purpose.is_some() || cli.offline
        {
            return Err("CONFIG_IMPORT_SCOPE: import converts the legacy file only; --set, --profile, --purpose and --offline do not apply".into());
        }
        return maintenance::config_import(cli, file, output, *replace);
    }
    let mut options = config_options(cli, &registry)?;
    if matches!(
        command,
        ConfigCommand::Show { .. } | ConfigCommand::Validate { .. }
    ) {
        apply_offline_flag(cli, &mut options)?;
    }
    let explicit = match command {
        ConfigCommand::Init { path, .. } => path.as_deref().or(cli.config.as_deref()),
        ConfigCommand::Validate { file } => file.as_deref().or(cli.config.as_deref()),
        _ => cli.config.as_deref(),
    };
    let path = config_path(explicit, &options.environment)?;
    if matches!(command, ConfigCommand::Init { .. }) {
        if !options.flags.is_empty() || options.profile.is_some() || options.purpose.is_some() {
            return Err(
                "CONFIG_INIT_SCOPE: init writes only version 2; set overrides separately".into(),
            );
        }
        let setup = initial_config_setup(&registry, &options.environment)?;
        if cli.output.is_none()
            && let Some(format) = options.environment.get("LAB_OUTPUT__FORMAT")
        {
            set_output_mode(OutputMode::from_setting(&serde_json::json!(format))?);
        }
        let replace = matches!(command, ConfigCommand::Init { replace: true, .. });
        let backup = if replace {
            Some(linguist_config::edit::replace_with_minimal(
                &path,
                &options.environment,
            )?)
        } else {
            linguist_config::initialize(&path)?;
            None
        };
        emit(
            &serde_json::json!({"version":2,"path":path,"created":!replace,"replaced":replace,"backup":backup,"purpose_setup":setup,"next_commands":{"available":["config show","config validate"],"planned":["decks map PURPOSE"]}}),
        )?;
        return Ok(0);
    }
    if let ConfigCommand::Migrate {
        from_import: Some(candidate),
        accept_unresolved,
        execute,
        ..
    } = command
    {
        if !options.flags.is_empty() || options.profile.is_some() || options.purpose.is_some() {
            return Err("CONFIG_MIGRATE_SCOPE: migration reads the durable file only".into());
        }
        return maintenance::config_activate(cli, candidate, accept_unresolved, *execute);
    }
    if let ConfigCommand::Migrate {
        output, execute, ..
    } = command
    {
        if !options.flags.is_empty() || options.profile.is_some() || options.purpose.is_some() {
            return Err("CONFIG_MIGRATE_SCOPE: migration reads the durable file only".into());
        }
        let file = ConfigFile::read(&path, &registry)?;
        let effective = resolve(
            &registry,
            &file,
            &linguist_config::ResolveOptions::default(),
        )?;
        use_settings_output(cli, &effective)?;
        emit(&serde_json::json!({
            "version":2,
            "source":path,
            "output":output,
            "source_version":2,
            "target_version":2,
            "changed":false,
            "requested_execute":execute,
            "executed":false,
            "candidate_written":false,
            "reason":"already_current"
        }))?;
        return Ok(0);
    }
    let edit_change = match command {
        ConfigCommand::Set { key, value } => Some((
            linguist_config::edit::Change::Set {
                key: key.clone(),
                value: registry.parse_value(key, value)?,
            },
            true,
        )),
        ConfigCommand::Unset { key } => Some((
            linguist_config::edit::Change::Unset { key: key.clone() },
            true,
        )),
        ConfigCommand::Reset { key, all, execute } => Some((
            linguist_config::edit::Change::Reset {
                key: key.clone(),
                all: *all,
            },
            *execute,
        )),
        _ => None,
    };
    if let Some((change, execute)) = edit_change {
        if !options.flags.is_empty() {
            return Err(
                "EDIT_OVERRIDE_CONFLICT: --set cannot be combined with durable config edits".into(),
            );
        }
        let receipt = linguist_config::edit::edit(
            &path,
            &linguist_config::edit::Scope {
                profile: cli.profile.clone(),
                purpose: cli.purpose.clone(),
            },
            &change,
            execute,
            &options.environment,
        )?;
        use_settings_output(cli, &receipt.effective)?;
        emit(&receipt)?;
        return Ok(0);
    }
    let file = match std::fs::symlink_metadata(&path) {
        Ok(_) => ConfigFile::read(&path, &registry)?,
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                && explicit.is_none()
                && !options.environment.contains_key("LAB_CONFIG") =>
        {
            ConfigFile::default()
        }
        Err(_) => {
            return Err(
                "CONFIG_IO: selected configuration does not exist or cannot be inspected".into(),
            );
        }
    };
    let mut effective = resolve(&registry, &file, &options)?;
    use_settings_output(cli, &effective)?;
    match command {
        ConfigCommand::Validate { .. } => {
            let resources = linguist_config::resources::inspect(&effective, &options.environment);
            let unavailable = linguist_config::coverage::unavailable_settings(&effective);
            let missing = !resources.missing.is_empty() || !unavailable.is_empty();
            emit(
                &serde_json::json!({"version":2,"valid":true,"fingerprint":effective.fingerprint,"runtime_resources_checked":false,"local_resources":resources,"unavailable_settings":unavailable}),
            )?;
            return Ok(if missing { 3 } else { 0 });
        }
        ConfigCommand::Show {
            key, provenance, ..
        } => {
            select_values(&mut effective.values, key.as_deref())?;
            effective
                .provenance
                .retain(|k, _| effective.values.contains_key(k));
            if !provenance {
                effective.provenance.clear();
            }
            emit(&effective)?;
        }
        _ => unreachable!(),
    }
    Ok(0)
}
fn select_values(
    values: &mut BTreeMap<String, serde_json::Value>,
    key: Option<&str>,
) -> Result<(), String> {
    if let Some(key) = key {
        values.retain(|k, _| k == key || k.starts_with(&format!("{key}.")));
        if values.is_empty() {
            return Err(format!("UNKNOWN_SETTING_SELECTOR: {key}"));
        }
    }
    Ok(())
}

fn error_exit(message: &str) -> u8 {
    let code = message.split(':').next().unwrap_or(message);
    if matches!(
        code,
        "UNSUPPORTED_CONFIG_VERSION"
            | "OFFLINE_OVERRIDE_CONFLICT"
            | "DICTIONARY_SETTING_MISSING"
            | "DICTIONARY_ARCHIVE_LIMIT"
            | "DICTIONARY_REVISION_LIMIT"
    ) {
        2
    } else if code == "DICTIONARY_ALREADY_ENRICHED" {
        5
    } else if code == "PREPARATION_ACTIVE_ITEM_REQUIRES_RECOVERY"
        || code == "JOB_DELETE_RECOVERY_REQUIRED"
        || ((code.starts_with("PREPARATION_") || code.starts_with("APPLY_JOB_"))
            && code.ends_with("_CORRUPT"))
    {
        7
    } else if code == "DOCUMENT_NOT_READY" || code == "IMPORT_ACTIVATION_BLOCKED" {
        4
    } else if code.starts_with("GC_BLOCKED_BY_")
        || matches!(
            code,
            "RESOURCE_DESTINATION_EXISTS"
                | "RESOURCE_VERSION_INSTALLED_DIFFERENTLY"
                | "IMPORT_OUTPUT_EXISTS"
                | "IMPORT_CANDIDATE_CHANGED"
        )
    {
        5
    } else if matches!(
        code,
        "RESOURCE_DESTINATION_OUTSIDE_RESOURCE_DIR"
            | "RESOURCE_VERSION_INVALID"
            | "RESOURCE_DESTINATION_INVALID"
    ) {
        2
    } else if code.contains("CONFLICT")
        || matches!(
            code,
            "STORAGE_RELOCATION_BLOCKED"
                | "PREPARATION_CANCEL_IS_TERMINAL"
                | "APPLY_JOB_CANCEL_IS_TERMINAL"
                | "JOB_TOMBSTONED"
                | "JOB_ALREADY_TOMBSTONED"
                | "JOB_DELETE_WORKER_ACTIVE"
                | "LEASE_HELD_OR_OWNER_UNVERIFIED"
        )
    {
        5
    } else if code.contains("UNAVAILABLE") {
        3
    } else if code.contains("IO")
        || code.contains("FAILED")
        || matches!(
            code,
            "ANKI_ACTION_REJECTED" | "ANKI_READ_TIMEOUT" | "ANKI_HTTP_FAILURE"
        )
    {
        6
    } else {
        2
    }
}

fn load_effective(cli: &Cli) -> Result<linguist_config::Effective, String> {
    use linguist_config::{ConfigFile, Registry};
    let registry = Registry::builtin();
    let mut options = config_options(cli, &registry)?;
    apply_offline_flag(cli, &mut options)?;
    let path = linguist_config::config_path(cli.config.as_deref(), &options.environment)?;
    let file = match std::fs::symlink_metadata(&path) {
        Ok(_) => ConfigFile::read(&path, &registry)?,
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                && cli.config.is_none()
                && !options.environment.contains_key("LAB_CONFIG") =>
        {
            ConfigFile::default()
        }
        Err(_) => {
            return Err(
                "CONFIG_IO: selected configuration does not exist or cannot be inspected".into(),
            );
        }
    };
    let effective = linguist_config::resolve(&registry, &file, &options)?;
    diagnostics::configure(&effective);
    Ok(effective)
}

fn package_limits(
    settings: &linguist_config::Effective,
) -> Result<linguist_application::checkpoint::PackageLimits, String> {
    let registry = linguist_config::Registry::builtin();
    for key in [
        "backup.verify_timeout_seconds",
        "backup.max_package_gb",
        "backup.max_collection_gb",
        "backup.max_media_gb",
        "backup.max_media_map_mb",
        "backup.max_entries",
        "backup.verify_scratch_dir",
    ] {
        registry.validate_value(key, &settings.values[key])?;
    }
    let value = |key: &str| settings.values[key].as_u64().unwrap();
    let gib = 1024 * 1024 * 1024;
    Ok(linguist_application::checkpoint::PackageLimits {
        max_package_bytes: value("backup.max_package_gb") * gib,
        max_collection_bytes: value("backup.max_collection_gb") * gib,
        max_media_bytes: value("backup.max_media_gb") * gib,
        max_media_map_bytes: value("backup.max_media_map_mb") * 1024 * 1024,
        max_entries: value("backup.max_entries") as usize,
        timeout: std::time::Duration::from_secs(value("backup.verify_timeout_seconds")),
        scratch_dir: linguist_config::expand_path(
            settings.values["backup.verify_scratch_dir"]
                .as_str()
                .unwrap(),
            &std::env::vars().collect(),
        )?,
    })
}

fn read_scope_manifest(
    path: Option<&std::path::Path>,
    max_bytes: u64,
) -> Result<linguist_application::checkpoint::ScopeManifest, String> {
    use linguist_application::checkpoint::{CoverageRequirement, ScopeManifest};
    let manifest = match path {
        None => ScopeManifest {
            schema_version: 1,
            requirement: CoverageRequirement::default(),
            note_ids: vec![],
            cards: vec![],
            model_ids: vec![],
            media: vec![],
        },
        Some(path) => {
            let bytes = read_bounded(path, max_bytes)?;
            serde_json::from_slice(&bytes).map_err(|_| "CHECKPOINT_SCOPE_INVALID")?
        }
    };
    manifest.validate()?;
    Ok(manifest)
}

fn read_bounded(path: &std::path::Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| "INPUT_FILE_UNAVAILABLE")?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "INPUT_FILE_UNAVAILABLE")?;
    if bytes.len() as u64 > max_bytes {
        return Err("INPUT_FILE_TOO_LARGE".into());
    }
    Ok(bytes)
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month = month as i64;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Accepts `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SSZ` (UTC only).
fn parse_since_ms(value: &str) -> Result<u64, String> {
    let invalid = || "INVALID_SINCE: use YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ".to_owned();
    let (date, time) = match value.split_once('T') {
        Some((date, time)) => (date, Some(time.strip_suffix('Z').ok_or_else(invalid)?)),
        None => (value, None),
    };
    let parts: Vec<&str> = date.split('-').collect();
    let number = |text: &str, len: usize| -> Result<u32, String> {
        if text.len() != len || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        text.parse().map_err(|_| invalid())
    };
    let [year, month, day] = parts.as_slice() else {
        return Err(invalid());
    };
    let (year, month, day) = (number(year, 4)?, number(month, 2)?, number(day, 2)?);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month[month as usize - 1]
    {
        return Err(invalid());
    }
    let mut seconds = days_from_civil(year as i64, month, day) * 86_400;
    if let Some(time) = time {
        let parts: Vec<&str> = time.split(':').collect();
        let [h, m, sec] = parts.as_slice() else {
            return Err(invalid());
        };
        let (h, m, sec) = (number(h, 2)?, number(m, 2)?, number(sec, 2)?);
        if h > 23 || m > 59 || sec > 59 {
            return Err(invalid());
        }
        seconds += (h * 3600 + m * 60 + sec) as i64;
    }
    Ok(seconds as u64 * 1000)
}

/// OP-35 for simulate/apply jobs: freeze an approved revision; never runs it.
#[allow(clippy::too_many_arguments)]
fn run_create_apply_job(
    settings: &linguist_config::Effective,
    env: &BTreeMap<String, String>,
    mode: JobModeArg,
    plan: uuid::Uuid,
    revision: Option<u32>,
    digest: String,
    approval: Option<uuid::Uuid>,
    checkpoint: Option<uuid::Uuid>,
    protected_manifest: Option<String>,
    item_ids: Vec<uuid::Uuid>,
    accept_schema_change: bool,
) -> Result<u8, String> {
    use linguist_application::job_executor;
    let frozen = linguist_application::freeze_settings(settings, env)?;
    let root = state_root(settings)?;
    // Unknown plans must never initialize state.
    let reader = linguist_store::Store::read_only(&root)?;
    let revision = match revision {
        Some(revision) => revision,
        None => reader.latest_revision(plan)?,
    };
    let approval = match approval {
        Some(approval) => approval,
        None => {
            // Approval IDs carry no order: pick one only when it is unambiguous.
            let matching: Vec<_> = reader
                .approvals_for(plan, revision)?
                .into_iter()
                .filter(|a| a.approval.digest == digest)
                .collect();
            match matching.as_slice() {
                [only] => only.id,
                [] => return Err("APPLY_APPROVAL_MISSING: approve the exact revision first".into()),
                _ => return Err("JOB_APPROVAL_AMBIGUOUS: several approvals cover this revision; pass --approval".into()),
            }
        }
    };
    drop(reader);
    if mode == JobModeArg::Apply && protected_manifest.is_none() {
        return Err("JOB_PROTECTED_MANIFEST_REQUIRED: apply jobs name the protected-manifest digest their checkpoint was created with".into());
    }
    let protected = protected_manifest.unwrap_or_else(|| "unchecked".into());
    let mut store = linguist_store::Store::open_existing(&root)?;
    let outcome = job_executor::create(
        &mut store,
        &frozen,
        &job_executor::CreateRequest {
            mode: mode.mode(),
            plan_id: plan,
            revision,
            digest: &digest,
            approval_id: approval,
            item_ids,
            checkpoint_id: checkpoint,
            protected_manifest_digest: &protected,
            accept_schema_change,
            now_ms: now_ms()?,
        },
    )?;
    emit(&serde_json::json!({
        "schema_version": 2,
        "job": outcome,
        "execution_available": false,
        "writes_enabled": false,
    }))?;
    Ok(0)
}

/// Jobs subcommands that differ by job kind. Returns `None` to let the
/// preparation handlers run.
fn run_job_command(root: &std::path::Path, command: &JobCommand) -> Result<Option<u8>, String> {
    use linguist_application::job_executor;
    use linguist_store::apply_job::JobControl;
    let job = match command {
        JobCommand::Retry { job, .. }
        | JobCommand::Delete { job, .. }
        | JobCommand::Run { job, .. }
        | JobCommand::Resume { job, .. }
        | JobCommand::Pause { job }
        | JobCommand::Cancel { job }
        | JobCommand::Show { job }
        | JobCommand::Items { job, .. }
        | JobCommand::Audit { job, .. } => *job,
        _ => return Ok(None),
    };
    // Absent state is handled (without initialization) by the shared handlers.
    if std::fs::symlink_metadata(root).is_err() {
        return Ok(None);
    }
    let reader = linguist_store::Store::read_only(root)?;
    if let JobCommand::Delete { execute, .. } = command {
        let result = if *execute {
            drop(reader);
            let mut store = linguist_store::Store::open_existing(root)?;
            job_executor::delete(&mut store, job, true, now_ms()?)?
        } else {
            let mut reader = reader;
            job_executor::delete(&mut reader, job, false, now_ms()?)?
        };
        emit(&serde_json::json!({"schema_version":2,"delete":result,"writes_enabled":false}))?;
        return Ok(Some(0));
    }
    let definition = match reader.apply_job(job) {
        Ok(definition) => definition,
        Err(code) if code == "APPLY_JOB_NOT_FOUND" => {
            if let JobCommand::Retry {
                item_ids,
                failed,
                apply,
                ..
            } = command
            {
                return prepare_retry(&reader, job, item_ids, *failed, *apply).map(Some);
            }
            return Ok(None);
        }
        Err(code) => return Err(code),
    };
    let apply_mode = definition.job.mode == linguist_core::records::JobMode::Apply;
    let check_flag = |apply: bool| -> Result<(), String> {
        match (apply_mode, apply) {
            (false, true) => Err(
                "JOB_MODE_NEVER_WRITES: simulate jobs perform preflight only; create an apply job"
                    .into(),
            ),
            (true, false) => Err(
                "APPLY_FLAG_REQUIRED: apply jobs need --apply on every run, resume and retry"
                    .into(),
            ),
            _ => Ok(()),
        }
    };
    let unavailable = |what: &str| -> String {
        format!(
            "CAPABILITY_UNAVAILABLE: {what} for simulate/apply jobs requires the verified native lab-native-v1 read/mutation adapter; no lease, job event or Anki request was made"
        )
    };
    match command {
        JobCommand::Run { apply, .. } => {
            check_flag(*apply)?;
            Err(unavailable("jobs run"))
        }
        JobCommand::Resume { apply, .. } => {
            check_flag(*apply)?;
            drop(reader);
            let mut store = linguist_store::Store::open_existing(root)?;
            // The resume request is durable even though execution is unavailable.
            job_executor::request_control(&mut store, job, JobControl::Resume)?;
            Err(unavailable("jobs resume"))
        }
        JobCommand::Pause { .. } | JobCommand::Cancel { .. } => {
            let action = if matches!(command, JobCommand::Pause { .. }) {
                JobControl::Pause
            } else {
                JobControl::Cancel
            };
            drop(reader);
            let mut store = linguist_store::Store::open_existing(root)?;
            let outcome = job_executor::request_control(&mut store, job, action)?;
            emit(
                &serde_json::json!({"schema_version":2,"job_id":job,"control":outcome,"worker_stopped_confirmed":outcome.worker_stopped_confirmed,"writes_enabled":false}),
            )?;
            Ok(Some(0))
        }
        JobCommand::Retry {
            item_ids,
            failed,
            apply,
            ..
        } => {
            check_flag(*apply)?;
            drop(reader);
            let mut store = linguist_store::Store::open_existing(root)?;
            let outcome = job_executor::request_retry(&mut store, job, item_ids, *failed)?;
            let exit = if outcome.accepted.is_empty() { 4 } else { 0 };
            let flag = if apply_mode { " --apply" } else { "" };
            emit(
                &serde_json::json!({"schema_version":2,"retry":outcome,"execution_available":false,"next_command":format!("lab jobs run {job}{flag}"),"writes_enabled":false}),
            )?;
            Ok(Some(exit))
        }
        JobCommand::Show { .. } => {
            let view = job_executor::show(&reader, job)?;
            emit(
                &serde_json::json!({"schema_version":2,"mode":definition.job.mode,"job":view,"execution_available":false,"writes_enabled":false}),
            )?;
            Ok(Some(0))
        }
        JobCommand::Items {
            after_index, limit, ..
        } => {
            let limit = limit.unwrap_or(
                definition
                    .job
                    .settings
                    .values
                    .get("output.page_size")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(100) as u32,
            );
            let items = job_executor::items(&reader, job, *after_index, limit)?;
            let next = items.last().map(|i| i.index + 1);
            emit(
                &serde_json::json!({"schema_version":2,"job_id":job,"items":items,"next_index":next,"execution_available":false}),
            )?;
            Ok(Some(0))
        }
        JobCommand::Audit { live, .. } => {
            if *live {
                return Err("CAPABILITY_UNAVAILABLE: live job audit requires native collection verification".into());
            }
            let report = job_executor::audit(&reader, job)?;
            let exit = if report.issues.is_empty() { 0 } else { 7 };
            emit(&serde_json::json!({"schema_version":2,"audit":report}))?;
            Ok(Some(exit))
        }
        _ => Ok(None),
    }
}

/// OP-42 for prepare jobs: classification only. A later `jobs run` retries
/// every eligible failure within the frozen attempt budget.
fn prepare_retry(
    store: &linguist_store::Store,
    job: uuid::Uuid,
    item_ids: &[uuid::Uuid],
    failed: bool,
    apply: bool,
) -> Result<u8, String> {
    if apply {
        return Err("JOB_MODE_NEVER_WRITES: prepare jobs capture sources only".into());
    }
    let definition = store.preparation_job(job)?;
    let mut eligible = Vec::new();
    let mut refused = Vec::new();
    for offset in (0..definition.job.item_ids.len()).step_by(1000) {
        for item in store.preparation_items(job, offset as u32, 1000)? {
            let selected = if failed {
                item.state == "failed"
            } else {
                item_ids.contains(&item.item_id)
            };
            if !selected {
                continue;
            }
            if item.retry_eligible {
                eligible.push(item.item_id);
            } else {
                let code = match item.state.as_str() {
                    "started" => "JOB_RETRY_RECOVER_FIRST",
                    "captured" => "JOB_RETRY_ALREADY_SUCCEEDED",
                    "pending" => "JOB_RETRY_NOT_FAILED",
                    _ if item.error_code.as_deref() == Some("SOURCE_CAPTURE_REVIEW_REQUIRED") => {
                        "JOB_RETRY_REQUIRES_REVIEW"
                    }
                    _ => "JOB_RETRY_ATTEMPTS_EXHAUSTED",
                };
                refused.push(
                    serde_json::json!({"item_id":item.item_id,"state":item.state,"code":code}),
                );
            }
        }
    }
    if let Some(missing) = item_ids
        .iter()
        .find(|id| !definition.job.item_ids.contains(id))
    {
        return Err(format!("PREPARATION_ITEM_NOT_FOUND: {missing}"));
    }
    let exit = if eligible.is_empty() { 4 } else { 0 };
    emit(
        &serde_json::json!({"schema_version":2,"job_id":job,"mode":"prepare","eligible":eligible,"refused":refused,"envelope_recorded":false,"next_command":format!("lab jobs run {job}"),"note":"jobs run retries every eligible failure within the frozen attempt budget","writes_enabled":false}),
    )?;
    Ok(exit)
}

fn now_ms() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .map_err(|_| "CLOCK_INVALID".into())
}

fn state_root(settings: &linguist_config::Effective) -> Result<PathBuf, String> {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let root =
        linguist_config::expand_path(settings.values["storage.state_dir"].as_str().unwrap(), &env)?;
    if !root.is_absolute() {
        return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
    }
    Ok(root)
}

/// OP-34 preview. Local preflight only: no lease, journal, checkpoint or Anki request.
#[allow(clippy::too_many_arguments)]
fn run_apply(
    settings: &linguist_config::Effective,
    plan: uuid::Uuid,
    revision: Option<u32>,
    digest: Option<String>,
    item_ids: &[uuid::Uuid],
    split_group: Option<uuid::Uuid>,
    accept_schema_change: bool,
    apply: bool,
) -> Result<u8, String> {
    let store = linguist_store::Store::read_only(&state_root(settings)?)?;
    let latest = store.latest_revision(plan)?;
    let revision = revision.unwrap_or(latest);
    let stored = store.revision(plan, revision)?;
    let actual = stored.approval_digest().map_err(|e| e.to_string())?;
    if let Some(digest) = &digest
        && digest != &actual
    {
        return Err("APPLY_DIGEST_MISMATCH".into());
    }
    if apply {
        // ALG-APPLY is implemented over the native port, but no tested
        // lab-native-v1 mutation transport exists. Never fall back to plain
        // AnkiConnect writes.
        return Err("CAPABILITY_UNAVAILABLE: apply --apply requires the verified native lab-native-v1 mutation adapter; no lease, checkpoint, journal or Anki request was made".into());
    }
    let (item_ids, split) = match split_group {
        Some(group) => {
            let units = stored
                .grammar_groups
                .iter()
                .find(|g| g.id == group)
                .ok_or("SPLIT_GROUP_NOT_FOUND")?
                .units
                .clone();
            let execution = store
                .split_execution_for(plan, revision, group)?
                .map(|record| linguist_application::split::status(&store, record.execution_id))
                .transpose()?;
            (
                units,
                Some(serde_json::json!({"group_id": group, "execution": execution})),
            )
        }
        None => (item_ids.to_vec(), None),
    };
    let items = linguist_application::apply::preview(&store, plan, revision, &item_ids)?;
    let blocked = items
        .iter()
        .filter(|item| !item.blockers.is_empty())
        .count();
    emit(&serde_json::json!({
        "schema_version": 2,
        "mode": "preview",
        "plan_id": plan,
        "revision": revision,
        "latest_revision": latest,
        "digest": actual,
        "accept_schema_change": accept_schema_change,
        "split_group": split,
        "items": items,
        "blocked_items": blocked,
        "apply_requirements": [
            "current invocation --apply",
            "approval covering each selected item",
            "verified lab-native-v1 bridge on loopback with a live collection session",
            "verified checkpoint covering the item scope",
            "exact installed target model and existing target deck",
            "fresh source match; normal study is captured at apply time",
        ],
        "collection_writes_enabled": false,
    }))?;
    Ok(if blocked == 0 { 0 } else { 4 })
}

/// OP-60 local proposal. Live evidence and recovery writes need the native adapter.
fn run_reconcile(
    settings: &linguist_config::Effective,
    operation: uuid::Uuid,
    apply: bool,
    rebind: bool,
) -> Result<u8, String> {
    if rebind && !apply {
        return Err("APPLY_FLAG_REQUIRED: --rebind needs the current invocation's --apply".into());
    }
    let store = linguist_store::Store::read_only(&state_root(settings)?)?;
    let journal = store.journal(operation)?;
    let proposal = match linguist_application::apply::local_proposal(&store, operation) {
        Ok(proposal) => Some(proposal),
        Err(code) if code == "APPLY_OPERATION_NOT_FOUND" => None,
        Err(code) => return Err(code),
    };
    // A restore journal resumes through `snapshots restore`, never a new operation.
    let restore = match store.restore_operation(operation) {
        Ok(record) => Some(record),
        Err(code) if code == "RESTORE_OPERATION_NOT_FOUND" => None,
        Err(code) => return Err(code),
    };
    if apply {
        return Err(format!(
            "CAPABILITY_UNAVAILABLE: recover reconcile --apply requires the verified native lab-native-v1 adapter; operation {operation} was not changed and no Anki request was made"
        ));
    }
    let unresolved = journal.pending_recovery;
    emit(&serde_json::json!({
        "schema_version": 2,
        "mode": "proposal",
        "operation_id": operation,
        "journal": journal,
        "apply_operation": proposal,
        "restore_operation": restore.as_ref().map(|r| serde_json::json!({
            "target_operation": r.target_operation,
            "target_snapshot": r.target_snapshot,
            "resume_command": linguist_application::restore::restore_command(r.target_snapshot),
        })),
        "reconciliation_kind": if proposal.is_some() { "apply" } else if restore.is_some() { "restore" } else { "unsupported_journal_kind" },
        "live_checked": false,
        "reconciliation_available": false,
    }))?;
    Ok(if unresolved { 4 } else { 0 })
}

/// OP-44 local preview: what restoring every apply operation of one group
/// (an apply job or a grammar split execution) would reverse.
fn run_rollback(
    settings: &linguist_config::Effective,
    group: uuid::Uuid,
    item_ids: &[uuid::Uuid],
    apply: bool,
) -> Result<u8, String> {
    let store = linguist_store::Store::read_only(&state_root(settings)?)?;
    let mut items = Vec::new();
    for version in store.group_journals(group, 10000)? {
        let Ok(record) = store.apply_operation(version.journal.id) else {
            continue;
        };
        if !item_ids.is_empty() && !item_ids.contains(&record.item_id) {
            continue;
        }
        let preview =
            linguist_application::restore::local_preview(&store, version.journal.snapshot_id);
        items.push(serde_json::json!({
            "item_id": record.item_id,
            "operation_id": version.journal.id,
            "snapshot_id": version.journal.snapshot_id,
            "state": version.journal.state,
            "restore": preview.as_ref().ok(),
            "error": preview.err(),
        }));
    }
    if items.is_empty() {
        return Err("ROLLBACK_GROUP_EMPTY: no apply operation carries this group".into());
    }
    if apply {
        return Err("CAPABILITY_UNAVAILABLE: jobs rollback --apply requires the verified native lab-native-v1 mutation adapter; no lease, checkpoint, journal or Anki request was made".into());
    }
    let split = match store.split_execution(group) {
        Ok(_) => Some(linguist_application::split::status(&store, group)?),
        Err(code) if code == "SPLIT_EXECUTION_NOT_FOUND" => None,
        Err(code) => return Err(code),
    };
    let blocked = items.iter().any(|item| {
        !item["error"].is_null()
            || item["restore"]["blockers"]
                .as_array()
                .is_some_and(|b| !b.is_empty())
    });
    emit(&serde_json::json!({
        "schema_version": 2,
        "mode": "preview",
        "group_id": group,
        "live_checked": false,
        "split_execution": split,
        "items": items,
        "order": "existing-note restores (the split anchor) first, then created notes, which are kept unless explicitly listed while unchanged and unstudied",
        "collection_writes_enabled": false,
    }))?;
    Ok(if blocked { 4 } else { 0 })
}

fn run_backup(command: BackupCommand, settings: &linguist_config::Effective) -> Result<u8, String> {
    use linguist_application::{backup, checkpoint};
    let max_input = settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024;
    let state_root = || -> Result<PathBuf, String> {
        let root = linguist_config::expand_path(
            settings.values["storage.state_dir"].as_str().unwrap(),
            &std::env::vars().collect(),
        )?;
        if !root.is_absolute() {
            return Err("STORE_PATH_MUST_BE_ABSOLUTE".into());
        }
        Ok(root)
    };
    match command {
        BackupCommand::Inspect { file } => {
            let report = checkpoint::inspect_colpkg(&file, package_limits(settings)?)?;
            emit(&serde_json::json!({"schema_version":2,"inspection":report}))?;
            Ok(0)
        }
        BackupCommand::Create {
            scope,
            output,
            scope_manifest,
            apply,
        } => {
            let preference: backup::ScopePreference = scope.parse()?;
            let manifest = read_scope_manifest(scope_manifest.as_deref(), max_input)?;
            let preference_setting = settings.values["backup.scope"].as_str().unwrap();
            let coverage = backup::plan_coverage(preference, &manifest);
            let (output, output_source) = match output {
                Some(output) => (output, "flag"),
                None => {
                    let directory = linguist_config::expand_path(
                        settings.values["storage.backup_dir"].as_str().unwrap(),
                        &std::env::vars().collect(),
                    )?;
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let suffix = uuid::Uuid::new_v4().simple().to_string();
                    (
                        directory.join(format!("checkpoint-{stamp}-{}.colpkg", &suffix[..8])),
                        "storage.backup_dir",
                    )
                }
            };
            if output_source == "storage.backup_dir"
                && !output.parent().is_some_and(std::path::Path::is_dir)
            {
                return Err(format!(
                    "CHECKPOINT_OUTPUT_PARENT_UNAVAILABLE: storage.backup_dir {} does not exist; create it privately (mkdir -m 700) or pass --output",
                    output.parent().unwrap().display()
                ));
            }
            let (output, _) = backup::checkpoint_paths(&output, uuid::Uuid::nil())?;
            if apply {
                // No tested native export_checkpoint adapter exists; never call an
                // unverified endpoint. Nothing was journaled or sent.
                return Err("CAPABILITY_UNAVAILABLE: backup create --apply requires the verified native export_checkpoint adapter; no lease, journal or Anki request was made".into());
            }
            emit(&serde_json::json!({
                "schema_version": 2,
                "mode": "preview",
                "output": output,
                "output_source": output_source,
                "scope_digest": manifest.digest()?,
                "scope_entries": {
                    "notes": manifest.note_ids.len(),
                    "cards": manifest.cards.len(),
                    "models": manifest.model_ids.len(),
                    "media": manifest.media.len(),
                },
                "configured_scope_preference": preference_setting,
                "coverage": coverage,
                "steps": [
                    "acquire collection writer lease and verify native binding",
                    "journal export intent, then request native export_checkpoint to a create-new temporary path",
                    "verify file size and SHA-256 against the claim, package structure, SQLite integrity and every scope entry",
                    "run a disposable decode restoration test in backup.verify_scratch_dir",
                    "link the artifact create-new, then save an immutable receipt"
                ],
                "native_export_available": false,
                "apply_eligible": false,
                "writes_enabled": false,
            }))?;
            Ok(0)
        }
        BackupCommand::List {
            scope,
            since,
            limit,
        } => {
            if let Some(scope) = &scope
                && !matches!(scope.as_str(), "affected" | "collection")
            {
                return Err("INVALID_BACKUP_SCOPE".into());
            }
            let since = since.as_deref().map(parse_since_ms).transpose()?;
            let limit =
                u32::try_from(page_limit(limit, settings)?).map_err(|_| "INVALID_PAGE_LIMIT")?;
            let root = state_root()?;
            let records = if std::fs::symlink_metadata(&root).is_err() {
                vec![]
            } else {
                let store = linguist_store::Store::read_only(&root)?;
                store
                    .list_checkpoints(scope.as_deref(), since, limit)?
                    .iter()
                    .map(|record| backup::summarize(&store, record))
                    .collect::<Result<Vec<_>, _>>()?
            };
            emit(&serde_json::json!({
                "schema_version": 2,
                "checkpoints": records,
                "limit": limit,
                "eligibility_note": "eligible means the stored receipt has a passing restoration test for its scope; dependent writes re-hash the artifact first",
            }))?;
            Ok(0)
        }
        BackupCommand::Verify {
            backup: target,
            restore_test_target,
            scope_manifest,
        } => {
            let limits = package_limits(settings)?;
            if let Ok(id) = uuid::Uuid::parse_str(&target) {
                if scope_manifest.is_some() {
                    return Err("CHECKPOINT_SCOPE_MANIFEST_NOT_ALLOWED: a registered receipt carries its own scope".into());
                }
                let root = state_root()?;
                std::fs::symlink_metadata(&root).map_err(|_| "STORE_NOT_FOUND")?;
                let report = if restore_test_target.is_some() {
                    let mut store = linguist_store::Store::open_existing(&root)?;
                    backup::verify_registered(
                        &mut store,
                        id,
                        limits,
                        restore_test_target.as_deref(),
                        now_ms()?,
                    )?
                } else {
                    let mut store = linguist_store::Store::read_only(&root)?;
                    backup::verify_registered(&mut store, id, limits, None, 0)?
                };
                emit(
                    &serde_json::json!({"schema_version":2,"registered":true,"verification":report,"checkpoint_eligible":true}),
                )?;
                return Ok(0);
            }
            let file = PathBuf::from(&target);
            let manifest = read_scope_manifest(scope_manifest.as_deref(), max_input)?;
            let (inspection, scope_report) =
                checkpoint::inspect_colpkg_scope(&file, limits.clone(), &manifest)?;
            let restoration = restore_test_target
                .as_deref()
                .map(|dir| checkpoint::restore_test(&file, limits, &manifest, dir))
                .transpose()?;
            emit(&serde_json::json!({
                "schema_version": 2,
                "registered": false,
                "scope_digest": manifest.digest()?,
                "inspection": inspection,
                "scope_report": scope_report,
                "restoration": restoration,
                "verified": true,
                "checkpoint_eligible": false,
                "eligibility_note": "an unregistered file has no collection binding or receipt and cannot authorize a write",
            }))?;
            Ok(0)
        }
    }
}

fn anki_client(settings: &linguist_config::Effective) -> Result<linguist_anki::Client, String> {
    linguist_anki::Client::from_settings(settings, &std::env::vars().collect())
}
fn deck_purpose_mappings(
    config: Option<&std::path::Path>,
    deck: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let environment: BTreeMap<String, String> = std::env::vars().collect();
    let path = linguist_config::config_path(config, &environment)?;
    let file = match std::fs::symlink_metadata(&path) {
        Ok(_) => linguist_config::ConfigFile::read(&path, &linguist_config::Registry::builtin())?,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && config.is_none()
                && !environment.contains_key("LAB_CONFIG") =>
        {
            linguist_config::ConfigFile::default()
        }
        Err(_) => {
            return Err(
                "CONFIG_IO: selected configuration does not exist or cannot be inspected".into(),
            );
        }
    };
    let mut mappings = Vec::new();
    for purpose in linguist_config::builtin_purposes() {
        for (role, key) in [("source", "source_deck"), ("target", "target_deck")] {
            if file
                .values
                .get(&format!("purposes.{purpose}.{key}"))
                .and_then(serde_json::Value::as_str)
                == Some(deck)
            {
                mappings.push(serde_json::json!({"purpose":purpose,"role":role,"source_model":file.values.get(&format!("purposes.{purpose}.source_model")),"verified_live":false}));
            }
        }
    }
    Ok(mappings)
}
fn mapping_edit(
    cli: &Cli,
    purpose: &str,
    change: linguist_config::edit::Change,
) -> Result<linguist_config::edit::EditReceipt, String> {
    if !linguist_config::builtin_purposes()
        .iter()
        .any(|p| p == purpose)
    {
        return Err("SOURCE_MAPPING_PURPOSE_UNSUPPORTED".into());
    }
    if cli.profile.is_some()
        || cli
            .purpose
            .as_deref()
            .is_some_and(|selected| selected != purpose)
        || !cli.settings.is_empty()
    {
        return Err("DECK_MAPPING_SCOPE_CONFLICT: use durable purpose mapping without profile, purpose or --set overrides".into());
    }
    let environment: BTreeMap<String, String> = std::env::vars().collect();
    let path = linguist_config::config_path(cli.config.as_deref(), &environment)?;
    linguist_config::edit::edit(
        &path,
        &linguist_config::edit::Scope::default(),
        &change,
        true,
        &environment,
    )
}
fn map_deck(
    cli: &Cli,
    command: &DeckCommand,
    max_bytes: u64,
    max_chars: usize,
) -> Result<u8, String> {
    let DeckCommand::Map {
        purpose,
        source_deck,
        target_deck,
        source_model,
        fields: field_file,
        task_map: task_file,
        ocr_languages,
    } = command
    else {
        unreachable!()
    };
    if !linguist_config::builtin_purposes()
        .iter()
        .any(|p| p == purpose)
    {
        return Err("SOURCE_MAPPING_PURPOSE_UNSUPPORTED".into());
    }
    if cli.profile.is_some()
        || cli
            .purpose
            .as_deref()
            .is_some_and(|selected| selected != purpose)
        || !cli.settings.is_empty()
    {
        return Err("DECK_MAPPING_SCOPE_CONFLICT: use durable purpose mapping without profile, purpose or --set overrides".into());
    }
    let registry = linguist_config::Registry::builtin();
    let prefix = format!("purposes.{purpose}.");
    let fields: BTreeMap<String, String> =
        serde_json::from_slice(&read_input(field_file, max_bytes, max_chars)?)
            .map_err(|_| "SOURCE_MAPPING_FIELDS_INVALID")?;
    let tasks: BTreeMap<String, String> = task_file
        .as_ref()
        .map(|path| {
            serde_json::from_slice(&read_input(path, max_bytes, max_chars)?)
                .map_err(|_| "SOURCE_MAPPING_TASKS_INVALID".to_owned())
        })
        .transpose()?
        .unwrap_or_default();
    registry.validate_value(&(prefix.clone() + "fields"), &serde_json::json!(fields))?;
    registry.validate_value(&(prefix.clone() + "card_tasks"), &serde_json::json!(tasks))?;
    registry.validate_value(
        &(prefix.clone() + "ocr_languages"),
        &serde_json::json!(ocr_languages),
    )?;
    let settings = load_effective(cli)?;
    let client = anki_client(&settings)?;
    let decks = client.decks()?;
    let source = linguist_anki::select_name(decks.clone(), source_deck)?;
    let target = target_deck
        .as_ref()
        .map(|name| linguist_anki::select_name(decks.clone(), name))
        .transpose()?;
    if client.deck_is_filtered(&source.name)? {
        return Err("SOURCE_MAPPING_FILTERED_DECK_UNSUPPORTED".into());
    }
    if let Some(target) = &target
        && client.deck_is_filtered(&target.name)?
    {
        return Err("SOURCE_MAPPING_FILTERED_DECK_UNSUPPORTED".into());
    }
    let inspected = client.inspect_model(source_model)?;
    if inspected.model.name != *source_model && inspected.model.id != *source_model {
        return Err("SOURCE_MAPPING_MODEL_CONFLICT".into());
    }
    let observed_fields = inspected
        .fields
        .iter()
        .map(|name| (name.clone(), name.clone()))
        .collect::<BTreeMap<_, _>>();
    let kind = if purpose.ends_with("vocab") {
        linguist_application::mapping::SourceKind::Vocabulary
    } else {
        linguist_application::mapping::SourceKind::Grammar
    };
    let validation = linguist_application::mapping::map_fields(
        kind,
        &observed_fields,
        &fields,
        max_bytes.min(100 * 1024 * 1024),
        max_chars,
    )?;
    if !validation.missing_required_roles.is_empty() {
        return Err(format!(
            "SOURCE_MAPPING_REQUIRED_ROLES_MISSING: {}",
            validation.missing_required_roles.join(",")
        ));
    }
    for ordinal in tasks.keys() {
        if ordinal
            .parse::<usize>()
            .map_err(|_| "SOURCE_MAPPING_TASK_ORDINAL_INVALID")?
            >= inspected.templates.len()
        {
            return Err("SOURCE_MAPPING_TASK_ORDINAL_INVALID".into());
        }
    }
    client.check_profile()?;
    let mut sets = BTreeMap::from([
        (
            prefix.clone() + "source_deck",
            serde_json::json!(source.name),
        ),
        (
            prefix.clone() + "source_model",
            serde_json::json!(inspected.model.name),
        ),
        (prefix.clone() + "fields", serde_json::json!(fields)),
        (prefix.clone() + "card_tasks", serde_json::json!(tasks)),
        (
            prefix.clone() + "ocr_languages",
            serde_json::json!(ocr_languages),
        ),
    ]);
    let mut unsets = Vec::new();
    if let Some(target) = &target {
        sets.insert(
            prefix.clone() + "target_deck",
            serde_json::json!(target.name),
        );
    } else {
        unsets.push(prefix.clone() + "target_deck");
    }
    let receipt = mapping_edit(
        cli,
        purpose,
        linguist_config::edit::Change::Batch { sets, unsets },
    )?;
    emit(
        &serde_json::json!({"version":2,"purpose":purpose,"mapping_digest":validation.mapping_digest,"source_deck":source,"target_deck":target,"source_model":inspected.model,"unmapped_fields":validation.unmapped_fields,"template_order_verified":inspected.template_order_verified,"filtered_deck_verified":true,"writes_enabled":false,"config":receipt}),
    )?;
    Ok(0)
}
fn unmap_deck(cli: &Cli, purpose: &str) -> Result<u8, String> {
    if !linguist_config::builtin_purposes()
        .iter()
        .any(|name| name == purpose)
    {
        return Err("SOURCE_MAPPING_PURPOSE_UNSUPPORTED".into());
    }
    if cli.profile.is_some()
        || cli
            .purpose
            .as_deref()
            .is_some_and(|selected| selected != purpose)
        || !cli.settings.is_empty()
    {
        return Err("DECK_MAPPING_SCOPE_CONFLICT: use durable purpose mapping without profile, purpose or --set overrides".into());
    }
    let environment: BTreeMap<String, String> = std::env::vars().collect();
    let path = linguist_config::config_path(cli.config.as_deref(), &environment)?;
    if matches!(std::fs::symlink_metadata(&path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        && cli.config.is_none()
        && !environment.contains_key("LAB_CONFIG")
    {
        emit(
            &serde_json::json!({"version":2,"purpose":purpose,"config":{"changed":false,"executed":false},"collection_changed":false}),
        )?;
        return Ok(0);
    }
    let prefix = format!("purposes.{purpose}.");
    let receipt = mapping_edit(
        cli,
        purpose,
        linguist_config::edit::Change::Batch {
            sets: BTreeMap::new(),
            unsets: [
                "source_deck",
                "target_deck",
                "source_model",
                "fields",
                "card_tasks",
                "ocr_languages",
            ]
            .iter()
            .map(|key| format!("{prefix}{key}"))
            .collect(),
        },
    )?;
    emit(
        &serde_json::json!({"version":2,"purpose":purpose,"config":receipt,"collection_changed":false}),
    )?;
    Ok(0)
}
fn page_limit(
    limit: Option<usize>,
    settings: &linguist_config::Effective,
) -> Result<usize, String> {
    let n = limit.unwrap_or(settings.values["output.page_size"].as_u64().unwrap() as usize);
    if !(1..=10000).contains(&n) {
        return Err("INVALID_PAGE_LIMIT".into());
    }
    Ok(n)
}
fn note_query(
    selector: &NoteSelector,
    purpose: Option<&str>,
    settings: &linguist_config::Effective,
) -> Result<String, String> {
    let families = usize::from(!selector.note_ids.is_empty())
        + usize::from(selector.query.is_some())
        + usize::from(selector.deck.is_some())
        + usize::from(purpose.is_some());
    if families != 1 {
        return Err("NOTE_SELECTOR_REQUIRED: choose only note IDs, query, deck or purpose".into());
    }
    let query = if !selector.note_ids.is_empty() {
        if selector.note_ids.len() > 10000 {
            return Err("NOTE_SELECTOR_TOO_LARGE".into());
        }
        let mut parts = Vec::new();
        for id in &selector.note_ids {
            linguist_anki::wire_id(&serde_json::json!(id))?;
            parts.push(format!("nid:{id}"));
        }
        format!("({})", parts.join(" OR "))
    } else if let Some(query) = &selector.query {
        query.clone()
    } else {
        let deck = selector.deck.as_deref().map(Ok).unwrap_or_else(|| {
            settings
                .values
                .get(&format!("purposes.{}.source_deck", purpose.unwrap()))
                .and_then(serde_json::Value::as_str)
                .ok_or("SOURCE_DECK_UNCONFIGURED")
        })?;
        linguist_anki::deck_query(deck)?
    };
    if query.chars().count() > settings.values["input.max_record_chars"].as_u64().unwrap() as usize
    {
        return Err("NOTE_SELECTOR_TOO_LARGE".into());
    }
    Ok(query)
}

/// Probe selected local helper engines (version, language packs, voice files).
/// Only local executables run; no service or collection is contacted.
fn local_engines(
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
) -> Vec<serde_json::Value> {
    let mut checks = Vec::new();
    if settings.values["ocr.engine"] == "tesseract" {
        let required = settings.values["images.existing_policy"] == "inspect";
        checks.push(
            match linguist_application::ocr::Engine::probe(settings, environment) {
                Ok(engine) => {
                    serde_json::json!({"engine":"ocr","status":"available","required":required,"identity":engine.identity()})
                }
                Err(error) => {
                    serde_json::json!({"engine":"ocr","status":error.code(),"required":required,"error":error,"guidance":error.guidance()})
                }
            },
        );
    } else {
        checks.push(serde_json::json!({"engine":"ocr","status":"OCR_ENGINE_UNAVAILABLE","required":settings.values["images.existing_policy"] == "inspect","selected":settings.values["ocr.engine"]}));
    }
    if settings.values["audio.provider"] == "piper" {
        let status = match linguist_application::speech::preflight(settings, environment, None) {
            Ok(()) => "available".to_owned(),
            Err(error) => error.code(),
        };
        checks.push(serde_json::json!({"engine":"speech","provider":"piper","status":status,"required":true,"synthesis_probed":false}));
    }
    // The controlled browser helper is not part of this build; selecting it is a gap.
    if settings.values["browser.enabled"] == true
        || settings.values["dictionary.browser_fallback"] == true
    {
        checks.push(serde_json::json!({"engine":"browser","status":"BROWSER_HELPER_UNAVAILABLE","required":true}));
    }
    checks
}

/// Run requested generation after a published preparation; failures are
/// reported with the retained plan, never discarding it.
fn generate_after_preparation(
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
    plan_id: uuid::Uuid,
) -> serde_json::Value {
    let client =
        match linguist_application::ollama::transport::Client::from_settings(settings, environment)
        {
            Ok(client) => client,
            Err(error) => return serde_json::json!({"error":error,"plan_id":plan_id}),
        };
    match linguist_application::generation::generate_pending(
        plan_id,
        settings,
        environment,
        &client,
    ) {
        Ok(items) => serde_json::json!({"items":items}),
        Err(error) => serde_json::json!({"error":error,"plan_id":plan_id}),
    }
}

/// Emit a preparation receipt, adding inline generation when `llm.enabled`.
fn emit_prepared<T: serde::Serialize>(
    settings: &linguist_config::Effective,
    environment: &BTreeMap<String, String>,
    result: &T,
    plan_id: uuid::Uuid,
    ready: bool,
) -> Result<u8, String> {
    if settings.values["llm.enabled"] == true {
        let generation = generate_after_preparation(settings, environment, plan_id);
        emit(&serde_json::json!({"preparation":result,"generation":generation}))?;
        // Generated content always needs review before readiness.
        return Ok(4);
    }
    emit(result)?;
    Ok(if ready { 0 } else { 4 })
}

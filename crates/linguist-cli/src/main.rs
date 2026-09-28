//! Offline contract tooling. Collection commands are added only with verified native safety.
use clap::{Args, CommandFactory, Parser, Subcommand};
use linguist_core::{LearningDocument, canonical, model, render, validation};
use std::{collections::BTreeMap, path::PathBuf, process::ExitCode};
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
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
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
        #[arg(long)]
        offline: bool,
        #[arg(long, conflicts_with = "ollama")]
        local: bool,
        /// Probe only the configured local Ollama model metadata; never load or pull.
        #[arg(long)]
        ollama: bool,
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
}
#[derive(Subcommand)]
enum JobCommand {
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
    /// Upgrade existing local job storage after preserving a verified backup.
    Migrate,
    /// Request a durable pause; dispatched reads finish their accounting.
    Pause {
        job: uuid::Uuid,
    },
    /// Clear a pause request and run with the job's frozen settings.
    Resume {
        job: uuid::Uuid,
    },
    /// Prevent future dispatch; retain all checkpoints and assets.
    Cancel {
        job: uuid::Uuid,
    },
    /// Capture pending sources using the job's frozen settings; never applies.
    Run {
        job: uuid::Uuid,
    },
    /// Freeze selected existing note IDs; never reads note content or starts workers.
    Create {
        #[command(flatten)]
        selector: NoteSelector,
        /// Queue the first N query/deck matches in frozen order.
        #[arg(long, conflicts_with = "note_ids", requires = "NoteSelector", value_parser = clap::value_parser!(u64).range(1..=100000))]
        limit: Option<u64>,
    },
    List {
        #[arg(long)]
        after: Option<uuid::Uuid>,
        #[arg(long,value_parser=clap::value_parser!(u32).range(1..=10000))]
        limit: Option<u32>,
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
    /// Prepare one version-2 structured JSON record, from a file or piped stdin.
    Add {
        #[arg(long)]
        /// UTF-8 JSON file; use - to read one record from noninteractive stdin.
        document: PathBuf,
    },
}
#[derive(Subcommand)]
enum DeckCommand {
    List {
        #[arg(long)]
        counts: bool,
        #[arg(long)]
        limit: Option<usize>,
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
    Inspect {
        #[arg(conflicts_with = "pending", required_unless_present = "pending")]
        operation: Option<uuid::Uuid>,
        #[arg(long)]
        pending: bool,
        /// Native read-back is unavailable until the Anki adapter is implemented.
        #[arg(long)]
        live: bool,
    },
}
#[derive(Subcommand)]
enum PlanCommand {
    /// Split a retained grammar source into authored units with one explicit anchor.
    SplitGrammar {
        plan: uuid::Uuid,
        #[arg(long)]
        request: PathBuf,
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
        #[arg(long)]
        patch: PathBuf,
        #[arg(long)]
        save_draft: bool,
    },
    /// Compare exact saved revisions without regeneration or collection reads.
    Diff {
        plan: uuid::Uuid,
        #[arg(long)]
        from_revision: u32,
        #[arg(long)]
        revision: Option<u32>,
        #[arg(long)]
        live: bool,
    },
    List {
        #[arg(long)]
        limit: Option<u32>,
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
    /// Write minimal version-2 TOML; refuses an existing destination.
    Init {
        #[arg(long)]
        path: Option<PathBuf>,
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
    let output = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&output)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|e| format!("OUTPUT_IO: {e}"))
}
fn run(cli: Cli) -> Result<u8, String> {
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
            .map_err(|e| format!("OUTPUT_IO: {e}"))?;
        return Ok(0);
    }
    if let Command::Config { command } = &cli.command {
        return run_config(&cli, command);
    }
    let settings = load_effective(&cli)?;
    let max_bytes = settings.values["input.max_file_mb"].as_u64().unwrap() * 1024 * 1024;
    let max_chars = settings.values["input.max_record_chars"].as_u64().unwrap() as usize;
    let vocab_command = matches!(cli.command, Command::Vocab { .. });
    match cli.command {
        Command::Config { .. } | Command::Completions { .. } => unreachable!(),
        Command::Vocab {
            command: PrepareCommand::Add { document },
        }
        | Command::Grammar {
            command: PrepareCommand::Add { document },
        } => {
            let kind = if vocab_command {
                linguist_application::Kind::Vocabulary
            } else {
                linguist_application::Kind::Grammar
            };
            let bytes = read_input(&document, max_bytes, max_chars)?;
            let result = linguist_application::prepare_authored(
                &bytes,
                kind,
                &settings,
                &std::env::vars().collect(),
            )?;
            emit(&result)?;
            Ok(if result.ready { 0 } else { 4 })
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
            emit(
                &serde_json::json!({"preparation_stage":if settings.values["dictionary.provider"] == "authored" {"source_draft"} else {"source_dictionary_draft"},"result":result,"dictionary_enrichment_completed":settings.values["dictionary.provider"] != "authored","enrichment_completed":false,"collection_writes_enabled":false}),
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
            if live {
                return Err(
                    "CAPABILITY_UNAVAILABLE: live Anki recovery inspection is not implemented"
                        .into(),
                );
            }
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
            emit(
                &serde_json::json!({"version":2,"journals":journals,"total_pending":total_pending,"live_checked":false,"reconciliation_available":false,"state_exists":store.is_some()}),
            )?;
            Ok(0)
        }
        Command::Jobs { command } => {
            let env: BTreeMap<String, String> = std::env::vars().collect();
            if let JobCommand::Create { selector, limit } = command {
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
            if let JobCommand::Migrate = command {
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
            let control = match &command {
                JobCommand::Pause { job } => Some((
                    *job,
                    linguist_store::preparation_control::ControlAction::Pause,
                )),
                JobCommand::Resume { job } => Some((
                    *job,
                    linguist_store::preparation_control::ControlAction::Resume,
                )),
                JobCommand::Cancel { job } => Some((
                    *job,
                    linguist_store::preparation_control::ControlAction::Cancel,
                )),
                _ => None,
            };
            if let Some((job, action)) = control {
                // Unknown/missing jobs must never initialize state for a control request.
                linguist_store::Store::read_only(&root)?.preparation_job(job)?;
                let receipt = linguist_store::Store::open_existing(&root)?
                    .request_preparation_control(job, action)?;
                if action == linguist_store::preparation_control::ControlAction::Resume {
                    let result = linguist_application::jobs::run(&root, job, &env)?;
                    let exit = result["exit_code"].as_u64().ok_or("JOB_RESULT_INVALID")? as u8;
                    emit(&result)?;
                    return Ok(exit);
                }
                emit(
                    &serde_json::json!({"schema_version":2,"job_id":job,"control":receipt,"worker_stopped_confirmed":false,"writes_enabled":false}),
                )?;
                return Ok(0);
            }
            if let JobCommand::Run { job } = command {
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
                JobCommand::List { after, limit } => {
                    let jobs = store
                        .as_ref()
                        .map(|s| s.list_preparation_jobs(after, limit.unwrap_or(page)))
                        .transpose()?
                        .unwrap_or_default();
                    let next = jobs.last().map(|j| j.id);
                    emit(
                        &serde_json::json!({"schema_version":2,"jobs":jobs,"next_cursor":next,"state_exists":store.is_some(),"execution_available":false}),
                    )?;
                }
                JobCommand::Show { job } => {
                    let definition = store
                        .as_ref()
                        .ok_or("PREPARATION_JOB_NOT_FOUND")?
                        .preparation_job(job)?;
                    emit(
                        &serde_json::json!({"schema_version":2,"definition":definition,"control":store.as_ref().unwrap().preparation_control(job)?,"worker_liveness":"unverified","execution_available":false,"writes_enabled":false}),
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
                | JobCommand::Recover { .. }
                | JobCommand::Run { .. }
                | JobCommand::Pause { .. }
                | JobCommand::Resume { .. }
                | JobCommand::Cancel { .. }
                | JobCommand::Migrate => unreachable!(),
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
                PlanCommand::SplitGrammar { plan, request } => {
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
                PlanCommand::List { limit } => {
                    let limit = limit
                        .unwrap_or(settings.values["output.page_size"].as_u64().unwrap() as u32);
                    emit(
                        &serde_json::json!({"version":2,"revisions":store.list_revisions(limit)?,"state_exists":true}),
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
                    save_draft,
                } => {
                    if store.latest_revision(plan)? != base_revision {
                        return Err("PLAN_EDIT_BASE_CONFLICT: select the latest revision".into());
                    }
                    let base = store.revision(plan, base_revision)?;
                    let bytes = read_input(&patch, max_bytes, max_chars)?;
                    let patch: linguist_core::editing::PlanPatch =
                        canonical::parse(&bytes).map_err(|e| e.to_string())?;
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
                } => {
                    if live {
                        return Err("CAPABILITY_UNAVAILABLE: live plan conflict inspection requires native binding".into());
                    }
                    let revision = revision
                        .map(Ok)
                        .unwrap_or_else(|| store.latest_revision(plan))?;
                    let before = store.revision(plan, from_revision)?;
                    let after = store.revision(plan, revision)?;
                    emit(
                        &linguist_core::inspection::revision_diff(&before, &after)
                            .map_err(|e| e.to_string())?,
                    )?;
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
            offline,
            local,
            ollama,
        } => {
            if local {
                emit(
                    &serde_json::json!({"version":2,"collection_writes_enabled":false,"native_bridge":"not_implemented","release_gates":"not_run","services_probed":false}),
                )?;
                return Ok(0);
            }
            let mut settings = settings;
            if offline {
                settings
                    .values
                    .insert("network.offline".into(), serde_json::json!(true));
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
            let client = anki_client(&settings)?;
            let capabilities = client.capabilities()?;
            let ready = capabilities.read_ready;
            emit(
                &serde_json::json!({"version":2,"anki":capabilities,"provider_resources_checked":false,"release_gates":"not_run"}),
            )?;
            Ok(if ready { 0 } else { 3 })
        }
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
                return Err("CAPABILITY_UNAVAILABLE: native model installation requires verified bridge, checkpoint and journal support".into());
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
                &serde_json::json!({"schema_version":2,"purpose":purpose,"proposal":action,"target":target,"existing":inspection,"checkpoint_verified":false,"native_template_order_verified":false,"apply_eligible":false,"writes_enabled":false}),
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
                DeckCommand::List { counts, limit } => {
                    let limit = page_limit(limit, &settings)?;
                    let total = decks.len();
                    let mut rows = Vec::new();
                    for deck in decks.into_iter().take(limit) {
                        let count = if counts {
                            Some(client.counts(&linguist_anki::deck_query(&deck.name)?)?)
                        } else {
                            None
                        };
                        rows.push(serde_json::json!({"id":deck.id,"name":deck.name,"counts":count,"counts_include_subdecks":counts}));
                    }
                    emit(
                        &serde_json::json!({"version":2,"decks":rows,"total":total,"truncated":total>limit}),
                    )?;
                }
                DeckCommand::Show { deck } => {
                    let deck = linguist_anki::select_name(decks, &deck)?;
                    let query = linguist_anki::deck_query(&deck.name)?;
                    let counts = client.counts(&query)?;
                    let mut subdecks = client
                        .decks()?
                        .into_iter()
                        .filter(|d| d.name.starts_with(&format!("{}::", deck.name)))
                        .collect::<Vec<_>>();
                    subdecks.sort_by(|a, b| a.name.cmp(&b.name));
                    emit(
                        &serde_json::json!({"version":2,"deck":deck,"counts":counts,"counts_include_subdecks":true,"subdecks":subdecks,"source_model_manifest_checked":false}),
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
                    let client = anki_client(&settings)?;
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
                    if cursor
                        .as_ref()
                        .is_some_and(|c| c.rsplit_once('/').is_none())
                    {
                        return Err("INVALID_CURSOR".into());
                    }
                    let client = anki_client(&settings)?;
                    let ids = client.find_notes(&query)?;
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
    match run(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            let _ = writeln_error(&message);
            ExitCode::from(error_exit(&message))
        }
    }
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
fn run_config(cli: &Cli, command: &ConfigCommand) -> Result<u8, String> {
    use linguist_config::{ConfigFile, Registry, config_path, resolve};
    let registry = Registry::builtin();
    match command {
        ConfigCommand::Describe { key } => {
            emit(registry.lookup(key)?)?;
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
    let options = config_options(cli, &registry)?;
    let explicit = match command {
        ConfigCommand::Init { path } => path.as_deref().or(cli.config.as_deref()),
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
        linguist_config::initialize(&path)?;
        emit(&serde_json::json!({"version":2,"path":path,"created":true}))?;
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
    match command {
        ConfigCommand::Validate { .. } => emit(
            &serde_json::json!({"version":2,"valid":true,"fingerprint":effective.fingerprint,"runtime_resources_checked":false}),
        )?,
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
        "DICTIONARY_SETTING_MISSING" | "DICTIONARY_ARCHIVE_LIMIT" | "DICTIONARY_REVISION_LIMIT"
    ) {
        2
    } else if code == "DICTIONARY_ALREADY_ENRICHED" {
        5
    } else if code == "PREPARATION_ACTIVE_ITEM_REQUIRES_RECOVERY"
        || (code.starts_with("PREPARATION_") && code.ends_with("_CORRUPT"))
    {
        7
    } else if code == "DOCUMENT_NOT_READY" {
        4
    } else if code.contains("CONFLICT")
        || matches!(
            code,
            "STORAGE_RELOCATION_BLOCKED"
                | "PREPARATION_CANCEL_IS_TERMINAL"
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
    let options = config_options(cli, &registry)?;
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
    linguist_config::resolve(&registry, &file, &options)
}

fn anki_client(settings: &linguist_config::Effective) -> Result<linguist_anki::Client, String> {
    linguist_anki::Client::from_settings(settings, &std::env::vars().collect())
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

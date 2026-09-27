//! Bounded, cancellable enrichment composition with provider-result caching.

use linguist_application::{EnrichmentPort, PortError, PortFuture, SourceNote};
use linguist_core::{
    CardBuildInput, CardDocument, CardMode, DictionaryData, LlmResponse, ProcessedCardData,
    build_card_document,
};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub type PipelineFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderOutput, PipelineError>> + Send + 'a>>;
pub trait EnrichmentServices: Send + Sync {
    fn dictionary<'a>(&'a self, expression: &'a str, deck_key: &'a str) -> PipelineFuture<'a>;
    fn generation<'a>(
        &'a self,
        expression: &'a str,
        deck_key: &'a str,
        context: &'a str,
        dictionary: &'a DictionaryData,
    ) -> PipelineFuture<'a>;
    fn kanji<'a>(&'a self, expression: &'a str, deck_key: &'a str) -> PipelineFuture<'a>;
    fn image<'a>(&'a self, expression: &'a str, deck_key: &'a str) -> PipelineFuture<'a>;
    fn audio<'a>(
        &'a self,
        expression: &'a str,
        deck_key: &'a str,
        dictionary: &'a DictionaryData,
    ) -> PipelineFuture<'a>;
}
pub trait ProgressSink: Send + Sync {
    fn event(&self, event: PipelineEvent);
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipelineEvent {
    pub expression: String,
    pub service: String,
    pub state: PipelineState,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineState {
    Started,
    Cached,
    Finished,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderOutput {
    Unavailable,
    Dictionary(DictionaryData),
    Generation(LlmResponse),
    Kanji(String),
    Image {
        filename: String,
        b64: String,
        classification: String,
    },
    Audio(Vec<ProviderAudio>),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderAudio {
    pub filename: String,
    pub b64: String,
    pub reading: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PipelineError {
    Cancelled,
    Provider {
        service: &'static str,
        message: String,
        retryable: bool,
    },
    Unexpected(&'static str),
}
impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("enrichment cancelled"),
            Self::Provider {
                service, message, ..
            } => write!(f, "{service}: {message}"),
            Self::Unexpected(value) => write!(f, "unexpected {value} provider output"),
        }
    }
}
impl std::error::Error for PipelineError {}
#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}
#[derive(Clone, Default)]
pub struct Cancellation(Arc<CancellationState>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.notify.notify_waiters();
    }
    fn cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    async fn wait(&self) {
        loop {
            let notified = self.0.notify.notified();
            if self.cancelled() {
                return;
            }
            notified.await;
        }
    }
}
#[derive(Clone, Debug)]
pub struct PipelineConfig {
    pub rate_limits: BTreeMap<String, Duration>,
    pub max_in_flight: usize,
    pub cache_revision: String,
}
impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            rate_limits: BTreeMap::from([
                ("dictionary".into(), Duration::from_secs(1)),
                ("generation".into(), Duration::from_millis(250)),
                ("kanji".into(), Duration::from_secs(1)),
                ("image".into(), Duration::from_secs(1)),
                ("audio".into(), Duration::from_millis(500)),
            ]),
            max_in_flight: 4,
            cache_revision: "native-v1".into(),
        }
    }
}
pub struct NativePipeline<S> {
    services: Arc<S>,
    config: PipelineConfig,
    cache: Arc<Mutex<BTreeMap<String, ProviderOutput>>>,
    in_flight: Arc<Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    last_call: Arc<Mutex<BTreeMap<String, Instant>>>,
    cancellation: Cancellation,
    progress: Option<Arc<dyn ProgressSink>>,
}
impl<S> Clone for NativePipeline<S> {
    fn clone(&self) -> Self {
        Self {
            services: self.services.clone(),
            config: self.config.clone(),
            cache: self.cache.clone(),
            in_flight: self.in_flight.clone(),
            last_call: self.last_call.clone(),
            cancellation: self.cancellation.clone(),
            progress: self.progress.clone(),
        }
    }
}
impl<S: EnrichmentServices> NativePipeline<S> {
    pub fn new(services: S, config: PipelineConfig) -> Self {
        Self {
            services: Arc::new(services),
            config,
            cache: Arc::new(Mutex::new(BTreeMap::new())),
            in_flight: Arc::new(Mutex::new(BTreeMap::new())),
            last_call: Arc::new(Mutex::new(BTreeMap::new())),
            cancellation: Cancellation::default(),
            progress: None,
        }
    }
    pub fn with_progress(mut self, progress: Arc<dyn ProgressSink>) -> Self {
        self.progress = Some(progress);
        self
    }
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }
    pub async fn enrich_many(
        &self,
        requests: Vec<EnrichmentRequest>,
    ) -> Vec<Result<CardDocument, PipelineError>>
    where
        S: 'static,
    {
        let count = requests.len();
        let mut next = requests.into_iter().enumerate();
        let mut workers = tokio::task::JoinSet::new();
        let mut results = vec![None; count];
        for _ in 0..self.config.max_in_flight.max(1) {
            if let Some((index, request)) = next.next() {
                let pipeline = self.clone();
                workers.spawn(async move {
                    (
                        index,
                        pipeline
                            .enrich(
                                request.mode,
                                &request.deck_key,
                                &request.expression,
                                &request.context,
                            )
                            .await,
                    )
                });
            }
        }
        while let Some(result) = workers.join_next().await {
            let (index, value) = result.expect("enrichment worker must not panic");
            results[index] = Some(value);
            if let Some((index, request)) = next.next() {
                let pipeline = self.clone();
                workers.spawn(async move {
                    (
                        index,
                        pipeline
                            .enrich(
                                request.mode,
                                &request.deck_key,
                                &request.expression,
                                &request.context,
                            )
                            .await,
                    )
                });
            }
        }
        results
            .into_iter()
            .map(|result| result.expect("every enrichment request has a result"))
            .collect()
    }
    pub async fn enrich(
        &self,
        mode: CardMode,
        deck_key: &str,
        expression: &str,
        context: &str,
    ) -> Result<CardDocument, PipelineError> {
        self.enrich_with_input(CardBuildInput {
            mode,
            language_key: deck_key.into(),
            processed_data: ProcessedCardData {
                word: expression.into(),
                source_note: context.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
    }

    /// Enrich a source record without discarding its original media, tags,
    /// provenance, or artifacts already processed before a retry.
    pub async fn enrich_with_input(
        &self,
        mut input: CardBuildInput,
    ) -> Result<CardDocument, PipelineError> {
        let deck_key = input.language_key.clone();
        let expression = input.processed_data.word.clone();
        let context = input.processed_data.source_note.clone();
        let deck_key = deck_key.as_str();
        let expression = expression.as_str();
        let context = context.as_str();
        let dictionary_key = format!("{deck_key}\0{expression}");
        let mut issues = Vec::new();
        let dictionary = match self
            .call(expression, &dictionary_key, "dictionary", || {
                self.services.dictionary(expression, deck_key)
            })
            .await
        {
            Ok(ProviderOutput::Dictionary(value)) => value,
            Ok(ProviderOutput::Unavailable) => input.processed_data.scraped.clone(),
            Ok(_) => return Err(PipelineError::Unexpected("dictionary")),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(error) => {
                issues.push(error.to_string());
                input.processed_data.scraped.clone()
            }
        };
        let generation_key = format!(
            "{deck_key}\0{expression}\0{context}\0{}\0{}",
            dictionary.reading, dictionary.definition
        );
        let audio_key = format!(
            "{deck_key}\0{expression}\0{}\0{:?}",
            dictionary.reading, dictionary.pronunciations
        );
        let deck_expression_key = format!("{deck_key}\0{expression}");
        let (generation_result, kanji_result, image_result, audio_result) = tokio::join!(
            self.call(expression, &generation_key, "generation", || self
                .services
                .generation(expression, deck_key, context, &dictionary)),
            self.call(expression, &deck_expression_key, "kanji", || self
                .services
                .kanji(expression, deck_key)),
            self.call(expression, &deck_expression_key, "image", || self
                .services
                .image(expression, deck_key)),
            self.call(expression, &audio_key, "audio", || self.services.audio(
                expression,
                deck_key,
                &dictionary
            )),
        );
        let generation = match generation_result {
            Ok(ProviderOutput::Generation(value)) => value,
            Ok(ProviderOutput::Unavailable) => input.processed_data.llm_response.clone(),
            Ok(_) => return Err(PipelineError::Unexpected("generation")),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(error) => {
                issues.push(error.to_string());
                input.processed_data.llm_response.clone()
            }
        };
        let kanji = match kanji_result {
            Ok(ProviderOutput::Kanji(value)) => value,
            Ok(ProviderOutput::Unavailable) => input.processed_data.kanji_construction.clone(),
            Ok(_) => return Err(PipelineError::Unexpected("kanji")),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(error) => {
                issues.push(error.to_string());
                input.processed_data.kanji_construction.clone()
            }
        };
        let image = match image_result {
            Ok(value) => Some(value),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(error) => {
                issues.push(error.to_string());
                None
            }
        };
        let audio = match audio_result {
            Ok(value) => Some(value),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(error) => {
                issues.push(error.to_string());
                None
            }
        };
        let data = &mut input.processed_data;
        data.scraped = dictionary;
        data.llm_response = generation;
        if !kanji.is_empty() {
            data.kanji_construction = kanji;
        }
        if let Some(ProviderOutput::Image {
            filename,
            b64,
            classification,
        }) = image
        {
            data.new_image_filename = filename;
            data.new_image_b64 = Some(b64);
            data.classification = classification;
        }
        if let Some(ProviderOutput::Audio(assets)) = audio {
            if !assets.is_empty() {
                data.audio_assets = assets
                    .into_iter()
                    .map(|asset| linguist_core::AudioAsset {
                        filename: asset.filename,
                        b64: Some(asset.b64),
                        reading: asset.reading,
                    })
                    .collect();
            }
        }
        data.issues.extend(issues);
        Ok(build_card_document(input))
    }
    async fn call<F, O>(
        &self,
        expression: &str,
        input_key: &str,
        service: &'static str,
        fetch: F,
    ) -> Result<ProviderOutput, PipelineError>
    where
        F: FnOnce() -> O,
        O: Future<Output = Result<ProviderOutput, PipelineError>>,
    {
        if self.cancellation.cancelled() {
            self.event(expression, service, PipelineState::Cancelled);
            return Err(PipelineError::Cancelled);
        }
        let key = format!("{}:{service}:{input_key}", self.config.cache_revision);
        if let Some(value) = self.cache.lock().unwrap().get(&key).cloned() {
            self.event(expression, service, PipelineState::Cached);
            return Ok(value);
        }
        let flight = self
            .in_flight
            .lock()
            .unwrap()
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _flight_guard = tokio::select! {
            _ = self.cancellation.wait() => {
                self.event(expression, service, PipelineState::Cancelled);
                return Err(PipelineError::Cancelled);
            }
            guard = flight.lock() => guard,
        };
        if let Some(value) = self.cache.lock().unwrap().get(&key).cloned() {
            self.event(expression, service, PipelineState::Cached);
            return Ok(value);
        }
        self.event(expression, service, PipelineState::Started);
        if let Some(limit) = self.config.rate_limits.get(service).copied() {
            loop {
                let wait = {
                    let mut gate = self.last_call.lock().unwrap();
                    match gate
                        .get(service)
                        .and_then(|last| limit.checked_sub(last.elapsed()))
                    {
                        Some(wait) => Some(wait),
                        None => {
                            gate.insert(service.into(), Instant::now());
                            None
                        }
                    }
                };
                match wait {
                    Some(wait) => tokio::select! {
                        _ = self.cancellation.wait() => {
                            self.event(expression, service, PipelineState::Cancelled);
                            return Err(PipelineError::Cancelled);
                        }
                        _ = tokio::time::sleep(wait) => {}
                    },
                    None => break,
                }
            }
        };
        if self.cancellation.cancelled() {
            self.event(expression, service, PipelineState::Cancelled);
            return Err(PipelineError::Cancelled);
        }
        let value = tokio::select! {
            _ = self.cancellation.wait() => {
                self.event(expression, service, PipelineState::Cancelled);
                return Err(PipelineError::Cancelled);
            }
            value = fetch() => value,
        };
        match value {
            Ok(value) => {
                if value != ProviderOutput::Unavailable {
                    self.cache.lock().unwrap().insert(key, value.clone());
                }
                self.event(expression, service, PipelineState::Finished);
                Ok(value)
            }
            Err(error) => {
                self.event(expression, service, PipelineState::Failed);
                Err(error)
            }
        }
    }
    fn event(&self, expression: &str, service: &str, state: PipelineState) {
        if let Some(sink) = &self.progress {
            sink.event(PipelineEvent {
                expression: expression.into(),
                service: service.into(),
                state,
            })
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnrichmentRequest {
    pub mode: CardMode,
    pub deck_key: String,
    pub expression: String,
    pub context: String,
}
impl<S: EnrichmentServices> EnrichmentPort for NativePipeline<S> {
    fn modernize<'a>(&'a self, note: &'a SourceNote) -> PortFuture<'a, CardDocument> {
        Box::pin(async move {
            self.enrich(
                CardMode::Modernize,
                &note.summary.deck_key,
                &note.summary.expression,
                "",
            )
            .await
            .map_err(port_error)
        })
    }
    fn inject<'a>(
        &'a self,
        deck_key: &'a str,
        expression: &'a str,
        context: &'a str,
    ) -> PortFuture<'a, CardDocument> {
        Box::pin(async move {
            self.enrich(CardMode::Inject, deck_key, expression, context)
                .await
                .map_err(port_error)
        })
    }
}
fn port_error(error: PipelineError) -> PortError {
    match error {
        PipelineError::Provider {
            service,
            message,
            retryable,
        } => PortError {
            operation: service,
            message,
            retryable,
        },
        PipelineError::Cancelled => PortError {
            operation: "enrich",
            message: error.to_string(),
            retryable: true,
        },
        PipelineError::Unexpected(_) => PortError {
            operation: "enrich",
            message: error.to_string(),
            retryable: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::{Barrier, Notify};
    struct Fake {
        calls: AtomicUsize,
    }
    impl Fake {
        fn output(value: ProviderOutput) -> PipelineFuture<'static> {
            Box::pin(async move { Ok(value) })
        }
    }
    impl EnrichmentServices for Fake {
        fn dictionary<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Self::output(ProviderOutput::Dictionary(DictionaryData {
                found: true,
                word: "食べる".into(),
                reading: "たべる".into(),
                definition: "to eat".into(),
                ..Default::default()
            }))
        }
        fn generation<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Self::output(ProviderOutput::Generation(LlmResponse {
                nuances: "ordinary verb".into(),
                examples: vec![linguist_core::ExamplePair {
                    sentence: "食べる。".into(),
                    translation: "Eat.".into(),
                }],
                ..Default::default()
            }))
        }
        fn kanji<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Self::output(ProviderOutput::Kanji("食: eat".into()))
        }
        fn image<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Self::output(ProviderOutput::Image {
                filename: "wiki.jpg".into(),
                b64: "aW1n".into(),
                classification: "visual_recall".into(),
            })
        }
        fn audio<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Self::output(ProviderOutput::Audio(vec![
                ProviderAudio {
                    filename: "a.mp3".into(),
                    b64: "YQ==".into(),
                    reading: "たべる".into(),
                },
                ProviderAudio {
                    filename: "b.mp3".into(),
                    b64: "Yg==".into(),
                    reading: "食べる".into(),
                },
            ]))
        }
    }
    #[tokio::test]
    async fn builds_document_and_reuses_dictionary() {
        let pipeline = NativePipeline::new(
            Fake {
                calls: AtomicUsize::new(0),
            },
            PipelineConfig {
                rate_limits: BTreeMap::new(),
                max_in_flight: 2,
                cache_revision: "test-v1".into(),
            },
        );
        let one = pipeline
            .enrich(CardMode::Inject, "japanese_vocab", "食べる", "")
            .await
            .unwrap();
        let two = pipeline
            .enrich(CardMode::Inject, "japanese_vocab", "食べる", "")
            .await
            .unwrap();
        assert!(one.ready());
        assert_eq!(one.media.len(), 3);
        assert!(
            one.values
                .audio
                .as_deref()
                .is_some_and(|audio| audio.contains("a.mp3") && audio.contains("b.mp3"))
        );
        assert_eq!(one, two);
        assert_eq!(pipeline.services.calls.load(Ordering::Relaxed), 1);
    }

    struct DictionaryOutage;
    impl EnrichmentServices for DictionaryOutage {
        fn dictionary<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Box::pin(async {
                Err(PipelineError::Provider {
                    service: "dictionary",
                    message: "offline".into(),
                    retryable: true,
                })
            })
        }
        fn generation<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Generation(LlmResponse {
                nuances: "Generated without dictionary".into(),
                ..Default::default()
            }))
        }
        fn kanji<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
        fn image<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
        fn audio<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
    }

    #[tokio::test]
    async fn dictionary_outage_keeps_partial_generated_draft() {
        let pipeline = NativePipeline::new(
            DictionaryOutage,
            PipelineConfig {
                rate_limits: BTreeMap::new(),
                ..PipelineConfig::default()
            },
        );
        let document = pipeline
            .enrich(CardMode::Inject, "english_vocab", "word", "")
            .await
            .unwrap();
        assert!(
            document
                .issues
                .iter()
                .any(|issue| issue.contains("dictionary: offline"))
        );
        assert!(
            document
                .values
                .meaning_text
                .unwrap_or_default()
                .contains("Generated without dictionary")
        );
    }

    struct UnavailableServices;
    impl EnrichmentServices for UnavailableServices {
        fn dictionary<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
        fn generation<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
        fn kanji<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
        fn image<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
        fn audio<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
    }

    #[tokio::test]
    async fn processed_golden_inputs_keep_python_parity() {
        let pipeline = NativePipeline::new(
            UnavailableServices,
            PipelineConfig {
                rate_limits: BTreeMap::new(),
                ..PipelineConfig::default()
            },
        );
        let fixtures = [
            (
                "modernization",
                include_str!("../../../contracts/fixture-sources/modernization.json"),
                include_str!("../../../contracts/fixtures/modernization-card.v1.json"),
            ),
            (
                "injection",
                include_str!("../../../contracts/fixture-sources/injection.json"),
                include_str!("../../../contracts/fixtures/injection-card.v1.json"),
            ),
            (
                "shared fields",
                include_str!("../../../contracts/fixture-sources/shared-fields.json"),
                include_str!("../../../contracts/fixtures/shared-fields-card.v1.json"),
            ),
            (
                "media replacement",
                include_str!("../../../contracts/fixture-sources/media-replacement.json"),
                include_str!("../../../contracts/fixtures/media-replacement-card.v1.json"),
            ),
            (
                "validation issues",
                include_str!("../../../contracts/fixture-sources/validation-issues.json"),
                include_str!("../../../contracts/fixtures/validation-issues-card.v1.json"),
            ),
            (
                "grammar",
                include_str!("../../../contracts/fixture-sources/grammar.json"),
                include_str!("../../../contracts/fixtures/grammar-card.v1.json"),
            ),
            (
                "dictionary preserve",
                include_str!("../../../contracts/fixture-sources/dictionary-preserve.json"),
                include_str!("../../../contracts/fixtures/dictionary-preserve-card.v1.json"),
            ),
        ];
        for (name, source, expected) in fixtures {
            let input: CardBuildInput = serde_json::from_str(source).unwrap();
            let expected: CardDocument = serde_json::from_str(expected).unwrap();
            let actual = pipeline.enrich_with_input(input).await.unwrap();
            assert_eq!(actual, expected, "{name}");
        }
    }

    struct DeckSensitive {
        kanji_calls: AtomicUsize,
        image_calls: AtomicUsize,
    }
    impl EnrichmentServices for DeckSensitive {
        fn dictionary<'a>(&'a self, expression: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Dictionary(DictionaryData {
                found: true,
                word: expression.into(),
                ..Default::default()
            }))
        }
        fn generation<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Generation(LlmResponse::default()))
        }
        fn kanji<'a>(&'a self, _: &'a str, deck_key: &'a str) -> PipelineFuture<'a> {
            self.kanji_calls.fetch_add(1, Ordering::Relaxed);
            Fake::output(ProviderOutput::Kanji(deck_key.into()))
        }
        fn image<'a>(&'a self, _: &'a str, deck_key: &'a str) -> PipelineFuture<'a> {
            self.image_calls.fetch_add(1, Ordering::Relaxed);
            if deck_key.ends_with("grammar") {
                Fake::output(ProviderOutput::Unavailable)
            } else {
                Fake::output(ProviderOutput::Image {
                    filename: "vocabulary.jpg".into(),
                    b64: "aW1n".into(),
                    classification: "visual_recall".into(),
                })
            }
        }
        fn audio<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
    }

    #[tokio::test]
    async fn cache_separates_deck_purposes_and_retries_unavailable_services() {
        let pipeline = NativePipeline::new(
            DeckSensitive {
                kanji_calls: AtomicUsize::new(0),
                image_calls: AtomicUsize::new(0),
            },
            PipelineConfig {
                rate_limits: BTreeMap::new(),
                ..PipelineConfig::default()
            },
        );
        for deck in ["japanese_vocab", "japanese_grammar", "japanese_grammar"] {
            pipeline
                .enrich(CardMode::Inject, deck, "同じ", "")
                .await
                .unwrap();
        }
        assert_eq!(pipeline.services.kanji_calls.load(Ordering::Relaxed), 2);
        assert_eq!(pipeline.services.image_calls.load(Ordering::Relaxed), 3);
    }
    #[tokio::test]
    async fn cancellation_stops_before_provider() {
        let pipeline = NativePipeline::new(
            Fake {
                calls: AtomicUsize::new(0),
            },
            PipelineConfig::default(),
        );
        pipeline.cancellation().cancel();
        assert!(matches!(
            pipeline
                .enrich(CardMode::Inject, "japanese_vocab", "食べる", "")
                .await,
            Err(PipelineError::Cancelled)
        ));
        assert_eq!(pipeline.services.calls.load(Ordering::Relaxed), 0);
    }

    struct Pending {
        started: Arc<Notify>,
    }
    impl EnrichmentServices for Pending {
        fn dictionary<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Box::pin(async move {
                self.started.notify_one();
                std::future::pending().await
            })
        }
        fn generation<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            unreachable!()
        }
        fn kanji<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            unreachable!()
        }
        fn image<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            unreachable!()
        }
        fn audio<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn cancellation_interrupts_in_flight_provider() {
        let started = Arc::new(Notify::new());
        let pipeline = NativePipeline::new(
            Pending {
                started: started.clone(),
            },
            PipelineConfig {
                rate_limits: BTreeMap::new(),
                ..PipelineConfig::default()
            },
        );
        let cancellation = pipeline.cancellation();
        let task = tokio::spawn(async move {
            pipeline
                .enrich(CardMode::Inject, "japanese_vocab", "食べる", "")
                .await
        });
        started.notified().await;
        cancellation.cancel();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap(),
            Err(PipelineError::Cancelled)
        ));
    }

    #[derive(Default)]
    struct Events(Mutex<Vec<PipelineEvent>>);
    impl ProgressSink for Events {
        fn event(&self, event: PipelineEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[tokio::test]
    async fn progress_distinguishes_fresh_and_cached_outputs() {
        let events = Arc::new(Events::default());
        let pipeline = NativePipeline::new(
            Fake {
                calls: AtomicUsize::new(0),
            },
            PipelineConfig {
                rate_limits: BTreeMap::new(),
                ..PipelineConfig::default()
            },
        )
        .with_progress(events.clone());
        for _ in 0..2 {
            pipeline
                .enrich(CardMode::Inject, "japanese_vocab", "食べる", "")
                .await
                .unwrap();
        }
        let events = events.0.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.state == PipelineState::Started)
        );
        assert!(
            events
                .iter()
                .any(|event| event.state == PipelineState::Finished)
        );
        assert!(
            events
                .iter()
                .any(|event| event.state == PipelineState::Cached)
        );
    }

    struct Bounded {
        active: AtomicUsize,
        maximum: AtomicUsize,
        calls: AtomicUsize,
        starts: Mutex<Vec<Instant>>,
    }
    impl EnrichmentServices for Bounded {
        fn dictionary<'a>(&'a self, expression: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.starts.lock().unwrap().push(Instant::now());
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.maximum.fetch_max(active, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(15)).await;
                self.active.fetch_sub(1, Ordering::SeqCst);
                Ok(ProviderOutput::Dictionary(DictionaryData {
                    found: true,
                    word: expression.into(),
                    ..Default::default()
                }))
            })
        }
        fn generation<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Generation(LlmResponse::default()))
        }
        fn kanji<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
        fn image<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
        fn audio<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            Fake::output(ProviderOutput::Unavailable)
        }
    }

    #[tokio::test]
    async fn enrich_many_honors_request_bound() {
        let pipeline = NativePipeline::new(
            Bounded {
                active: AtomicUsize::new(0),
                maximum: AtomicUsize::new(0),
                calls: AtomicUsize::new(0),
                starts: Mutex::new(Vec::new()),
            },
            PipelineConfig {
                rate_limits: BTreeMap::new(),
                max_in_flight: 2,
                cache_revision: "bounded".into(),
            },
        );
        let requests = (0..8)
            .map(|index| EnrichmentRequest {
                mode: CardMode::Inject,
                deck_key: "english_vocab".into(),
                expression: format!("word-{index}"),
                context: String::new(),
            })
            .collect();
        assert!(
            pipeline
                .enrich_many(requests)
                .await
                .into_iter()
                .all(|item| item.is_ok())
        );
        assert_eq!(pipeline.services.maximum.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn concurrent_duplicate_requests_share_processed_artifacts() {
        let pipeline = NativePipeline::new(
            Bounded {
                active: AtomicUsize::new(0),
                maximum: AtomicUsize::new(0),
                calls: AtomicUsize::new(0),
                starts: Mutex::new(Vec::new()),
            },
            PipelineConfig {
                rate_limits: BTreeMap::new(),
                max_in_flight: 2,
                cache_revision: "single-flight".into(),
            },
        );
        let request = EnrichmentRequest {
            mode: CardMode::Inject,
            deck_key: "english_vocab".into(),
            expression: "word".into(),
            context: String::new(),
        };
        let results = pipeline.enrich_many(vec![request.clone(), request]).await;
        assert!(results.iter().all(Result::is_ok));
        assert_eq!(results[0], results[1]);
        assert_eq!(pipeline.services.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rate_limit_spaces_calls_to_same_service() {
        let pipeline = NativePipeline::new(
            Bounded {
                active: AtomicUsize::new(0),
                maximum: AtomicUsize::new(0),
                calls: AtomicUsize::new(0),
                starts: Mutex::new(Vec::new()),
            },
            PipelineConfig {
                rate_limits: BTreeMap::from([("dictionary".into(), Duration::from_millis(40))]),
                max_in_flight: 2,
                cache_revision: "rate-limit".into(),
            },
        );
        let requests = ["first", "second"].map(|expression| EnrichmentRequest {
            mode: CardMode::Inject,
            deck_key: "english_vocab".into(),
            expression: expression.into(),
            context: String::new(),
        });
        assert!(
            pipeline
                .enrich_many(requests.into())
                .await
                .iter()
                .all(Result::is_ok)
        );
        let starts = pipeline.services.starts.lock().unwrap();
        assert_eq!(starts.len(), 2);
        assert!(starts[1].duration_since(starts[0]) >= Duration::from_millis(35));
    }

    struct Concurrent {
        barrier: Arc<Barrier>,
    }
    impl Concurrent {
        fn wait(&self, output: ProviderOutput) -> PipelineFuture<'_> {
            Box::pin(async move {
                self.barrier.wait().await;
                Ok(output)
            })
        }
    }
    impl EnrichmentServices for Concurrent {
        fn dictionary<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Box::pin(async {
                Ok(ProviderOutput::Dictionary(DictionaryData {
                    found: true,
                    word: "word".into(),
                    reading: "reading".into(),
                    definition: "definition".into(),
                    ..Default::default()
                }))
            })
        }
        fn generation<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            self.wait(ProviderOutput::Generation(LlmResponse::default()))
        }
        fn kanji<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            self.wait(ProviderOutput::Kanji(String::new()))
        }
        fn image<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            self.wait(ProviderOutput::Image {
                filename: "image.jpg".into(),
                b64: "aQ==".into(),
                classification: "dictionary".into(),
            })
        }
        fn audio<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a DictionaryData,
        ) -> PipelineFuture<'a> {
            self.wait(ProviderOutput::Audio(vec![ProviderAudio {
                filename: "audio.mp3".into(),
                b64: "YQ==".into(),
                reading: "reading".into(),
            }]))
        }
    }

    #[tokio::test]
    async fn downstream_services_run_concurrently_after_dictionary() {
        let pipeline = NativePipeline::new(
            Concurrent {
                barrier: Arc::new(Barrier::new(4)),
            },
            PipelineConfig {
                rate_limits: BTreeMap::new(),
                ..PipelineConfig::default()
            },
        );
        let document = tokio::time::timeout(
            Duration::from_secs(1),
            pipeline.enrich(CardMode::Inject, "test", "word", ""),
        )
        .await
        .expect("downstream services must reach the barrier together")
        .unwrap();
        assert!(document.ready());
    }
}

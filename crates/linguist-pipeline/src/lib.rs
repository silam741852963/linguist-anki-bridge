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
    fn dictionary<'a>(&'a self, expression: &'a str) -> PipelineFuture<'a>;
    fn generation<'a>(
        &'a self,
        expression: &'a str,
        dictionary: &'a DictionaryData,
    ) -> PipelineFuture<'a>;
    fn kanji<'a>(&'a self, expression: &'a str) -> PipelineFuture<'a>;
    fn image<'a>(&'a self, expression: &'a str) -> PipelineFuture<'a>;
    fn audio<'a>(&'a self, expression: &'a str, reading: &'a str) -> PipelineFuture<'a>;
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
    Audio {
        filename: String,
        b64: String,
        reading: String,
    },
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
#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release)
    }
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
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
        let dictionary = match self
            .call(expression, expression, "dictionary", || {
                self.services.dictionary(expression)
            })
            .await?
        {
            ProviderOutput::Dictionary(value) => value,
            _ => return Err(PipelineError::Unexpected("dictionary")),
        };
        let generation_key = format!(
            "{expression}\0{}\0{}",
            dictionary.reading, dictionary.definition
        );
        let audio_key = format!("{expression}\0{}", dictionary.reading);
        let (generation_result, kanji_result, image_result, audio_result) = tokio::join!(
            self.call(expression, &generation_key, "generation", || self
                .services
                .generation(expression, &dictionary)),
            self.call(expression, expression, "kanji", || self
                .services
                .kanji(expression)),
            self.call(expression, expression, "image", || self
                .services
                .image(expression)),
            self.call(expression, &audio_key, "audio", || self
                .services
                .audio(expression, &dictionary.reading)),
        );
        let mut issues = Vec::new();
        let generation = match generation_result {
            Ok(ProviderOutput::Generation(value)) => value,
            Ok(_) => return Err(PipelineError::Unexpected("generation")),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(error) => {
                self.issue(expression, "generation", &error);
                issues.push(error.to_string());
                LlmResponse::default()
            }
        };
        let kanji = match kanji_result {
            Ok(ProviderOutput::Kanji(value)) => value,
            Ok(_) => return Err(PipelineError::Unexpected("kanji")),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(error) => {
                self.issue(expression, "kanji", &error);
                issues.push(error.to_string());
                String::new()
            }
        };
        let image = match image_result {
            Ok(value) => Some(value),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(error) => {
                self.issue(expression, "image", &error);
                issues.push(error.to_string());
                None
            }
        };
        let audio = match audio_result {
            Ok(value) => Some(value),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(error) => {
                self.issue(expression, "audio", &error);
                issues.push(error.to_string());
                None
            }
        };
        if !dictionary.found {
            issues.push("Dictionary entry not found".into())
        };
        let (new_image_filename, new_image_b64, classification) = match image {
            Some(ProviderOutput::Image {
                filename,
                b64,
                classification,
            }) => (filename, Some(b64), classification),
            _ => (String::new(), None, "uncertain".into()),
        };
        let audio_assets = match audio {
            Some(ProviderOutput::Audio {
                filename,
                b64,
                reading,
            }) => vec![linguist_core::AudioAsset {
                filename,
                b64: Some(b64),
                reading,
            }],
            _ => Vec::new(),
        };
        Ok(build_card_document(CardBuildInput {
            mode,
            language_key: deck_key.into(),
            processed_data: ProcessedCardData {
                word: expression.into(),
                scraped: dictionary,
                llm_response: generation,
                kanji_construction: kanji,
                new_image_filename,
                new_image_b64,
                classification,
                audio_assets,
                source_note: context.into(),
                issues,
                ..Default::default()
            },
            ..Default::default()
        }))
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
                    Some(wait) => tokio::time::sleep(wait).await,
                    None => break,
                }
            }
        };
        if self.cancellation.cancelled() {
            self.event(expression, service, PipelineState::Cancelled);
            return Err(PipelineError::Cancelled);
        }
        let value = fetch().await;
        match value {
            Ok(value) => {
                self.cache.lock().unwrap().insert(key, value.clone());
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
    fn issue(&self, expression: &str, service: &str, error: &PipelineError) {
        self.event(
            expression,
            service,
            if matches!(error, PipelineError::Cancelled) {
                PipelineState::Cancelled
            } else {
                PipelineState::Failed
            },
        )
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
    use tokio::sync::Barrier;
    struct Fake {
        calls: AtomicUsize,
    }
    impl Fake {
        fn output(value: ProviderOutput) -> PipelineFuture<'static> {
            Box::pin(async move { Ok(value) })
        }
    }
    impl EnrichmentServices for Fake {
        fn dictionary<'a>(&'a self, _: &'a str) -> PipelineFuture<'a> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Self::output(ProviderOutput::Dictionary(DictionaryData {
                found: true,
                word: "食べる".into(),
                reading: "たべる".into(),
                definition: "to eat".into(),
            }))
        }
        fn generation<'a>(&'a self, _: &'a str, _: &'a DictionaryData) -> PipelineFuture<'a> {
            Self::output(ProviderOutput::Generation(LlmResponse {
                nuances: "ordinary verb".into(),
                examples: vec![linguist_core::ExamplePair {
                    sentence: "食べる。".into(),
                    translation: "Eat.".into(),
                }],
                ..Default::default()
            }))
        }
        fn kanji<'a>(&'a self, _: &'a str) -> PipelineFuture<'a> {
            Self::output(ProviderOutput::Kanji("食: eat".into()))
        }
        fn image<'a>(&'a self, _: &'a str) -> PipelineFuture<'a> {
            Self::output(ProviderOutput::Image {
                filename: "wiki.jpg".into(),
                b64: "aW1n".into(),
                classification: "visual_recall".into(),
            })
        }
        fn audio<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            Self::output(ProviderOutput::Audio {
                filename: "a.mp3".into(),
                b64: "YQ==".into(),
                reading: "たべる".into(),
            })
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
        assert_eq!(one, two);
        assert_eq!(pipeline.services.calls.load(Ordering::Relaxed), 1);
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
        fn dictionary<'a>(&'a self, _: &'a str) -> PipelineFuture<'a> {
            Box::pin(async {
                Ok(ProviderOutput::Dictionary(DictionaryData {
                    found: true,
                    word: "word".into(),
                    reading: "reading".into(),
                    definition: "definition".into(),
                }))
            })
        }
        fn generation<'a>(&'a self, _: &'a str, _: &'a DictionaryData) -> PipelineFuture<'a> {
            self.wait(ProviderOutput::Generation(LlmResponse::default()))
        }
        fn kanji<'a>(&'a self, _: &'a str) -> PipelineFuture<'a> {
            self.wait(ProviderOutput::Kanji(String::new()))
        }
        fn image<'a>(&'a self, _: &'a str) -> PipelineFuture<'a> {
            self.wait(ProviderOutput::Image {
                filename: "image.jpg".into(),
                b64: "aQ==".into(),
                classification: "dictionary".into(),
            })
        }
        fn audio<'a>(&'a self, _: &'a str, _: &'a str) -> PipelineFuture<'a> {
            self.wait(ProviderOutput::Audio {
                filename: "audio.mp3".into(),
                b64: "YQ==".into(),
                reading: "reading".into(),
            })
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

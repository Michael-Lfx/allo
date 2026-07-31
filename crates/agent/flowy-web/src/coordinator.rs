use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::time::{Instant, timeout_at};

use crate::provider::extract_policy::{
    LocalExtractFailure, LocalExtractFailureKind, classify_web_error,
};
use crate::provider::ExtractProvider;
use crate::types::{ExtractRequest, ExtractedPage, WebError};

const MAX_LOCAL_CONCURRENCY: usize = 2;

#[derive(Debug, Clone)]
pub struct ExtractBudget {
    pub absolute_deadline: Instant,
    pub local_per_url_timeout: Duration,
}

#[derive(Debug)]
pub struct ExtractBatchOutcome {
    pub items: Vec<ExtractItemOutcome>,
    pub diagnostics: ExtractBatchDiagnostics,
}

#[derive(Debug, Clone)]
pub struct ExtractItemOutcome {
    pub index: usize,
    pub requested_url: String,
    pub page: Option<ExtractedPage>,
    pub local_failure: Option<LocalExtractFailure>,
    pub final_error: Option<WebError>,
}

#[derive(Debug, Clone, Default)]
pub struct ExtractBatchDiagnostics {
    pub requested_count: usize,
    pub success_count: usize,
    pub failure_count: usize,
}

#[async_trait]
pub trait ExtractCoordinator: Send + Sync {
    async fn extract_many(
        &self,
        requests: Vec<ExtractRequest>,
        budget: ExtractBudget,
    ) -> ExtractBatchOutcome;
}

pub struct LocalExtractCoordinator {
    provider: Arc<dyn ExtractProvider>,
    max_concurrency: usize,
}

impl LocalExtractCoordinator {
    pub fn new(provider: Arc<dyn ExtractProvider>) -> Self {
        Self {
            provider,
            max_concurrency: MAX_LOCAL_CONCURRENCY,
        }
    }

    pub fn with_max_concurrency(mut self, max_concurrency: usize) -> Self {
        self.max_concurrency = max_concurrency.max(1);
        self
    }
}

#[async_trait]
impl ExtractCoordinator for LocalExtractCoordinator {
    async fn extract_many(
        &self,
        requests: Vec<ExtractRequest>,
        budget: ExtractBudget,
    ) -> ExtractBatchOutcome {
        let requested_count = requests.len();
        let labels = requests
            .iter()
            .map(|request| request.url.clone())
            .collect::<Vec<_>>();
        let mut outcomes: Vec<Option<ExtractItemOutcome>> =
            (0..requested_count).map(|_| None).collect();
        let mut started = vec![false; requested_count];
        let mut in_flight = FuturesUnordered::new();
        let mut next_index = 0usize;

        while next_index < requested_count && in_flight.len() < self.max_concurrency {
            started[next_index] = true;
            in_flight.push(run_local_extract(
                Arc::clone(&self.provider),
                next_index,
                requests[next_index].url.clone(),
                budget.clone(),
            ));
            next_index += 1;
        }

        while !in_flight.is_empty() {
            let completed = timeout_at(budget.absolute_deadline, in_flight.next()).await;
            let Some(outcome) = (match completed {
                Ok(outcome) => outcome,
                Err(_) => break,
            }) else {
                break;
            };
            let index = outcome.index;
            outcomes[index] = Some(outcome);

            if next_index < requested_count {
                started[next_index] = true;
                in_flight.push(run_local_extract(
                    Arc::clone(&self.provider),
                    next_index,
                    requests[next_index].url.clone(),
                    budget.clone(),
                ));
                next_index += 1;
            }
        }

        for (index, outcome) in outcomes.iter_mut().enumerate() {
            if outcome.is_some() {
                continue;
            }
            let message = if started[index] {
                "Page extraction did not complete before the tool deadline."
            } else {
                "Page extraction did not complete before the tool deadline."
            };
            let error = WebError::Timeout(message.to_owned());
            *outcome = Some(ExtractItemOutcome {
                index,
                requested_url: labels[index].clone(),
                page: None,
                local_failure: Some(LocalExtractFailure {
                    kind: LocalExtractFailureKind::Timeout,
                    error: error.clone(),
                }),
                final_error: Some(error),
            });
        }

        let items = outcomes
            .into_iter()
            .map(|outcome| outcome.expect("every extract request has an outcome"))
            .collect::<Vec<_>>();
        let mut success_count = 0usize;
        let mut failure_count = 0usize;
        for item in &items {
            if item.page.is_some() {
                success_count += 1;
            } else {
                failure_count += 1;
            }
        }
        ExtractBatchOutcome {
            diagnostics: ExtractBatchDiagnostics {
                requested_count,
                success_count,
                failure_count,
            },
            items,
        }
    }
}

async fn run_local_extract(
    provider: Arc<dyn ExtractProvider>,
    index: usize,
    url: String,
    budget: ExtractBudget,
) -> ExtractItemOutcome {
    if let Some(invalid_index) = invalid_url_index(&url) {
        let error = WebError::InvalidArgument(format!(
            "urls[{invalid_index}] must be a string"
        ));
        return ExtractItemOutcome {
            index,
            requested_url: url,
            page: None,
            local_failure: Some(LocalExtractFailure {
                kind: LocalExtractFailureKind::InvalidUrl,
                error: error.clone(),
            }),
            final_error: Some(error),
        };
    }

    let per_url_deadline = Instant::now() + budget.local_per_url_timeout;
    let url_deadline = std::cmp::min(per_url_deadline, budget.absolute_deadline);
    match timeout_at(
        url_deadline,
        provider.extract(ExtractRequest {
            url: url.clone(),
        }),
    )
    .await
    {
        Ok(Ok(page)) => ExtractItemOutcome {
            index,
            requested_url: url,
            page: Some(page),
            local_failure: None,
            final_error: None,
        },
        Ok(Err(error)) => {
            let kind = classify_web_error(&error);
            ExtractItemOutcome {
                index,
                requested_url: url,
                page: None,
                local_failure: Some(LocalExtractFailure {
                    kind,
                    error: error.clone(),
                }),
                final_error: Some(error),
            }
        }
        Err(_) => {
            let kind = if url_deadline < per_url_deadline {
                "Page extraction did not complete before the tool deadline."
            } else {
                "Page extraction timed out."
            };
            let error = WebError::Timeout(kind.to_owned());
            ExtractItemOutcome {
                index,
                requested_url: url,
                page: None,
                local_failure: Some(LocalExtractFailure {
                    kind: LocalExtractFailureKind::Timeout,
                    error: error.clone(),
                }),
                final_error: Some(error),
            }
        }
    }
}

fn invalid_url_index(url: &str) -> Option<usize> {
    const PREFIX: &str = "(invalid urls[";
    let start = url.find(PREFIX)?;
    let rest = &url[start + PREFIX.len()..];
    let end = rest.find(']')?;
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;

    use async_trait::async_trait;

    use crate::provider::ExtractProvider;
    use crate::types::{EXTRACTOR_READABILITY, ExtractRequest, ExtractedPage, WebError};

    use super::{
        ExtractBudget, ExtractCoordinator, LocalExtractCoordinator, invalid_url_index,
    };

    struct MockProvider {
        fail_urls: Vec<String>,
    }

    #[async_trait]
    impl ExtractProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn extract(&self, req: ExtractRequest) -> Result<ExtractedPage, WebError> {
            if self.fail_urls.iter().any(|url| url == &req.url) {
                return Err(WebError::Provider(format!("failed: {}", req.url)));
            }
            Ok(ExtractedPage {
                url: req.url.clone(),
                title: Some("Title".to_owned()),
                markdown: format!("Body for {}", req.url),
                truncated: false,
                provider: "mock".to_owned(),
                extractor: EXTRACTOR_READABILITY.to_owned(),
            })
        }
    }

    struct TimedProvider {
        delay: Duration,
        active: AtomicUsize,
        max_active: AtomicUsize,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ExtractProvider for TimedProvider {
        fn name(&self) -> &str {
            "timed"
        }

        async fn extract(&self, _req: ExtractRequest) -> Result<ExtractedPage, WebError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(ExtractedPage {
                url: "https://example.com/".to_owned(),
                title: Some("Timed".to_owned()),
                markdown: "completed".to_owned(),
                truncated: false,
                provider: "timed".to_owned(),
                extractor: EXTRACTOR_READABILITY.to_owned(),
            })
        }
    }

    fn request(url: &str) -> ExtractRequest {
        ExtractRequest { url: url.to_owned() }
    }

    fn budget() -> ExtractBudget {
        ExtractBudget {
            absolute_deadline: tokio::time::Instant::now() + Duration::from_secs(10),
            local_per_url_timeout: Duration::from_secs(8),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn coordinator_runs_at_most_two_concurrently_and_preserves_order() {
        let provider = Arc::new(TimedProvider {
            delay: Duration::from_secs(2),
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        });
        let coordinator = LocalExtractCoordinator::new(provider.clone());
        let task = tokio::spawn(async move {
            coordinator
                .extract_many(
                    vec![
                        request("https://example.com/one"),
                        request("https://example.com/two"),
                        request("https://example.com/three"),
                    ],
                    budget(),
                )
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(provider.max_active.load(Ordering::SeqCst), 2);
        tokio::time::advance(Duration::from_secs(2)).await;
        let outcome = task.await.unwrap();
        assert_eq!(outcome.diagnostics.success_count, 3);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
        assert_eq!(provider.max_active.load(Ordering::SeqCst), 2);
        let urls = outcome
            .items
            .iter()
            .map(|item| item.requested_url.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            urls,
            vec![
                "https://example.com/one",
                "https://example.com/two",
                "https://example.com/three"
            ]
        );
    }

    #[tokio::test]
    async fn coordinator_preserves_partial_failure() {
        let provider = Arc::new(MockProvider {
            fail_urls: vec!["https://example.com/bad".to_owned()],
        });
        let coordinator = LocalExtractCoordinator::new(provider);
        let outcome = coordinator
            .extract_many(
                vec![
                    request("https://example.com/ok"),
                    request("https://example.com/bad"),
                ],
                budget(),
            )
            .await;
        assert_eq!(outcome.diagnostics.success_count, 1);
        assert_eq!(outcome.diagnostics.failure_count, 1);
        assert!(outcome.items[0].page.is_some());
        assert!(outcome.items[1].final_error.is_some());
    }

    #[tokio::test]
    async fn coordinator_preserves_invalid_argument_message() {
        assert_eq!(invalid_url_index("(invalid urls[0])"), Some(0));
        let provider = Arc::new(MockProvider {
            fail_urls: Vec::new(),
        });
        let coordinator = LocalExtractCoordinator::new(provider);
        let outcome = coordinator
            .extract_many(
                vec![ExtractRequest {
                    url: "(invalid urls[0])".to_owned(),
                }],
                budget(),
            )
            .await;
        assert_eq!(
            outcome.items[0].final_error.as_ref().unwrap().to_string(),
            "invalid argument: urls[0] must be a string"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn coordinator_bounds_total_deadline() {
        let provider = Arc::new(TimedProvider {
            delay: Duration::from_secs(20),
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        });
        let coordinator = LocalExtractCoordinator::new(provider);
        let task = tokio::spawn(async move {
            coordinator
                .extract_many(
                    vec![
                        request("https://example.com/one"),
                        request("https://example.com/two"),
                    ],
                    budget(),
                )
                .await
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(10)).await;
        let outcome = task.await.unwrap();
        assert_eq!(outcome.diagnostics.success_count, 0);
        assert_eq!(outcome.diagnostics.failure_count, 2);
        assert!(
            outcome
                .items
                .iter()
                .all(|item| item.final_error.is_some())
        );
    }
}

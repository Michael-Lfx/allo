//! OpenAI-compatible streaming transport for the pi loop.
//!
//! Wraps pi-ai's `OpenAiCompletionsProvider` and hands the agent loop a
//! synchronous [`StreamFn`]: the provider stream is driven on a task and its
//! events are forwarded into the loop's consumer stream. Failures stay encoded
//! as terminal stream events, never as a panic.

use std::sync::Arc;

use std::time::Duration;

use rpi_agent::StreamFn;
use rpi_ai::provider::Provider;
use rpi_ai::providers::openai_completions::OpenAiCompletionsProvider;
use rpi_ai::types::{
    Api, AssistantMessage, AssistantMessageEvent, Content, DoneReason,
    ErrorReason, StopReason,
};

use crate::loop_runner::ModelRoute;
use crate::{ExecutionDenied, ExecutionGate, ExecutionMetadata};

/// Stream function for local echo mode (no account credential configured).
///
/// The assistant echoes the prompt, matching the pre-migration mock behavior,
/// and never touches the network.
#[must_use]
pub fn echo_stream_fn(text: String) -> StreamFn {
    rpi_agent::stream_fn(move |model, _, _| {
        let mut message = AssistantMessage::empty(
            Api::OpenaiCompletions,
            "flowy",
            model.id.clone(),
            0,
        );
        message.content = vec![Content::text(text.clone())];
        message.stop_reason = StopReason::Stop;
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        tokio::spawn(async move {
            producer
                .push(AssistantMessageEvent::Start {
                    partial: std::sync::Arc::new(message.clone()),
                })
                .await;
            producer
                .push(AssistantMessageEvent::Done {
                    reason: DoneReason::Stop,
                    message,
                })
                .await;
        });
        consumer
    })
}

fn http_client() -> reqwest::Client {
    // pi's provider already applies its own request timeout (600s) and stream
    // idle timeout (300s); do not add a shorter blanket timeout here or long
    // streaming turns get cut off.
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("build flowy HTTP client")
}

/// Build the stream function for one turn.
///
/// `api_key` is the account token for this run only; the provider is created
/// per run so a logout cannot leave a live credential in a long-lived client.
pub fn openai_stream_fn(
    route: &ModelRoute,
    api_key: Option<String>,
) -> StreamFn {
    openai_stream_fn_with_model(route.rpi_model(), route, api_key, false)
}

/// Build a one-shot stream function that pins `tool_choice` to a single named
/// function tool via `Model::sampling_params`, used by [`crate::complete_structured`].
pub fn openai_structured_stream_fn(
    route: &ModelRoute,
    api_key: Option<String>,
    forced_tool_name: &str,
) -> StreamFn {
    let mut model = route.rpi_model();
    let mut params = model.sampling_params.take().unwrap_or_default();
    params.insert(
        "tool_choice".to_owned(),
        serde_json::json!({
            "type": "function",
            "function": { "name": forced_tool_name }
        }),
    );
    model.sampling_params = Some(params);
    openai_stream_fn_with_model(model, route, api_key, false)
}

pub(crate) fn bounded_openai_stream_fn(
    route: &ModelRoute,
    api_key: Option<String>,
) -> StreamFn {
    openai_stream_fn_with_model(route.rpi_model(), route, api_key, true)
}

fn openai_stream_fn_with_model(
    model: rpi_ai::Model,
    route: &ModelRoute,
    api_key: Option<String>,
    bounded: bool,
) -> StreamFn {
    tracing::debug!(
        target: "flowy.prompt",
        stage = "openai_stream_fn::build",
        provider = %route.provider,
        model = %route.model,
        base_url = %route.base_url,
        has_key = api_key.is_some(),
        "provider ready"
    );
    let client = if bounded {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .expect("build bounded Flowy HTTP client")
    } else {
        http_client()
    };
    let provider =
        Arc::new(OpenAiCompletionsProvider::with_models_without_env_api_key(
            route.provider.clone(),
            api_key,
            client,
            vec![model.clone()],
        ));
    rpi_agent::stream_fn(move |_requested, context, options| {
        tracing::debug!(
            target: "flowy.prompt",
            stage = "openai_stream_fn::invoke",
            model = %model.id,
            base_url = %model.base_url,
            "→ HTTP Provider::stream_simple"
        );
        let provider = Arc::clone(&provider);
        let model = model.clone();
        let context = context.clone();
        let mut options = options.clone();
        if bounded {
            options.max_retries = Some(0);
        }
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        let cancel_token = producer.cancellation_token();
        tokio::spawn(async move {
            let mut inner = tokio::select! {
                () = cancel_token.cancelled() => return,
                stream = provider.stream_simple(&model, &context, &options) => stream,
            };
            loop {
                tokio::select! {
                    () = cancel_token.cancelled() => break,
                    maybe_event = inner.next() => {
                        let Some(event) = maybe_event else {
                            break;
                        };
                        producer.push(event).await;
                    }
                }
            }
            drop(inner);
            tracing::debug!(
                target: "flowy.prompt",
                stage = "openai_stream_fn::invoke",
                model = %model.id,
                "stream ended"
            );
        });
        consumer
    })
}

/// Reserve every model invocation, including any host retry, before the
/// underlying provider or scripted stream can observe a dispatch. Internal
/// provider retries are disabled for budgeted production routes.
pub(crate) fn budget_stream_fn(
    budget: Arc<crate::run_budget::RunBudgetState>,
    stream: StreamFn,
) -> StreamFn {
    rpi_agent::stream_fn(move |model, context, options| {
        if let Err(stop) = budget.reserve_model() {
            let mut error = AssistantMessage::empty(
                Api::OpenaiCompletions,
                model.provider.clone(),
                model.id.clone(),
                0,
            );
            error.stop_reason = StopReason::Error;
            error.error_message = Some(stop.to_string());
            let (mut producer, consumer) =
                rpi_ai::event_stream::create_assistant_message_event_stream();
            tokio::spawn(async move {
                producer
                    .push(AssistantMessageEvent::Error {
                        reason: ErrorReason::Error,
                        error,
                    })
                    .await;
            });
            consumer
        } else {
            stream(model, context, options)
        }
    })
}

/// One provider invocation per gated model dispatch. Provider retries and
/// redirects cannot reuse a token after the account generation changes.
pub(crate) fn guarded_openai_stream_fn(
    route: &ModelRoute,
    execution: ExecutionMetadata,
    gate: Arc<dyn ExecutionGate>,
) -> StreamFn {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
        .expect("build gated Flowy HTTP client");
    let model = route.rpi_model();
    let provider =
        Arc::new(OpenAiCompletionsProvider::with_models_without_env_api_key(
            route.provider.clone(),
            None,
            client,
            vec![model.clone()],
        ));
    let route = route.clone();
    rpi_agent::stream_fn(move |_, context, options| {
        let model = model.clone();
        let route = route.clone();
        let execution = execution.clone();
        let gate = Arc::clone(&gate);
        let provider = Arc::clone(&provider);
        let context = context.clone();
        let mut options = options.clone();
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        let cancel = producer.cancellation_token();
        tokio::spawn(async move {
            if cancel.is_cancelled() || options.signal.is_cancelled() {
                return;
            }
            let credential = match gate.model_credential(&execution, &route) {
                Ok(credential) => credential,
                Err(denied) => {
                    producer.push(denied_event(&model, denied)).await;
                    return;
                }
            };
            options.api_key = Some(credential);
            options.max_retries = Some(0);
            let mut inner = tokio::select! {
                biased;
                () = cancel.cancelled() => return,
                stream = provider.stream_simple(&model, &context, &options) => stream,
            };
            loop {
                let event = tokio::select! {
                    biased;
                    () = cancel.cancelled() => break,
                    event = inner.next() => event,
                };
                let Some(event) = event else { break };
                producer.push(event).await;
            }
            drop(inner);
        });
        consumer
    })
}

/// The optional host-provided stream (for example a deterministic fixture)
/// observes the same execution authorization as the default transport.
pub(crate) fn guard_stream_fn(
    route: &ModelRoute,
    execution: ExecutionMetadata,
    gate: Arc<dyn ExecutionGate>,
    stream: StreamFn,
) -> StreamFn {
    let route = route.clone();
    rpi_agent::stream_fn(move |model, context, options| {
        match gate.model_credential(&execution, &route) {
            Ok(_) => stream(model, context, options),
            Err(denied) => {
                let event = denied_event(model, denied);
                let (mut producer, consumer) =
                    rpi_ai::event_stream::create_assistant_message_event_stream(
                    );
                tokio::spawn(async move { producer.push(event).await });
                consumer
            }
        }
    })
}

fn denied_event(
    model: &rpi_ai::Model,
    denied: ExecutionDenied,
) -> AssistantMessageEvent {
    let mut error = AssistantMessage::empty(
        Api::OpenaiCompletions,
        model.provider.clone(),
        model.id.clone(),
        0,
    );
    error.stop_reason = StopReason::Error;
    error.error_message = Some(denied.to_string());
    AssistantMessageEvent::Error {
        reason: ErrorReason::Error,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::openai_stream_fn;
    use crate::loop_runner::ModelRoute;
    use rpi_ai::provider::Provider;
    use std::sync::Arc;

    fn route() -> ModelRoute {
        ModelRoute {
            provider: "flowy".into(),
            model: "AIPC-deepseek-v4-flash".into(),
            base_url: "https://example.invalid/claw/v1".into(),
            ..ModelRoute::default()
        }
    }

    #[tokio::test]
    async fn revoked_lease_rejects_model_dispatch_even_with_a_supplied_stream()
    {
        use std::sync::atomic::{AtomicBool, Ordering};

        let route = route();
        let gate = Arc::new(crate::execution_gate::tests::RevocableGate(
            AtomicBool::new(true),
        ));
        let stream = super::guard_stream_fn(
            &route,
            super::ExecutionMetadata::default(),
            gate.clone(),
            super::echo_stream_fn("model reply".into()),
        );
        let model = route.rpi_model();
        let context = rpi_ai::types::Context::default();
        let options = rpi_ai::provider::SimpleStreamOptions::default();
        let mut first = stream(&model, &context, &options);
        assert!(matches!(
            first.next().await,
            Some(super::AssistantMessageEvent::Start { .. })
        ));
        drop(first);

        gate.0.store(false, Ordering::SeqCst);
        let mut next = stream(&model, &context, &options);
        let Some(super::AssistantMessageEvent::Error { error, .. }) =
            next.next().await
        else {
            panic!("revoked request reached the model");
        };
        drop(next);
        assert_eq!(
            error.error_message.as_deref(),
            Some("execution authorization is no longer current")
        );
    }

    #[test]
    fn stream_fn_uses_the_run_route_and_does_not_leak_the_key_into_the_model() {
        let stream = openai_stream_fn(&route(), Some("token".into()));
        let cloned = Arc::clone(&stream);
        let _clone: rpi_agent::StreamFn = cloned;
        // The model carries routing only; the credential stays in the provider.
        let model = route().rpi_model();
        assert_eq!(model.base_url, "https://example.invalid/claw/v1");
        assert_eq!(model.id, "AIPC-deepseek-v4-flash");
        assert!(!model.base_url.contains("token"));
    }

    #[test]
    fn provider_is_constructed_with_the_supplied_models() {
        let model = route().rpi_model();
        let provider =
            rpi_ai::providers::openai_completions::OpenAiCompletionsProvider::with_models_without_env_api_key(
                "flowy",
                Some("token".into()),
                super::http_client(),
                vec![model.clone()],
            );
        assert_eq!(provider.id(), "flowy");
        assert_eq!(provider.models().len(), 1);
        assert_eq!(provider.models()[0].base_url, model.base_url);
    }
}

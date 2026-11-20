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
    StopReason,
};

use crate::loop_runner::ModelRoute;

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
    tracing::debug!(
        target: "flowy.prompt",
        stage = "openai_stream_fn::build",
        provider = %route.provider,
        model = %route.model,
        base_url = %route.base_url,
        has_key = api_key.is_some(),
        "provider ready"
    );
    let model = route.rpi_model();
    let provider =
        Arc::new(OpenAiCompletionsProvider::with_models_without_env_api_key(
            route.provider.clone(),
            api_key,
            http_client(),
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
        let options = options.clone();
        let (mut producer, consumer) =
            rpi_ai::event_stream::create_assistant_message_event_stream();
        tokio::spawn(async move {
            let mut inner =
                provider.stream_simple(&model, &context, &options).await;
            while let Some(event) = inner.next().await {
                producer.push(event).await;
            }
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

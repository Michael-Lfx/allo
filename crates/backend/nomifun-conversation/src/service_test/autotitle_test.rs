use super::*;
use nomifun_ai_agent::conversation_title_completer::ConversationTitleCompleter;

/// Records every `summarize` call and returns `"{prefix}{call_index}"` titles.
/// `empty` mode simulates every candidate failing to produce a usable title.
struct FakeTitleCompleter {
    prefix: String,
    empty: bool,
    calls: Mutex<Vec<String>>,
}

impl FakeTitleCompleter {
    fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_owned(),
            empty: false,
            calls: Mutex::new(vec![]),
        }
    }

    fn empty() -> Self {
        Self {
            prefix: String::new(),
            empty: true,
            calls: Mutex::new(vec![]),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ConversationTitleCompleter for FakeTitleCompleter {
    async fn summarize(
        &self,
        content: &str,
        _candidates: &[ProviderWithModel],
    ) -> Result<String, AppError> {
        let mut calls = self.calls.lock().unwrap();
        let index = calls.len();
        calls.push(content.to_owned());
        if self.empty {
            Ok(String::new())
        } else {
            Ok(format!("{}{index}", self.prefix))
        }
    }
}

/// Seed a conversation row directly (no broadcaster noise) with full control
/// over name and `extra` — the two fields the title state machine touches.
async fn seed_title_conversation(repo: &Arc<MockRepo>, name: &str, extra: serde_json::Value) -> String {
    let row = ConversationRow {
        id: 0,
        conversation_id: ConversationId::new().into_string(),
        user_id: TEST_USER_1.into(),
        name: name.into(),
        r#type: "acp".into(),
        extra: serde_json::to_string(&extra).unwrap(),
        delegation_policy: "automatic".into(),
        execution_model_pool: None,
        decision_policy: "automatic".into(),
        execution_template_id: None,
        model: None,
        status: Some("finished".into()),
        source: Some("nomifun".into()),
        channel_chat_id: None,
        pinned: false,
        pinned_at: None,
        cron_job_id: None,
        preset_id: None,
        preset_revision: None,
        preset_snapshot: None,
        created_at: 1,
        updated_at: 1,
    };
    repo.create(&row).await.unwrap()
}

fn title_message_row(conversation_id: &str, message_id: &str, position: &str, text: &str, created_at: i64) -> MessageRow {
    MessageRow {
        id: 0,
        message_id: message_id.into(),
        conversation_id: conversation_id.into(),
        msg_id: Some(message_id.into()),
        r#type: "text".into(),
        content: serde_json::json!({ "content": text }).to_string(),
        position: Some(position.into()),
        status: Some("finish".into()),
        hidden: false,
        created_at,
    }
}

/// (name, autoTitleState, titleSource) read back from the seeded row.
async fn title_state(repo: &Arc<MockRepo>, conversation_id: &str) -> (String, Option<String>, Option<String>) {
    let row = repo.get(conversation_id).await.unwrap().unwrap();
    let extra: serde_json::Value = serde_json::from_str(&row.extra).unwrap();
    (
        row.name,
        extra
            .get("autoTitleState")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        extra
            .get("titleSource")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
    )
}

fn title_session_model() -> Option<ProviderWithModel> {
    Some(pwm(PROVIDER_ID_1, "m1"))
}

#[tokio::test]
async fn first_turn_titles_and_broadcasts() {
    let (svc, broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "New conversation", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();
    repo.insert_message(&title_message_row(&conversation_id, "assistant-1", "left", "assistant reply", 20))
        .await
        .unwrap();
    broadcaster.take_events();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;

    let (name, state, source) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "title-0");
    assert_eq!(state.as_deref(), Some("done"));
    assert_eq!(source, None);
    assert_eq!(completer.calls().len(), 1);
    let events = broadcaster.take_events();
    assert!(
        events
            .iter()
            .any(|event| event.data["action"] == "updated" && event.data["conversation_id"] == conversation_id),
        "the single title pass must broadcast conversation.listChanged after applying the title"
    );
}

#[tokio::test]
async fn user_rename_blocks_title_and_preview() {
    let (svc, _broadcaster, repo, runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "preview", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();

    // A rename without an explicit name_source is a user rename (conservative).
    let req: UpdateConversationRequest = serde_json::from_value(json!({ "name": "My title" })).unwrap();
    svc.update(TEST_USER_1, &conversation_id, req, &runtime_registry)
        .await
        .unwrap();
    let (name, _, source) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "My title");
    assert_eq!(source.as_deref(), Some("user"));

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;

    let (name, state, _) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "My title", "a user rename is never overwritten by a title pass");
    assert_eq!(state, None, "no auto-title state may be written after a user rename");
    assert_eq!(completer.calls().len(), 0, "the title pass must exit before any model call");

    // A late auto preview must not overwrite the user's name either.
    let preview: UpdateConversationRequest =
        serde_json::from_value(json!({ "name": "late preview", "name_source": "auto" })).unwrap();
    svc.update(TEST_USER_1, &conversation_id, preview, &runtime_registry)
        .await
        .unwrap();
    let (name, _, _) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "My title");
}

#[tokio::test]
async fn preview_applies_before_model_title_and_blocked_after() {
    let (svc, _broadcaster, repo, runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "New conversation", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();

    // Preview lands first: no title state yet, so it is applied.
    let preview: UpdateConversationRequest =
        serde_json::from_value(json!({ "name": "hello title…", "name_source": "auto" })).unwrap();
    let updated = svc
        .update(TEST_USER_1, &conversation_id, preview, &runtime_registry)
        .await
        .unwrap();
    assert_eq!(updated.name, "hello title…");

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;
    let (name, state, _) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "title-0");
    assert_eq!(state.as_deref(), Some("done"));

    // A late (would-be second) preview must not overwrite the model title.
    let late_preview: UpdateConversationRequest =
        serde_json::from_value(json!({ "name": "late preview", "name_source": "auto" })).unwrap();
    svc.update(TEST_USER_1, &conversation_id, late_preview, &runtime_registry)
        .await
        .unwrap();
    let (name, state, _) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "title-0");
    assert_eq!(state.as_deref(), Some("done"));
}

#[tokio::test]
async fn first_turn_title_includes_assistant_reply() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "preview", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();
    repo.insert_message(&title_message_row(&conversation_id, "assistant-1", "left", "assistant reply", 20))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;

    let (name, state, _) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "title-0", "the single pass produces the model title");
    assert_eq!(state.as_deref(), Some("done"));
    let calls = completer.calls();
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].contains("Assistant: assistant reply"),
        "the title input must include the first assistant reply, got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn no_assistant_text_still_titles_from_user_input() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "preview", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();

    // No assistant reply stored: the pass still titles from the user message alone.
    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;

    let (name, state, _) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "title-0");
    assert_eq!(state.as_deref(), Some("done"));
    let calls = completer.calls();
    assert_eq!(calls.len(), 1, "with no assistant text the pass still calls the model once");
    assert!(
        !calls[0].contains("Assistant:"),
        "the input must be user-only when there is no assistant reply, got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn all_candidates_fail_marks_failed_and_keeps_preview() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::empty());
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "preview", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;

    let (name, state, _) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "preview", "terminal failure keeps the preview name");
    assert_eq!(state.as_deref(), Some("failed"));
    assert_eq!(completer.calls().len(), 1);
}

#[tokio::test]
async fn legacy_conversation_is_not_renamed() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    // A conversation that already had turns before this feature shipped: its
    // first right message is an old one, not the message just sent.
    let conversation_id = seed_title_conversation(&repo, "Existing", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, "old-first", "right", "old request", 10))
        .await
        .unwrap();
    repo.insert_message(&title_message_row(&conversation_id, "assistant-1", "left", "old reply", 20))
        .await
        .unwrap();
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "new request", 30))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "new request".to_owned(), title_session_model())
        .await;

    let (name, state, _) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "Existing");
    assert_eq!(state, None);
    assert_eq!(completer.calls().len(), 0);
}

use super::*;
use nomifun_ai_agent::conversation_title_completer::{ConversationTitleCompleter, ConversationTitleResult};

/// Records every `summarize` call and returns `"{prefix}{call_index}"` titles.
/// `empty` mode simulates a title provider that produces no usable result.
struct FakeTitleCompleter {
    prefix: String,
    empty: bool,
    echo_input: bool,
    calls: Mutex<Vec<String>>,
}

impl FakeTitleCompleter {
    fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_owned(),
            empty: false,
            echo_input: false,
            calls: Mutex::new(vec![]),
        }
    }

    fn empty() -> Self {
        Self {
            prefix: String::new(),
            empty: true,
            echo_input: false,
            calls: Mutex::new(vec![]),
        }
    }

    fn echo_input() -> Self {
        Self {
            prefix: String::new(),
            empty: false,
            echo_input: true,
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
    ) -> Result<ConversationTitleResult, AppError> {
        let mut calls = self.calls.lock().unwrap();
        let index = calls.len();
        calls.push(content.to_owned());
        if self.empty {
            Ok(ConversationTitleResult {
                title: String::new(),
                llm_call_count: 1,
                response_channel: None,
                response_chars: 0,
            })
        } else {
            let title = if self.echo_input {
                content.to_owned()
            } else {
                format!("{}{index}", self.prefix)
            };
            let response_chars = title.chars().count();
            Ok(ConversationTitleResult {
                title,
                llm_call_count: 1,
                response_channel: None,
                response_chars,
            })
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

/// `(name, autoTitleState, titleSource)` read back from the seeded row.
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
async fn first_turn_titles_from_user_message_and_broadcasts() {
    let (svc, broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "hello title", json!({})).await;
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
    assert_eq!(source.as_deref(), Some("auto"));
    assert_eq!(completer.calls(), vec!["hello title"]);
    let events = broadcaster.take_events();
    assert!(
        events
            .iter()
            .any(|event| event.data["action"] == "updated" && event.data["conversation_id"] == conversation_id),
        "the single title pass must broadcast conversation.listChanged after applying the title"
    );
}

#[tokio::test]
async fn user_rename_blocks_title() {
    let (svc, _broadcaster, repo, runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "hello title", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();

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
}

#[tokio::test]
async fn explicitly_named_conversation_is_not_auto_titled() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "SSH session", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;

    let (name, state, source) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "SSH session");
    assert_eq!(state, None);
    assert_eq!(source, None);
    assert!(completer.calls().is_empty());
}

#[tokio::test]
async fn title_input_contains_only_first_user_message() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "hello title", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();
    repo.insert_message(&title_message_row(&conversation_id, "assistant-1", "left", "assistant reply", 20))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;

    let calls = completer.calls();
    assert_eq!(calls, vec!["hello title"]);
}

#[tokio::test]
async fn provisional_title_is_cleaned_and_kept_on_failure() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::empty());
    svc.with_title_completer(completer.clone());
    let input = "<think>internal reasoning</think>\n## 修复会话标题生成中的重复调用以及并发覆盖问题";
    let conversation_id = seed_title_conversation(&repo, input, json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", input, 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, input.to_owned(), title_session_model())
        .await;

    let (name, state, source) = title_state(&repo, &conversation_id).await;
    assert!(!name.contains("think"));
    assert!(!name.contains("#"));
    assert!(name.starts_with("修复会话标题生成"));
    assert!(name.chars().count() <= 24);
    assert_eq!(state.as_deref(), Some("failed"));
    assert_eq!(source.as_deref(), Some("auto"));
    assert_eq!(completer.calls(), vec![input]);
}

#[tokio::test]
async fn provisional_title_keeps_emoji_and_collapses_whitespace() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::empty());
    svc.with_title_completer(completer);
    let input = "\n\n* 😀   修复登录问题\n后续内容";
    let conversation_id = seed_title_conversation(&repo, input, json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", input, 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, input.to_owned(), title_session_model())
        .await;

    let (name, state, _) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "😀 修复登录问题");
    assert_eq!(state.as_deref(), Some("failed"));
}

#[tokio::test]
async fn empty_title_result_keeps_message_fallbacks() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::empty());
    svc.with_title_completer(completer.clone());
    let cases = [
        ("41", "41"),
        ("2026 年计划", "2026 年计划"),
        ("123abc", "123abc"),
        ("1. 修复标题问题", "修复标题问题"),
        (".NET 迁移", ".NET 迁移"),
        ("-1 编号", "-1 编号"),
        ("../docs", "../docs"),
        ("`literal`", "`literal`"),
        ("```", "```"),
    ];

    for (index, (input, expected)) in cases.iter().enumerate() {
        let conversation_id = seed_title_conversation(&repo, "", json!({})).await;
        let message_id = format!("title-fallback-{index}");
        repo.insert_message(&title_message_row(&conversation_id, &message_id, "right", input, 10))
            .await
            .unwrap();

        svc.maybe_autotitle(&conversation_id, &message_id, (*input).to_owned(), title_session_model())
            .await;

        let (name, state, source) = title_state(&repo, &conversation_id).await;
        assert_eq!(name, *expected);
        assert_eq!(state.as_deref(), Some("failed"));
        assert_eq!(source.as_deref(), Some("auto"));
    }

    assert_eq!(completer.calls().len(), cases.len());
}

#[tokio::test]
async fn echoed_user_input_keeps_temporary_title_and_fails() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::echo_input());
    svc.with_title_completer(completer.clone());
    let input = "请修复标题生成问题";
    let conversation_id = seed_title_conversation(&repo, "", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", input, 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, input.to_owned(), title_session_model())
        .await;

    let (name, state, source) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, input);
    assert_eq!(state.as_deref(), Some("failed"));
    assert_eq!(source.as_deref(), Some("auto"));
    assert_eq!(completer.calls(), vec![input]);
}

#[tokio::test]
async fn concurrent_title_tasks_have_one_cas_winner() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let input = "并发标题任务";
    let conversation_id = seed_title_conversation(&repo, "", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", input, 10))
        .await
        .unwrap();

    tokio::join!(
        svc.maybe_autotitle(
            &conversation_id,
            MESSAGE_ID_1,
            input.to_owned(),
            title_session_model(),
        ),
        svc.maybe_autotitle(
            &conversation_id,
            MESSAGE_ID_1,
            input.to_owned(),
            title_session_model(),
        )
    );

    let (name, state, source) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "title-0");
    assert_eq!(state.as_deref(), Some("done"));
    assert_eq!(source.as_deref(), Some("auto"));
    assert_eq!(completer.calls().len(), 1);
}

#[tokio::test]
async fn blank_first_message_does_not_start_title() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", " \n\t", 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, " \n\t".to_owned(), title_session_model())
        .await;

    let (name, state, source) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "");
    assert_eq!(state, None);
    assert_eq!(source, None);
    assert!(completer.calls().is_empty());
}

#[tokio::test]
async fn repeated_and_later_messages_do_not_start_another_title() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let conversation_id = seed_title_conversation(&repo, "hello title", json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", "hello title", 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;
    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, "hello title".to_owned(), title_session_model())
        .await;

    repo.insert_message(&title_message_row(&conversation_id, "user-2", "right", "later request", 30))
        .await
        .unwrap();
    svc.maybe_autotitle(&conversation_id, "user-2", "later request".to_owned(), title_session_model())
        .await;

    assert_eq!(completer.calls(), vec!["hello title"]);
}

#[tokio::test]
async fn missing_title_model_marks_failed_without_calling_completer() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let input = "hello title";
    let conversation_id = seed_title_conversation(&repo, input, json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", input, 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, input.to_owned(), None)
        .await;

    let (name, state, source) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, input);
    assert_eq!(state.as_deref(), Some("failed"));
    assert_eq!(source.as_deref(), Some("auto"));
    assert!(completer.calls().is_empty());
}

#[tokio::test]
async fn title_input_is_limited_to_500_unicode_characters() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::new("title-"));
    svc.with_title_completer(completer.clone());
    let input = "用户".repeat(300);
    let conversation_id = seed_title_conversation(&repo, &input, json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", &input, 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, input, title_session_model())
        .await;

    let calls = completer.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].chars().count(), 500);
}

#[tokio::test]
async fn all_candidates_fail_marks_failed_and_keeps_temporary_name() {
    let (svc, _broadcaster, repo, _runtime_registry) = make_service();
    let completer = Arc::new(FakeTitleCompleter::empty());
    svc.with_title_completer(completer.clone());
    let input = "## 修复会话标题生成中的重复调用以及并发覆盖问题";
    let conversation_id = seed_title_conversation(&repo, input, json!({})).await;
    repo.insert_message(&title_message_row(&conversation_id, MESSAGE_ID_1, "right", input, 10))
        .await
        .unwrap();

    svc.maybe_autotitle(&conversation_id, MESSAGE_ID_1, input.to_owned(), title_session_model())
        .await;

    let (name, state, source) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "修复会话标题生成中的重复调用以及并发覆盖问题");
    assert_eq!(state.as_deref(), Some("failed"));
    assert_eq!(source.as_deref(), Some("auto"));
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

    let (name, state, source) = title_state(&repo, &conversation_id).await;
    assert_eq!(name, "Existing");
    assert_eq!(state, None);
    assert_eq!(source, None);
    assert_eq!(completer.calls().len(), 0);
}

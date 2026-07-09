# Tool Call Supersede — Status + Persist Race 修复计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 tool_call 预览被 supersede 后 DB/WS 状态不一致（hidden 但 status 仍为 running），以及 canonical 正式调用比预览落库更早到达时 hide 静默失败导致预览仍可见的竞态。

**Architecture:** 全部改动集中在 `nomifun-conversation` 的 `StreamRelay`：`hide_tool_call_message` 在隐藏时同步写入 `status=finish` 并 patch content JSON 的 `status=completed`；在 `consume()` 事件循环内维护 `pending_superseded_call_ids: HashSet<String>`，hide 遇到 `DbError::NotFound` 时记入 pending，`persist_tool_call` 在 insert 路径检查 pending 并直接以 hidden+finish 落库。不改动 provider/sink 层的 supersede 判定逻辑。

**Tech Stack:** Rust（`nomifun-conversation`、`nomifun-db`、`nomifun-ai-agent` protocol types），`cargo test -p nomifun-conversation`。

## 背景（工程师需知）

DeepSeek 等 provider 会先通过 text channel 发出 client-generated `call_{uuid}` 预览，再发出正式 API `019*` tool_call。已有 supersede 逻辑（`should_supersede_preview` + `hide_tool_call_message`）会在 canonical `Running` 到达时隐藏预览。

**Bug #1（本计划 Task 1–2）：** `hide_tool_call_message` 目前只设 `hidden=true`，不改 `messages.status`（仍为 `work`）也不改 content JSON 里的 `status`（仍为 `running`）。UI 虽跳过 hidden 消息，但 DB 导出/调试会看到 32 条「进行中」幽灵记录（会话 23 实测）。

**Bug #2（本计划 Task 3–5）：** `hide_tool_call_message` 在 `update_message` 返回 `DbError::NotFound`（行尚未 insert）时只打 debug 日志。正式 `Running` 若比预览 insert 更早完成 hide，预览稍后 insert 会以 `hidden=false, status=work` 落库并短暂/永久可见。

## File Structure

| 文件 | 职责 |
|------|------|
| `crates/backend/nomifun-conversation/src/stream_relay.rs` | `consume()` 循环、`hide_tool_call_message`、`persist_tool_call`、supersede 逻辑、集成测试 |
| `crates/backend/nomifun-db/src/repository/sqlite_conversation.rs` | `update_message` 在 0 rows 时返回 `NotFound`（只读参考，不改） |
| `crates/backend/nomifun-ai-agent/src/protocol/events/tool_call.rs` | `ToolCallStatus` 枚举（`running`/`completed`/`error`） |

**不改：** `backend_output_sink.rs`、`openai.rs`、前端 UI（hidden 消息已被 `MessageList` 跳过）。

## Global Constraints

- **测试命令：** `cargo test -p nomifun-conversation`；聚焦时用 `cargo test -p nomifun-conversation run_supersedes_preview` 或新测试名。
- **Rust import：** `use std::collections::{HashMap, HashSet};`（在现有 `HashMap` import 行扩展）。
- **DbError 匹配：** 测试模块已 `use nomifun_db::DbError;`；生产代码用 `matches!(e, DbError::NotFound(_))` 或 `if let Err(DbError::NotFound(_))`.
- **逐任务提交**，commit message 聚焦 why。

---

### Task 1: 为 supersede hide 增加 content/status 补丁 helper

**Files:**
- Modify: `crates/backend/nomifun-conversation/src/stream_relay.rs`（`impl StreamRelay` 内，`merge_json_content` 附近）
- Test: 同文件 `#[cfg(test)] mod tests`

- [ ] **Step 1: 写失败测试**

在 `stream_relay.rs` 测试模块内（`run_supersedes_preview_tool_call_when_canonical_browser_call_arrives` 之前）新增：

```rust
#[test]
fn superseded_preview_content_sets_completed_status() {
    let preview_json = serde_json::json!({
        "call_id": "nomi-call_call_preview",
        "name": "Browser",
        "args": {"url": "https://example.com"},
        "status": "running"
    })
    .to_string();

    let patched = StreamRelay::superseded_preview_content(&preview_json);
    let value: serde_json::Value = serde_json::from_str(&patched).unwrap();
    assert_eq!(value["status"], "completed");
    assert_eq!(value["call_id"], "nomi-call_call_preview");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p nomifun-conversation superseded_preview_content_sets_completed_status -- --nocapture`

Expected: FAIL — `superseded_preview_content` not found / associated function not found

- [ ] **Step 3: 实现 helper**

在 `impl StreamRelay` 中、`merge_json_content` 之前添加：

```rust
/// Patch stored tool_call JSON so superseded previews show terminal status in content.
fn superseded_preview_content(existing_json: &str) -> String {
    use nomifun_ai_agent::protocol::events::tool_call::{ToolCallEventData, ToolCallStatus};

    match serde_json::from_str::<ToolCallEventData>(existing_json) {
        Ok(mut data) => {
            data.status = ToolCallStatus::Completed;
            serde_json::to_string(&data).unwrap_or_else(|_| existing_json.to_owned())
        }
        Err(_) => existing_json.to_owned(),
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p nomifun-conversation superseded_preview_content_sets_completed_status -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/backend/nomifun-conversation/src/stream_relay.rs
git commit -m "fix(conversation): add superseded preview content patch helper"
```

---

### Task 2: hide_tool_call_message 同步更新 status 与 content

**Files:**
- Modify: `crates/backend/nomifun-conversation/src/stream_relay.rs:1175-1200`
- Test: 同文件 `run_supersedes_preview_tool_call_when_canonical_browser_call_arrives`

- [ ] **Step 1: 加强现有集成测试断言（先写失败测试）**

在 `run_supersedes_preview_tool_call_when_canonical_browser_call_arrives` 的 `take_updates()` 断言块中，在 hidden 断言之后追加：

```rust
assert!(
    updates.iter().any(|(id, update)| {
        id == "nomi-call_call_fbb31e380c974b268f4561c1"
            && update.hidden == Some(true)
            && update
                .status
                .as_ref()
                .and_then(|s| s.as_deref())
                == Some("finish")
    }),
    "superseded preview update must set hidden=true and status=finish"
);
assert!(
    updates.iter().any(|(id, update)| {
        id == "nomi-call_call_fbb31e380c974b268f4561c1"
            && update.content.as_ref().is_some_and(|c| c.contains("\"status\":\"completed\""))
    }),
    "superseded preview update must patch content status to completed"
);
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p nomifun-conversation run_supersedes_preview_tool_call_when_canonical_browser_call_arrives -- --nocapture`

Expected: FAIL — status 断言失败（当前 update 只有 `hidden: Some(true)`）

- [ ] **Step 3: 实现 hide_tool_call_message 完整更新**

将 `hide_tool_call_message` 替换为（签名暂不加 pending，Task 3 会扩展）：

```rust
async fn hide_tool_call_message(&self, call_id: &str) {
    let content = self
        .repo
        .get_message_by_msg_id(self.conv_id(), call_id, "tool_call")
        .await
        .ok()
        .flatten()
        .map(|row| Self::superseded_preview_content(&row.content));

    let update = nomifun_db::MessageRowUpdate {
        content,
        status: Some(Some("finish".to_owned())),
        hidden: Some(true),
    };
    match self.repo.update_message(call_id, &update).await {
        Ok(()) => {
            debug!(call_id, "Hidden superseded tool_call preview message");
            self.broadcast_stream_payload(json!({
                "conversation_id": self.conv_id(),
                "msg_id": call_id,
                "type": "tool_call",
                "data": {
                    "call_id": call_id,
                    "status": "completed",
                    "superseded": true,
                },
                "status": "finish",
                "hidden": true,
                "replace": true,
            }));
        }
        Err(e) => {
            debug!(
                call_id,
                error = %ErrorChain(&e),
                "Could not hide superseded tool_call preview (may not have been persisted yet)"
            );
        }
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p nomifun-conversation run_supersedes_preview_tool_call_when_canonical_browser_call_arrives -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/backend/nomifun-conversation/src/stream_relay.rs
git commit -m "fix(conversation): mark superseded tool previews finished in DB and WS"
```

---

### Task 3: 引入 pending_superseded_call_ids 并接线

**Files:**
- Modify: `crates/backend/nomifun-conversation/src/stream_relay.rs`（import、`consume()`、`supersede_orphan_preview_tool_calls`、`hide_tool_call_message`、`persist_tool_call` 签名）

- [ ] **Step 1: 扩展 import 与 consume 局部状态**

文件顶部：

```rust
use std::collections::{HashMap, HashSet};
```

`consume()` 内 `active_tool_calls` 声明后追加：

```rust
let mut pending_superseded_call_ids: HashSet<String> = HashSet::new();
```

- [ ] **Step 2: 更新函数签名与调用点**

`supersede_orphan_preview_tool_calls` 签名改为：

```rust
async fn supersede_orphan_preview_tool_calls(
    &self,
    active_tool_calls: &mut HashMap<String, ToolCallEventData>,
    canonical: &ToolCallEventData,
    pending_superseded_call_ids: &mut HashSet<String>,
)
```

循环内调用改为：

```rust
self.hide_tool_call_message(&call_id, pending_superseded_call_ids).await;
```

`hide_tool_call_message` 签名改为：

```rust
async fn hide_tool_call_message(
    &self,
    call_id: &str,
    pending_superseded_call_ids: &mut HashSet<String>,
)
```

`persist_tool_call` 签名改为：

```rust
async fn persist_tool_call(
    &self,
    data: &nomifun_ai_agent::protocol::events::tool_call::ToolCallEventData,
    pending_superseded_call_ids: &mut HashSet<String>,
)
```

`AgentStreamEvent::ToolCall` 分支：

```rust
self.supersede_orphan_preview_tool_calls(
    &mut active_tool_calls,
    &data,
    &mut pending_superseded_call_ids,
)
.await;
// ...
self.persist_tool_call(data, &mut pending_superseded_call_ids).await;
```

`fail_active_tool_calls` 内：

```rust
self.persist_tool_call(data, pending_superseded_call_ids).await;
```

`fail_active_tool_calls` 签名增加参数 `pending_superseded_call_ids: &mut HashSet<String>`，并在 `consume()` 所有调用处传入 `&mut pending_superseded_call_ids`。

- [ ] **Step 3: hide 失败时写入 pending**

在 `hide_tool_call_message` 的 `Err` 分支，将现有 debug 日志替换为：

```rust
Err(e) => {
    if matches!(e, nomifun_db::DbError::NotFound(_)) {
        pending_superseded_call_ids.insert(call_id.to_owned());
        debug!(
            call_id,
            "Queued superseded tool_call preview for hidden insert (row not persisted yet)"
        );
    } else {
        debug!(
            call_id,
            error = %ErrorChain(&e),
            "Could not hide superseded tool_call preview"
        );
    }
}
```

成功分支在 broadcast 前追加：`pending_superseded_call_ids.remove(call_id);`

- [ ] **Step 4: 编译验证**

Run: `cargo check -p nomifun-conversation`

Expected: 编译通过（persist 逻辑 Task 4 才完整，此处仅接线）

- [ ] **Step 5: Commit**

```bash
git add crates/backend/nomifun-conversation/src/stream_relay.rs
git commit -m "refactor(conversation): wire pending superseded call ids through stream relay"
```

---

### Task 4: persist_tool_call insert 路径 honor pending

**Files:**
- Modify: `crates/backend/nomifun-conversation/src/stream_relay.rs:1062-1136`
- Test: 同文件测试模块（StrictRecordingRepo + 新单元测试）

- [ ] **Step 1: 写失败测试 — StrictRecordingRepo + pending insert**

在 `RecordingRepo` 定义之前添加：

```rust
/// Like RecordingRepo but returns NotFound on update when the row was never inserted.
struct StrictRecordingRepo {
    inner: RecordingRepo,
}

impl StrictRecordingRepo {
    fn new() -> Self {
        Self {
            inner: RecordingRepo::new(),
        }
    }

    fn take_inserts(&self) -> Vec<MessageRow> {
        self.inner.take_inserts()
    }
}

#[async_trait::async_trait]
impl IConversationRepository for StrictRecordingRepo {
    async fn get(&self, id: i64) -> Result<Option<nomifun_db::models::ConversationRow>, DbError> {
        self.inner.get(id).await
    }
    async fn create(&self, row: &nomifun_db::models::ConversationRow) -> Result<i64, DbError> {
        self.inner.create(row).await
    }
    async fn update(&self, id: i64, updates: &nomifun_db::ConversationRowUpdate) -> Result<(), DbError> {
        self.inner.update(id, updates).await
    }
    async fn delete(&self, id: i64) -> Result<(), DbError> {
        self.inner.delete(id).await
    }
    async fn list_paginated(
        &self,
        user_id: &str,
        filters: &nomifun_db::ConversationFilters,
    ) -> Result<nomifun_common::PaginatedResult<nomifun_db::models::ConversationRow>, DbError> {
        self.inner.list_paginated(user_id, filters).await
    }
    async fn find_by_source_and_chat(
        &self,
        user_id: &str,
        source: &str,
        chat_id: &str,
        agent_type: &str,
    ) -> Result<Option<nomifun_db::models::ConversationRow>, DbError> {
        self.inner.find_by_source_and_chat(user_id, source, chat_id, agent_type).await
    }
    async fn list_by_cron_job(
        &self,
        user_id: &str,
        cron_job_id: &str,
    ) -> Result<Vec<nomifun_db::models::ConversationRow>, DbError> {
        self.inner.list_by_cron_job(user_id, cron_job_id).await
    }
    async fn list_associated(
        &self,
        user_id: &str,
        conversation_id: i64,
    ) -> Result<Vec<nomifun_db::models::ConversationRow>, DbError> {
        self.inner.list_associated(user_id, conversation_id).await
    }
    async fn get_messages(
        &self,
        conv_id: i64,
        page: u32,
        page_size: u32,
        order: nomifun_db::SortOrder,
    ) -> Result<nomifun_common::PaginatedResult<MessageRow>, DbError> {
        self.inner.get_messages(conv_id, page, page_size, order).await
    }
    async fn insert_message(&self, row: &MessageRow) -> Result<(), DbError> {
        self.inner.insert_message(row).await
    }
    async fn update_message(&self, id: &str, updates: &nomifun_db::MessageRowUpdate) -> Result<(), DbError> {
        let exists = self
            .inner
            .inserts
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.id == id);
        if !exists {
            return Err(DbError::NotFound(format!("Message '{id}' not found")));
        }
        self.inner.update_message(id, updates).await
    }
    async fn delete_messages_by_conversation(&self, conv_id: i64) -> Result<(), DbError> {
        self.inner.delete_messages_by_conversation(conv_id).await
    }
    async fn get_message_by_msg_id(
        &self,
        conv_id: i64,
        msg_id: &str,
        msg_type: &str,
    ) -> Result<Option<MessageRow>, DbError> {
        self.inner.get_message_by_msg_id(conv_id, msg_id, msg_type).await
    }
    async fn search_messages(
        &self,
        user_id: &str,
        keyword: &str,
        page: u32,
        page_size: u32,
    ) -> Result<nomifun_common::PaginatedResult<nomifun_db::MessageSearchRow>, DbError> {
        self.inner.search_messages(user_id, keyword, page, page_size).await
    }
}
```

> **Note:** `RecordingRepo.inserts` 需改为 `pub(crate)` 或在 `RecordingRepo` 上添加 `fn has_message_id(&self, id: &str) -> bool` 供 StrictRecordingRepo 使用。推荐在 `RecordingRepo` 添加：

```rust
fn has_message_id(&self, id: &str) -> bool {
    self.inserts.lock().unwrap().iter().any(|m| m.id == id)
}
```

然后 StrictRecordingRepo 用 `self.inner.has_message_id(id)`。

新增测试（需 `#[cfg(test)]` 测试 helper，见 Step 3）：

```rust
#[tokio::test]
async fn persist_tool_call_inserts_hidden_when_pending_superseded() {
    use nomifun_ai_agent::protocol::events::tool_call::{ToolCallEventData, ToolCallStatus};

    let repo = Arc::new(StrictRecordingRepo::new());
    let bus = Arc::new(nomifun_realtime::BroadcastEventBus::new(64));
    let relay = StreamRelay::new(
        "1".into(),
        "asst-1".into(),
        "user-1".into(),
        repo.clone(),
        bus,
        None,
    );

    let preview = ToolCallEventData {
        call_id: "nomi-call_call_race_preview".into(),
        name: "Browser".into(),
        args: serde_json::json!({"url": "https://example.com"}),
        status: ToolCallStatus::Running,
        description: None,
        input: None,
        output: None,
    };

    let mut pending = HashSet::from(["nomi-call_call_race_preview".to_string()]);
    relay
        .test_hide_tool_call_message("nomi-call_call_race_preview", &mut pending)
        .await;
    assert!(pending.contains("nomi-call_call_race_preview"));

    relay.test_persist_tool_call(&preview, &mut pending).await;
    assert!(!pending.contains("nomi-call_call_race_preview"));

    let inserts = repo.take_inserts();
    assert_eq!(inserts.len(), 1);
    assert!(inserts[0].hidden);
    assert_eq!(inserts[0].status.as_deref(), Some("finish"));
    let content: ToolCallEventData = serde_json::from_str(&inserts[0].content).unwrap();
    assert_eq!(content.status, ToolCallStatus::Completed);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p nomifun-conversation persist_tool_call_inserts_hidden_when_pending_superseded -- --nocapture`

Expected: FAIL — helper 不存在 / insert 仍 `hidden: false`

- [ ] **Step 3: 添加测试 helper 并实现 persist 逻辑**

在 `#[cfg(test)] impl StreamRelay` 块（若无则新建）：

```rust
#[cfg(test)]
impl StreamRelay {
    async fn test_hide_tool_call_message(
        &self,
        call_id: &str,
        pending_superseded_call_ids: &mut HashSet<String>,
    ) {
        self.hide_tool_call_message(call_id, pending_superseded_call_ids).await;
    }

    async fn test_persist_tool_call(
        &self,
        data: &nomifun_ai_agent::protocol::events::tool_call::ToolCallEventData,
        pending_superseded_call_ids: &mut HashSet<String>,
    ) {
        self.persist_tool_call(data, pending_superseded_call_ids).await;
    }
}
```

修改 `persist_tool_call` 的 insert 分支（`else` 块）：

```rust
} else {
    let superseded = pending_superseded_call_ids.remove(&data.call_id);
    let row_status = if superseded { "finish" } else { status };
    let mut persisted = data.clone();
    if superseded {
        persisted.status = ToolCallStatus::Completed;
    }
    let content = serde_json::to_string(&persisted).unwrap_or_default();

    let row = MessageRow {
        id: data.call_id.clone(),
        conversation_id: self.conv_id(),
        msg_id: Some(data.call_id.clone()),
        r#type: "tool_call".into(),
        content,
        position: Some("left".into()),
        status: Some(row_status.to_owned()),
        hidden: superseded,
        created_at: now_ms(),
    };
    // ... insert + debug/error unchanged
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p nomifun-conversation persist_tool_call_inserts_hidden_when_pending_superseded -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/backend/nomifun-conversation/src/stream_relay.rs
git commit -m "fix(conversation): insert superseded previews as hidden when hide raced persist"
```

---

### Task 5: 全量回归验证

**Files:**
- Test: `crates/backend/nomifun-conversation/src/stream_relay.rs`

> **说明：** consume 事件循环是严格顺序 + `await persist`，无法在集成层可靠复现「hide 先于 insert」时序；Task 4 的 `StrictRecordingRepo` 单元测试直接调用 `test_hide` → `test_persist` 覆盖该路径。本 Task 只做回归。

- [ ] **Step 1: 运行 supersede 相关测试**

Run: `cargo test -p nomifun-conversation superseded_preview run_supersedes_preview persist_tool_call_inserts_hidden -- --nocapture`

Expected: 全部 PASS

- [ ] **Step 2: 运行 conversation crate 全量测试**

Run: `cargo test -p nomifun-conversation`

Expected: 全部 PASS

- [ ] **Step 3: Commit（若有测试文件微调）**

```bash
git add crates/backend/nomifun-conversation/src/stream_relay.rs
git commit -m "test(conversation): regression for supersede status and persist race fixes"
```

---

## Self-Review Checklist

| 需求 | 对应 Task |
|------|-----------|
| #1 hide 时更新 DB `status=finish` | Task 2 |
| #1 hide 时 patch content `status=completed` | Task 1–2 |
| #1 WS replace 携带 terminal status | Task 2 |
| #2 hide NotFound → pending | Task 3 |
| #2 persist insert honor pending → hidden+finish | Task 4 |
| 现有 supersede 集成测试仍绿 | Task 2, 5 |
| 不引入 provider/sink 改动 | 全计划 |

**Placeholder scan:** 无 TBD/TODO/「类似 Task N」省略。

**Type consistency:** 全程使用 `pending_superseded_call_ids: &mut HashSet<String>`、`ToolCallStatus::Completed`、row status `"finish"`（与现有 `persist_tool_call` Completed 映射一致）。

---

## 手动验证（可选，实现后）

1. 重跑「北京最近有什么会议嘛」类 DeepSeek Browser 任务。
2. 查 DB：`SELECT id, status, hidden FROM messages WHERE conversation_id = ? AND type = 'tool_call' AND hidden = 1`
3. 期望：hidden 预览的 `status` 均为 `finish`，无 `work`/`running` 幽灵行；无 `end_turn` 误报 tool error。

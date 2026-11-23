# PROGRESS · 本地受理回执纵线

分支：`feat-flowy-local-receipt`　基线：`ba5ace40a`　worktree：`.agents/worktrees/feat-flowy-local-receipt`

---

## ⚠️ 第一轮调研对象错了，已重做

第一轮我在**根 workspace**（Tauri + React + `nomifun-*`）里查受理链路，写了半天的 `allowSendWhileLoading`、`executeSteer`、`NomiSendBox`。

**那些跟本方案无关。** 方案描述的是 `flowy18_wip/`（GPUI + pi-rust）。

详见 `BLOCKED.md` 第 1 条。第一轮那节结论已作废，本文件已重写。

---

## 任务 0 · 基线（2026-11-23）

### 环境

| 项 | 实测 |
| --- | --- |
| 工具链 | bun `1.3.14`、cargo `1.100.0` |
| `flowy18_wip/target/`（worktree 内） | 空 —— 冷构建 |
| 主工作区已有二进制 | `flowy18_wip/target/debug/flowy-app.exe` |
| `node_modules/`（worktree 内） | 无 → `bun install` 后仍需从主工作区补 2 个包 |

### 命令基线

| 命令 | 结果 |
| --- | --- |
| `cargo check -p nomifun-ai-agent`（根 workspace） | 通过，0 error，4m57s —— **但属错对象，留作对照** |
| `bun run typecheck` | 通过，exit 0（补包后） |
| `cargo build -p flowy-app`（`flowy18_wip`） | **进行中，耗时未知** |

### 启动命令的正确形态

```bash
cd flowy18_wip
(./target/debug/flowy-app.exe >/dev/null 2>&1 &)
```

**`flowy-app.exe` 是对的。** 我第一轮说「应为 `Flowy.exe`」是错的，已撤回 —— `Flowy.exe` 属于根 workspace 那个 Tauri 产品。

---

## 受理链路：三层拦截，全部定位

目标文件：`flowy18_wip/crates/flowy-studio/src/composer.rs`、`panel.rs`、`flowy18_wip/crates/flowy-agent/src/handle.rs`。

### 第一层 · UI 直接不给按钮

`crates/flowy-studio/src/composer.rs:221` `composer_submit`：

```rust
// A missing cloud model means no live routing is configured; keep the
// send affordance disabled instead of failing after the click.
let has_draft = has_draft && self.cloud_ready;
if running {
    // 渲染「停止」按钮 —— 根本没有发送按钮
    theme::IconButton::new("chat-stop", ...)
} else {
    theme::IconButton::new("chat-send", ...).disabled(!has_draft)
}
```

**忙时用户连点都点不到。**

### 第二层 · `send_prompt` 拒绝

`crates/flowy-studio/src/panel.rs:934`：

```rust
if self.submission_pending { return; }                       // ① 静默丢
if self.agent.is_initializing() { notice; return; }          // ②
...
if !self.cloud_ready || model.is_none() { notice; return; }  // ③ 云未就绪
let text = ...; if text.is_empty() { return; }               // ④
let id = *self.submission.get_or_insert_with(new_submission_id);  // ⑤ 分配提交 id
```

### 第三层 · agent 层明确拒绝「忙」

`crates/flowy-agent/src/handle.rs:1210` `submit_message`：

```rust
if selection.is_some_and(|selection| {
    self.context_generation() != selection.generation
        || lock(&state.active).as_deref() != selection.active
}) {
    return Err("notice.agent_busy");          // ← ①
}
let id = self.ensure_active_session()?;
if lock(&state.live)
    .get(&id)
    .is_some_and(|session| session.status.is_busy())
{
    return Err("notice.agent_busy");          // ← ②
}
```

**② 是决定性的：`session.status.is_busy()` 直接返回 `notice.agent_busy`。**

### 三层合起来

| 层 | 忙时会发生什么 |
| --- | --- |
| UI (`composer.rs:221`) | 只渲染「停止」，**没有发送按钮** |
| `panel.rs:934` | `submission_pending` 静默丢；`!cloud_ready` 拒绝 |
| `handle.rs:1210` | `session.status.is_busy()` → `notice.agent_busy` |

**要打通「本地受理回执」，三层都要动，且第三层是新逻辑（现在根本没有落盘路径）。**

---

## 这一单的真正形状

审查要的验收：**「A 还在跑慢工具时，我能提交 B、拿到编号、重开客户端还能找到 B」**

对照三层拦截，需要：

1. **UI**：`running` 时同时渲染「停止」和「发送」，不再二选一
2. **`send_prompt`**：忙时不丢、不拒，走一条「受理」分支
3. **agent**：新增一条**纯落盘**路径 —— 写入持久化、返回编号、**不进 loop**；等 turn 空闲后再取出处理

**关键设计问题**：落盘之后的「待处理队列」放哪？`flowy-agent` 里有没有现成的 pending 概念，下一轮要查。

---

## 理解的目标 / 顺序 / 最大风险

1. **目标**：忙时也能受理，拿编号，重开可查
2. **顺序**：先做**落盘 + 编号**（不碰调度）→ 再让 UI 放行 → 最后接「空闲后自动处理」
3. **最大风险**：`handle.rs` 的 `submit_message` 是 hot path，改它要保证不破坏既有的 `agent_busy` 语义（别的地方可能依赖它做防重）
4. **约束**：只在本分支 commit、不推送；不动既有测试断言

---

## 任务 1 · 文档落地 ✅

`docs/architecture/flowy-v0/` 共 11 个文件，已 commit `10fedc0a6`。

---

## 下一步

- [ ] 等 `cargo build -p flowy-app` 完成，实测确认三层拦截
- [ ] 查 `flowy-agent` 有没有现成的 pending / 待处理概念
- [ ] 设计「落盘受理」的最小改动面
- [ ] 端到端脚本（忙时提交 → 拿编号 → 重启后能查到）

---

## 任务 2 · 受理队列（已实现、已测）

### 先记三个环境事实

1. **构建产物在 `build.noindex/debug`，不在 `target/debug`。** 我第一轮看 `target` 只有 4K 以为卡住了，其实货在别处 —— 这个仓库改了 Cargo 输出目录。
2. **主工作区 `flowy18_wip/target` 只有 182M**，复制过来后增量构建 7m44s 完成。
3. **`flowy18_wip` 用 JSON 文件持久化**（`sessions/history.json`，临时文件 + rename 原子写），**没有 SQLite**。
   → 目标里说的 `messages.status='pending'` / `conversation_delivery_receipts` 属于根 workspace 的表，这边不存在。

### 做了什么

新增 `crates/flowy-agent-core/src/acceptance.rs`：

- `AcceptedMessage { id, session_id, text, created_at_ms, taken_at_ms }`
- `AcceptanceQueue::accept()` —— 忙时唯一的一次写：一个文件、一次原子 rename，**不碰 session 状态**
- `pending()` —— 取未处理的，按时间排序
- `take(id)` —— 标记已领取，**两个并发调用者不会跑同一段文本**
- 落盘位置 `root/acceptance/pending.json`，**故意与 `sessions/history.json` 分开** —— 队列在 turn 运行中写，索引在 turn 边界写，共用一个文件就是白送的写竞争

### 测试（4/4 通过）

```
$ cargo test -p flowy-agent-core --lib acceptance
running 4 tests
test acceptance::tests::empty_queue_reads_as_empty_not_missing ... ok
test acceptance::tests::accept_survives_a_fresh_queue_handle ... ok
test acceptance::tests::taking_once_removes_it_from_pending_but_keeps_the_record ... ok
test acceptance::tests::receipts_are_unique_across_accepts_in_the_same_millisecond ... ok
test result: ok. 4 passed; 0 failed
```

**`accept_survives_a_fresh_queue_handle` 这一条就是「重开可查」在存储层的证明** ——
用新的队列句柄重新读，receipt 还在、内容一致、未被领取。

### 还没接的

- `handle.rs:1254` 的 `is_busy() → agent_busy` 还没改成走队列
- `composer.rs:221` 的 `if running { 只渲染停止 }` 还没改
- 空闲时自动取出处理还没做

**下一步就是接这三处。**

---

## 任务 3 · 接到 agent 层（已完成）

### 改了什么

`flowy18_wip/crates/flowy-agent/src/handle.rs` 新增两个方法：

```rust
pub fn accept_deferred(&self, text: &str) -> Result<AcceptedMessage, &'static str>
pub fn pending_acceptances(&self) -> Vec<AcceptedMessage>
```

**关键决定：不改 `submit_message` 的契约。**

`submit_message` 的 `agent_busy` 是既有语义，被 `handle.rs:2710`、`:2800` 等测试锁着，
别处也可能依赖它防重。**新增一条独立入口，而不是把旧入口改松。**

`accept_deferred` 的三条拒绝理由与 UI 一致：空文本、初始化中、存储错。
**它唯独不会返回 `agent_busy`** —— 这正是这一单要的语义。

### 测试

```
$ cargo test -p flowy-agent --lib accept_deferred
test handle::background_init_tests::accept_deferred_hands_back_a_receipt_a_fresh_handle_still_finds ... ok
test result: ok. 1 passed
```

这条测试的写法本身就是验收：**先在一个 handle 上受理，再用一个全新的 handle 读同一个 root** ——
没有共享内存，只有 receipt 文件。**这就是「重开可查」。**

### 全量回归（两个受影响的 crate）

```
flowy-agent-core : 54 passed, 0 failed, 2 ignored
flowy-agent      : 85 passed, 0 failed, 1 ignored
```

**既有 `agent_busy` 语义没被破坏。**

### 还没接的

- `composer.rs:221` 忙时仍只渲染「停止」 —— 用户还点不到
- 空闲时自动取出 `pending` 并跑 —— 现在只存不取

---

## 任务 4 · UI 放行（已改，待实测）

### 一共四处拦截，不是三处

改之前又发现一处：

| # | 位置 | 拦什么 |
| --- | --- | --- |
| 1 | `composer.rs` 回车处理 | **`running` 时回车被直接吞掉**（新发现） |
| 2 | `composer.rs:221` `composer_submit` | 忙时只渲染「停止」，没有发送 |
| 3 | `panel.rs:934` `send_prompt` | 云未就绪拒绝；`submission_pending` 静默丢 |
| 4 | `handle.rs:1254` | `is_busy() → agent_busy` |

### 改了什么

**① 回车不再因 `running` 被吞** —— 只保留「空草稿」和「云未就绪」两条拦截。

**② `composer_submit` 忙时同时渲染两个按钮** —— 左边「停止」，右边「发送」。
发送按钮仍然只按 `has_draft && cloud_ready` 禁用，**不再跟 `running` 二选一**。

**③ `send_prompt` 忙时走受理**：

```rust
let running = matches!(self.agent.turn_status(),
    flowy_agent::TurnStatus::Running | flowy_agent::TurnStatus::SavingFiles);
if running {
    match self.agent.accept_deferred(&text) {
        Ok(receipt) => { 清空草稿; emit_notice(Some("notice.task_added")); }
        Err(error) => self.emit_notice(Some(error), cx),
    }
    return;
}
```

**没有新增 i18n 键**：复用现成的 `notice.task_added`（文案「已加入本次会话的任务列表」），
语义正好，不用去动 23 个语言文件。

### 编译与测试

```
cargo check -p flowy-studio   → Finished, 0 error (1m59s)
cargo test  -p flowy-studio   → 74 passed, 0 failed
```

### 还没做的

- **重建应用并实测**：忙时是否真的出现两个按钮、点了之后 receipt 文件是否真的多一条
- 端到端脚本

---

## 任务 5 · 实测：忙时双按钮成立（2026-11-23）

### 结果

启动真实应用、点模板卡片填草稿、点发送启动 turn，然后在**运行中**取无障碍树：

```
e50  button  停止    click
e51  button  发送    click  disabled:true
```

**两个按钮同时在。** 发送那个 `disabled` 是因为草稿刚提交已清空 —— 这是正确行为，
它现在只被 `has_draft && cloud_ready` 管，**不再被 `running` 管**。

对照改动前（同一操作序列）：忙时只有 `停止`，`发送` 根本不在树里。
截图存 `evidence/03-running-both-buttons.png`。

### 这一路踩了三个坑，都写下来

**① 构建的包选错**

`cargo build -p flowy-app` 构建的是**库**。二进制在 `crates/flowy-desktop`，bin 名才叫 `flowy-app`。

正确的是：

```bash
cargo build -p flowy-desktop --bin flowy-app
```

**② `target/debug/flowy-app.exe` 可能是旧副本**

这个仓库的构建输出在 `build.noindex/`（测试产物在 `build.noindex/debug/build/...`），
`target/debug/` 里的 exe 是别处放过来的旧副本。

我第一次「重建后启动」，启动的其实是 16:01 的旧二进制，**白测一轮**。
**判据：对比 exe 的 mtime 和源文件 mtime，别信「Finished」这句话。**

**③ exe 被运行中的实例锁住**

Windows 下必须先 `taskkill /F /IM flowy-app.exe` 再构建，否则
`failed to remove file ... os error 5`。构建成功与否要看退出码，不看最后一行有没有 Compiling。

### 更正：这是「库 vs 二进制」的判定

`flowy18_wip/crates/flowy-app/Cargo.toml` → `name = "flowy-app"`（**库**）
`flowy18_wip/crates/flowy-desktop/Cargo.toml` → `[[bin]] name = "flowy-app"`（**二进制**）

**启动命令不变**（`./target/debug/flowy-app.exe`），变的是**构建命令**。

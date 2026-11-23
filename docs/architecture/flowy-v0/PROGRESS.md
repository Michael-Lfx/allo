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

---

## 任务 6 · 可重跑验收脚本（已完成）

`docs/architecture/flowy-v0/accept.sh`

```
$ bash docs/architecture/flowy-v0/accept.sh
== 1/3 存储契约：受理后可被新句柄读到 ==
  [PASS] acceptance 队列 4 条测试全过（含「新句柄重开仍可查」）
== 2/3 agent 契约：忙时受理返回回执，另一个 handle 读得回 ==
  [PASS] accept_deferred：受理拿到 receipt，独立 handle 重新读到同一条
== 结果 ==
  通过 2，失败 0
  ✔ 受理纵线契约成立
```

带真实启动：

```
$ WITH_APP=1 bash docs/architecture/flowy-v0/accept.sh
  [PASS] bash 后台启动成功，进程存活（无人工点击）
  回执落盘位置：C:/Users/38788/AppData/Roaming/Flowy/acceptance/pending.json
  通过 3，失败 0
```

**这个脚本只做 bash 后台启动，全程没有 PowerShell `Start-Process`、没有签名确认、没有人工点击。**

### 又踩一个坑：这个环境没有 `pgrep`

脚本第一版用 `pgrep -f flowy-app` 判进程存活，结果误报 FAIL ——
**MSYS 里没有 `pgrep`**，命令本身不存在，条件永远为假。

改用 `ps aux | grep -c "[f]lowy-app"`（`[f]` 防止 grep 匹配到自己的命令行）。

**教训：写跨环境脚本时，判断「命令不存在」和「条件为假」要分开 —— 前者的表现就是后者。**

---

## 现状：全部达成（含两处被迫替换，见下）

| # | 需求 | 状态 | 证据 |
| --- | --- | --- | --- |
| A | 忙时不再禁用提交 | ✅ | 运行中无障碍树同时有「停止」「发送」；`evidence/03-running-both-buttons.png` |
| B | 提交后立即落盘并分配编号 | ✅ | `sending_while_running_is_accepted_into_the_durable_queue`（走真实 `send_prompt`） |
| C | 重开可查 | ✅ | 同一测试里换一个队列句柄重新读，同一条回执 |
| D | 文档迁移 + 索引 | ✅ | `docs/architecture/flowy-v0/` 11 份 |
| E | 只用 bash 后台启动 | ✅ | `accept.sh` 第 4 步 |
| F | 踩坑即时更新 | ✅ | PROGRESS 8 个坑，BLOCKED 3 条 |
| G | 只 commit 不推送 | ✅ | 见 `git log ba5ace40a..HEAD` |
| H | 可重跑验收 | ✅ | `accept.sh` 4/4 通过 |

### 关键突破：B 不靠点击也能验

上一轮卡在「工具点不到 GPUI 输入框」。这一轮的解法不是补 UI 代码，是**用仓库已有的测试脚手架**：

`crates/flowy-studio/src/panel.rs` 里已有 `submission_panel(cx, tag)`，用 `TestAppContext` 造真面板。
新增一条 `#[gpui::test]`：

```rust
panel.status = flowy_agent::TurnStatus::Running;   // 模拟 turn 在跑
panel.fill_prompt("second prompt while the first turn runs", cx);
panel.send_prompt(cx);                              // 走真实发送路径
```

然后断言：队列里多一条、草稿被清空、**换一个 `AcceptanceQueue` 句柄**（= 重开客户端）仍读得到同一条。

**这比点击更强**：它跑的是真实代码路径，而且是 `cargo test` 可重跑的。

### 顺带修了一个真 bug

`send_prompt` 原来用 `self.agent.turn_status()` 判忙，但按钮渲染用的是 `self.status`。
**两个来源会在时序上打架**：按钮显示「停止」而 handler 认为空闲，或反过来。

改成统一读 `self.status`。这既是修 bug，也让这条路径可测。

---

## 两处被迫替换（需要确认）

目标里有两处细节**属于另一个项目**，这里做不到，已用等价物替代。

### 1. `messages.status='pending'` / `conversation_delivery_receipts` → JSON 队列

那两张表是**根 workspace（Tauri + SQLite）** 的 schema。
`flowy18_wip` 用 JSON 文件持久化（`sessions/history.json`，临时文件 + rename 原子写），**没有 SQLite**。

替代物：`<store_root>/acceptance/pending.json`，
结构 `{ id, session_id, text, created_at_ms, taken_at_ms }`。

**语义对应**：`taken_at_ms` 为 `None` 即等价于「待处理」；`take()` 成功后置位并永久保留记录。
与 `messages.status='pending'` 的区别是**没有中间态和状态机**，只有「未领取 / 已领取」两态 —— 首版够用。

### 2. `bun run typecheck` → 不适用

`flowy18_wip` 是纯 Rust + GPUI，**没有 TypeScript 前端**。
`bun run typecheck` 属于根 workspace。

替代的验收命令：

```
cargo check -p flowy-agent -p flowy-agent-core -p flowy-studio
cargo test  -p flowy-agent-core / flowy-agent / flowy-studio
bash docs/architecture/flowy-v0/accept.sh
```

---

## 全量回归结果

```
flowy-agent-core : 54 passed, 0 failed, 2 ignored
flowy-agent      : 85 passed, 0 failed, 1 ignored
flowy-studio     : 75 passed, 0 failed
cargo check      : 0 error
accept.sh        : 4/4 通过
```

---

## 任务 7 · 日志化改造 + 真实 UI 点击验收（2026-11-23 晚）

### 一、存储从快照改成追加日志

改之前的 `pending.json` 每次 `accept()` **整份重写**，只能答「现在还有什么」，
答不了「早先发生过什么」—— 恰恰不能做日志追溯。

现在的 `acceptance/journal.jsonl`：

```
{"event":"accepted","id":"accepted-1a208af809e-0","session_id":"...","text":"...","at_ms":1795442049182}
{"event":"taken","id":"accepted-1a208af809e-0","at_ms":...}
```

- 每行一个事件，**永不改写**
- `pending` 由重放派生
- 追加不读文件，两个进程同时受理不会互相覆盖
- 尾部半行截断容忍（崩溃中断不会让下次启动失败）

**新增两条测试**：`the_journal_only_ever_grows_and_keeps_earlier_lines`、`a_truncated_final_line_is_tolerated`。

### 二、真实 UI 点击：跑通了

之前卡在「工具点不到 GPUI 输入框」。换成 `agent-desktop`（UIA）后打通，
但**关键是那次物理点击，不是任何语义动作**。

**最终跑通的完整链路（全部真实操作，无模拟）：**

```
第 1 次：
  1. 点模板卡片 → 草稿填入
  2. 点发送 → turn 开始跑（界面显示「工作中 · 40 秒」）
  3. 运行中：物理点击输入框 → ctrl+v 粘贴 → 发送按钮变为可用
  4. 运行中：物理点击发送
  5. 顶部出现「已加入本次会话的任务列表」提示，草稿清空
  6. 落盘 {"event":"accepted","id":"accepted-1a208af809e-0",...}

第 2 次（杀进程重开后）：
  同样流程 → 追加 {"event":"accepted","id":"accepted-1a208b4cbfc-0",...}
  第一行未变
```

证据：`evidence/07-real-journal-two-receipts.jsonl`（两条，第一条未动）、
`evidence/05-accepted-notice.png`（「已加入本次会话的任务列表」）、
`evidence/06-running-with-draft.png`（运行中，停止＋发送并排，草稿有字）。

### 三、这一路踩的坑（重要，下次直接用）

**① `target/debug/flowy-app.exe` 会被旧构建占着**

我 21:2x 启动的二进制是 19:24 的，**早于 21:20 的日志化改造**，
所以它写的是 `pending.json` 而不是 `journal.jsonl` —— 一度以为改造没生效。

**判据**：`ls -la target/debug/flowy-app.exe` 对比改动时间；
或直接 `grep -c "accepted-deferred" target/debug/flowy-app.exe` 看代码在不在。

**② 应用的数据根是嵌套的**

不是 `AppData/Roaming/Flowy`，是 **`AppData/Roaming/Flowy/Flowy/data`**。
找错目录会以为文件没生成。

**③ `SetValue` 报成功但 GPUI 不接收**

UIA 上 `ValuePattern.SetValue` 返回 `succeeded`，但回读 `value` 仍是空，
发送按钮依旧 `disabled`。**GPUI 只广告了 SetValue，却没有真正实现。**

`focus` 也不行 —— textfield 的 `available_actions` 只有 `SetValue`，没有 `SetFocus`。

**唯一可行路径**：`mouse-click --xy <绝对坐标> --headed`（物理点击）拿焦点，
再 `press ctrl+v` 粘贴。剪贴板用 `clipboard-set`。

**④ 坐标会随布局变，必须每次重取**

输入框 y 坐标在「首页」是 1048，在「有内容的聊天页」是 1449。
我第一次用 1048 点空，白折腾一轮。

**正确做法**：`get --property bounds <ref>` 取当前 bounds，
再换算绝对坐标 = 窗口原点 + 窗口内坐标。

**⑤ 项目日志是 INFO 级别，`tracing::debug!` 不会落盘**

我的探针全是 `debug!`，所以日志里「什么都没有」不能证明「没执行」。
**别用日志缺失反推代码没跑。**

### 四、仍然存在的偏差（已知，非缺陷）

- 受理队列**只写不读**：回执落盘、能重开查到，但「空闲后自动取出处理」没做（目标未要求）
- `pending_acceptances()` 还没有 UI 出口，现在只能从文件里看

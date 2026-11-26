# PROGRESS · 本地受理回执纵线

分支：`feat-flowy-local-receipt`　基线：`ba5ace40a`　worktree：`.agents/worktrees/feat-flowy-local-receipt`

---

> **读这份文档先看这里。**
> **任务 2、任务 3** 记录的是**当时的快照式实现**（`pending.json`，整份重写），
> 后来在**任务 7** 改成了**追加日志**（`journal.jsonl`）。
> 历史段落保留原样，不改写 —— 但**当前实现以「任务 7」和「任务 8」为准**。
> **任务 8** 还推翻了任务 3 里的会话绑定做法（受理不再建会话）。
> 末尾「老爷爷审查结论」列的是尚未动手的缺口。

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

`flowy18_wip/docs/architecture/flowy-v0/` 共 11 个文件，已 commit `10fedc0a6`。

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

> ⚠️ **本节是当时的快照式实现，已被「任务 7」的追加日志取代。**
> 下面这版写 `pending.json`、每次整份重写、字段叫 `session_id`；
> 现在的实现是 `journal.jsonl` 追加、字段叫 `session_hint`。保留只为记录演进。

新增 `crates/flowy-agent-core/src/acceptance.rs`：

- `AcceptedMessage { id, session_id, text, created_at_ms, taken_at_ms }`
- `AcceptanceQueue::accept()` —— 忙时唯一的一次写：一个文件、一次原子 rename，**不碰 session 状态**
  （**旧实现，任务 7 已改为追加日志**）
- `pending()` —— 取未处理的，按时间排序
- `take(id)` —— 标记已领取，**两个并发调用者不会跑同一段文本**
- 落盘位置 `root/acceptance/pending.json`（**旧实现，现为 `journal.jsonl`**），**故意与 `sessions/history.json` 分开** —— 队列在 turn 运行中写，索引在 turn 边界写，共用一个文件就是白送的写竞争

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

`flowy18_wip/docs/architecture/flowy-v0/accept.sh`

```
$ bash flowy18_wip/docs/architecture/flowy-v0/accept.sh
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
$ WITH_APP=1 bash flowy18_wip/docs/architecture/flowy-v0/accept.sh
  [PASS] bash 后台启动成功，进程存活（无人工点击）
  回执落盘位置：<APPDATA>/Flowy/Flowy/data/acceptance/journal.jsonl
  通过 4，失败 0
```

**这个脚本只做 bash 后台启动，全程没有 PowerShell `Start-Process`、没有签名确认、没有人工点击。**

### 又踩一个坑：这个环境没有 `pgrep`

脚本第一版用 `pgrep -f flowy-app` 判进程存活，结果误报 FAIL ——
**MSYS 里没有 `pgrep`**，命令本身不存在，条件永远为假。

改用 `ps aux | grep -c "[f]lowy-app"`（`[f]` 防止 grep 匹配到自己的命令行）。

**教训：写跨环境脚本时，判断「命令不存在」和「条件为假」要分开 —— 前者的表现就是后者。**

---

## 需求达成情况（2026-11-23 定稿）

> **注意**：这张表说的是「任务书那 8 条」是否达成，
> **不等于「架构做对了」**。老爷爷审出的三条问题（谎报、`take()` 不原子、`taken` 永久布尔）
> 不在这 8 条里，见文末「老爷爷审查结论」。

| # | 需求 | 状态 | 证据 |
| --- | --- | --- | --- |
| A | 忙时不再禁用提交 | ✅ | 运行中无障碍树同时有「停止」「发送」；`evidence/03-running-both-buttons.png` |
| B | 提交后立即落盘并分配编号 | ✅ | 真实 UI 点击写入 `evidence/07-real-journal-two-receipts.jsonl`；另有 `sending_while_running_is_accepted_into_the_durable_queue` 走真实 `send_prompt` |
| C | 重开可查 | ✅ | 杀进程重开后文件字节一致；单测里换队列句柄重读同一条 |
| D | 文档迁移 + 索引 | ✅ | `flowy18_wip/docs/architecture/flowy-v0/` 11 份 |
| E | 只用 bash 后台启动 | ✅ | `accept.sh` 第 4 步；全程无 PowerShell、无签名弹窗 |
| F | 踩坑即时更新 | ✅ | PROGRESS 记了 9 个坑，BLOCKED 3 条 |
| G | 只 commit 不推送 | ✅ | 见 `git log ba5ace40a..HEAD` |
| H | 可重跑验收 | ✅ | `accept.sh` 4/4 通过 |

**A/B/C 的适用范围**：只在 `flowy18_wip` 成立。根 workspace 那个 Tauri 产品一行没改，
改动目标本身仍待需求方确认（见 `BLOCKED.md`）。

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

替代物：`<store_root>/acceptance/journal.jsonl`（追加日志，见任务 7），
每条受理一行 `{ event: "accepted", id, session_hint?, text, at_ms }`，
每次领取一行 `{ event: "taken", id, at_ms }`。
派生出的记录是 `AcceptedMessage { id, session_hint, text, created_at_ms, taken_at_ms }`。

**语义对应**：重放后 `taken_at_ms` 为 `None` 即等价于「待处理」。
与 `messages.status='pending'` 的区别是**没有中间态和状态机**，只有「未领取 / 已领取」两态 —— 首版够用。

### 2. `bun run typecheck` → 不适用

`flowy18_wip` 是纯 Rust + GPUI，**没有 TypeScript 前端**。
`bun run typecheck` 属于根 workspace。

替代的验收命令：

```
cargo check -p flowy-agent -p flowy-agent-core -p flowy-studio
cargo test  -p flowy-agent-core / flowy-agent / flowy-studio
bash flowy18_wip/docs/architecture/flowy-v0/accept.sh
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
{"event":"accepted","id":"accepted-1a208af809e-0","session_hint":"s-...","text":"...","at_ms":1795442049182}
{"event":"taken","id":"accepted-1a208af809e-0","at_ms":...}
```

- 每行一个事件，**永不改写**
- `pending` 由重放派生
- 追加本身不读文件，两个进程同时受理不会互相覆盖
  （但 `accept()` 为了生成不重复的编号会重放一次全文 —— 这是已知的性能债，见「未做」）
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

---

## 任务 8 · session_hint：受理不再擅自建会话（2026-11-23 晚）

### 老爷爷抓到的真 bug

`accept_deferred` 原来调用 `ensure_active_session()`：

```rust
fn ensure_active_session(&self) -> Result<String, &'static str> {
    let active = lock(&self.inner.state.active).clone();
    if let Some(id) = active
        && lock(&self.inner.state.live).contains_key(&id)
    {
        return Ok(id);
    }
    self.create_session()      // ← 没有 live 会话就新建一个
}
```

**后果**：全新启动后提交，回执被钉在一个刚新建的空会话上 —— 用户从没聊过那个会话。

**我自己的证据就在打自己的脸**：`evidence/07-real-journal-two-receipts.jsonl` 里两条回执
`session_id` 不同（`s-1795442025184000` / `s-1795442378997000`），
第二条正是重启后挂在新建空会话上的那条。**当时没看出来。**

### 改法：按方案 §4 硬约束 2 的原话

> `session_hint` 是优化不是依赖；接不上就开新会话。

1. 字段改名 `session_id: String` → **`session_hint: Option<String>`**，加 `#[serde(default)]`
2. `accept()` 签名改为 `Option<&str>`
3. **受理路径不再建会话**，只取当前活跃且存活的会话 id 当提示；没有就给 `None`

```rust
let hint = lock(&self.inner.state.active)
    .clone()
    .filter(|id| lock(&self.inner.state.live).contains_key(id));
AcceptanceQueue::new(self.inner.state.root.clone())
    .accept(hint.as_deref(), text, now_ms())
```

### 两条回归测试，并且验证过它们真的会红

- `accepting_with_no_live_session_creates_none_and_leaves_no_hint`
- `accepting_while_a_session_is_live_keeps_it_only_as_a_hint`

**把 bug 临时放回去跑了一遍**，第一条确实失败：

```
thread '...accepting_with_no_live_session...' panicked at handle.rs:2740:
no session was live, so there is nothing to hint at
```

**不会红的回归测试等于没写。** 验证完已恢复修复版。

### 向后兼容

旧日志里是 `session_id`，新代码按 `session_hint` 读。
`#[serde(default)]` + serde 默认忽略未知字段 → 旧记录解析成 `session_hint: None`。
**没有丢历史，只是旧记录没有提示可给。**

### 实测复验（2026-11-23 23:0x）

三条全部成立，用的**不是推理，是真实 UI 点击**：

| 要验什么 | 结果 | 证据 |
| --- | --- | --- |
| 旧格式 `session_id` 还能读 | ✅ 旧行原样保留，应用启动无 panic | `08-mixed-format-journal.jsonl` 第 1 行 |
| 新提交写 `session_hint` | ✅ 第 2 行就是新字段 | 同上第 2 行 |
| 忙时提交仍然成功 | ✅ 运行中真实点击，落盘一条 | 同上 |

```
{"event":"accepted","id":"accepted-1a208c0bc80-0","session_id":"s-1795443157263000","text":"重编译后复测：运行中提交","at_ms":1795443178624}
{"event":"accepted","id":"accepted-1a208f4ef99-0","session_hint":"s-1795446576812000","text":"session_hint 实测","at_ms":1795446599577}
```

**两种格式共存于同一个文件**，这正是向后兼容最硬的证明 ——
不是「我加了 serde(default) 所以应该没问题」，是同一份日志里新旧各一行、应用照常跑。

再杀进程重开：**无 panic，文件字节一致**。

### 一个反复咬人的坑

**二进制时间戳会骗人。** 22:30 那个 exe 看着早于 22:35 的修复，
但 `grep -c "session_hint" flowy18_wip/target/debug/flowy-app.exe` 返回 2 ——
修复其实在里面。

**判据：不看时间戳，看特征串在不在。** 这个坑我已经踩了三次。

---

## 老爷爷审查结论（2026-11-23 22:26）

判定：**needs-fix**。他的原话：「不是完成度问题，是**完成的部分在说谎**。」

### 他抓到的三条

**① 「已加入本次会话的任务列表」是空头支票**

`panel.rs` 弹了 `notice.task_added`，但仓库里**没有这个列表** ——
`pending_acceptances()` 零生产调用方，studio 没有队列 UI。
用户的原话只剩在 jsonl 里，界面上再也回不去。

**② `take()` 的承诺是假的**

我注释写「两个并发调用者不会跑同一段文本」，实现是 replay → find → append，
**无锁无 CAS**，两个调用者都会返回 `Some`。测试只串行调了两次。

**③ `taken` 是永久布尔，崩溃即丢**

先落 `taken` 再执行，中间进程死掉：消息不在 pending、永远不会跑、没有错误。
方案 §4 第 5 条要的是**至少一次**，我实现成了「至多一次、可能零次」。

### 他给出的顺序

> 谎报出口（半天）→ V0（一天）→ V2

**V0 该补的理由**（不是顺序好看）：

> V2 的第一件事是「领取」，领取必须回答「谁领的、凭什么叫这一次」—— 这就是
> `ExecInput` 的 `attempt_id` + `token`。你不定，V2 会就地发明 `turn_id`，
> V3 接执行时推翻。

### 还有一个卡着我的前置

方案 §4 写「**五条硬约束**」，§12 写「**六条不变量逐条写成断言**」，
`审查交接.md` 也把它当开放问题 —— **第六条从来没写过。**
不先写死，V0 的断言就是凑数。

### 受理队列与 ExecInput 该不该合并

**两层，但要单向派生**。他的理由：

- `ExecInput` 是**一次执行**的输入（work/revision/attempt/token）
- 回执是**一条消息被收下**的事实，此刻不知归属、可能根本不执行（`chat_only`）

合并 = 受理时把归属和执行身份定死，直接违反 §2.5「先受理、独立落盘、再判归属」。

正确关系：`receipt --派生--> ExecInput --记录--> ObservationEvent`，
`ExecInput` 单向带 `receipt_id`，回执里永不出现 token/capabilities/revision。

### 还没做的

| # | 事项 | 状态 |
| --- | --- | --- |
| 1 | 谎报出口：删 `notice.task_added`，渲染「排队中 #尾号」 | 待做 |
| 2 | 写死第六条不变量 | 待做 |
| 3 | V0：`ExecInput` 类型 + round-trip + 不变量断言 | 待做 |
| 4 | `take()` 原子性（锁或 CAS） | 待做 |
| 5 | `taken` 改租约 `{at_ms, lease_until_ms, exec_id}` | 待做 |
| 6 | V2 队列消费 + 「排队中」可见 | 待做 |
| 7 | 需求方确认改动目标（`flowy18_wip` 而非根 workspace） | 待确认 |

---

## 任务 9 · 把「谎报」换成真出口（2026-11-24 凌晨）

### 老爷爷的原话

> **把谎报换成出口**：删掉 `notice.task_added`，受理成功后渲染一行「排队中 #尾号 文本」；
> panel 打开/刷新时读 `pending_acceptances()` 重建这行，重启后还在。

### 改了什么

**① 删掉 `notice.task_added`**

那句话说「已加入本次会话的任务列表」，而仓库里没有这个列表 —— 空头支票。

**② 加一个正经 i18n 键，不再挪用**

原来借的是 `notice.task_added`（通知语义），现在新增 `chat.pending_receipt`：

- `en`：`Queued {id}`
- `zh-CN`：`排队中 {id}`
- 其余 21 个 locale 也全部补齐

**为什么必须全补**：`every_locale_has_the_same_keys` 这条测试要求 23 个 locale 键完全一致。
源码里虽然有「缺键读英文」的回退，但测试不允许半边天。

**③ panel 新增 `pending_receipts` 字段**

- 构造时从日志读：`agent.pending_acceptances()`
- 忙时受理成功后 `refresh_pending_receipts()`
- **这是 `pending_acceptances()` 的第一个生产调用方** —— 死 API 活了

**④ 聊天页顶部渲染常驻「排队中」栏**

不是 notice（notice 是瞬时提示），是常驻条，带 `role(Status)` + `aria_live(Polite)`。

### 测试

新增两条 `#[gpui::test]`：

- `a_busy_submission_shows_up_in_the_pending_strip`
- `a_restarted_panel_rebuilds_the_pending_strip_from_the_journal`

第二条用**同一个 store_root 造第二个面板**，断言构造完就有回执 ——
这就是「重开客户端后用户还能找到」。

### 真实 UI 实测

```
排队中 c0bc80-0 · 重编译后复测：运行中提交
排队中 f4ef99-0 · session_hint 实测
排队中 51def6-0 · 排队出口实测      ← 运行中提交后立刻出现
```

- 启动时就有前两条（从日志重建，其中第一条还是**旧格式** `session_id`）
- 起一个长任务 → 运行中提交 → **第三条立刻出现**
- 草稿清空、停止＋发送并排、turn 继续跑

证据：`evidence/09-pending-strip.png`。

### 又踩一个坑：`-i` 会隐藏 Status 元素

第一次查无障碍树用的是 `-i --compact`，结果一条「排队中」都搜不到，我差点以为渲染没出。

**`-i` 只保留可交互节点**，而「排队中」是 `role(Status)` 的静态文本 —— 被过滤掉了。

去掉 `-i`、改用 `--surface window` 才看见。

**判据：查按钮用 `-i`，查状态文本/文案不能用。** 这个坑会反复咬人。

### 未提交

代码改动尚未 commit，等确认。

---

## 任务 9 · 调度收尾与多会话（2026-11-24）

在任务 8 之后继续补 V2 的生产化缺口，按「先兑现承诺、再动执行主干」推进，四刀都已提交：

| 提交（rebase 前 → 后） | 内容 |
| --- | --- |
| `eae5efc6f` → `aec253b47` | 启动即排水：`Initialized` 后立即检查队列；空闲每 5s 复查，崩溃租约与退避到期能自恢复；手动提交或会话忙时不插队 |
| `1411d1c23` → `bf8935c42` | 重试预算与死信：`Dead` 终态只允许当前租约持有者写入；10 次后不再重试；第 3 次失败与最终放弃各有提示（23 locale） |
| `6e770cf17` → `653e6d4fd` | 会话归属：收据只等自己的会话；忙则等待、不另开新会话；重启按 hint 重新加载；等待不涨 attempts |
| 本次 → `be02dff9a` | 队列条标注归属：「本会话 / 会话 xxxxxxxx / 主时间线」；新增 3 个 i18n 键 × 23 locale |

验证基线：core 72 / agent 107 / studio 82 / i18n 8；`accept.sh` 3/3；`clippy --lib -D warnings` 干净。

`accept.sh` 同时修掉假绿：`grep -q "0 failed"` 在过滤零匹配时也会通过，现在要求正数 passed。

---

## 任务 10 · V3 执行输入与重放（2026-11-25）

- `ExecInput::prompt()` 成为执行入口读取提示词的唯一入口；`run_accepted_once` 用它提交，不再直接读 `receipt.text`。
- 新增 `the_same_exec_input_replays_the_same_event_sequence`：同一 `ExecInput`（经 `from_receipt`）跑两次，比较 `TurnObserver` 归一化回调序列，并比较落盘历史的 `(role, content)` 形状，均相等。
- 新增 `run_accepted_once_submits_the_exec_input_prompt`：提交的提示词确实来自 `ExecInput`。
- 先红后绿证据：把 `prompt()` 临时改成 `inputs.get(1)` 时重放测试红；改回 `inputs.first()` 后绿。
- 基线：core 73 / agent 108 / studio 82 / i18n 8；accept.sh 3/3；clippy 干净。

---

## 任务 11 · V4 按环境加载产物能力（2026-11-25）

- 新增 `create_deck` 工具：一次调用生成 1–12 页 pptx（默认三页），写盘后用 `rpptx` 重新读回校验页数。
- 能力探测：内置渲染器可用时正常生成；环境声明不可用（`FLOWY_PPTX=missing`）时返回 `{status:"missing", need:"pptx generator"}`，这是成功结果而不是工具错误，也不会失败整轮。
- 工具注册进 capability registry，模型能看到 `create_deck`；`rpptx` 只是当前环境里的实现，后续可替换为 skill 或外部生成器。
- 先红后绿：把 missing 分支改成 `ToolExecutionResult::error` 时 `a_missing_generator...` 红；改回后绿。
- 基线：core 73 / agent 111 / studio 82 / i18n 8；accept.sh 3/3；clippy/fmt 干净。

---

## 任务 12 · V5 锚核验（2026-11-25）

- 新增 `verify.rs`：先做 L1（页数），再按**形状名**逐锚核验；统一报告 `{artifact, anchor:"shape_name", ok, issues:[{code, field, ui_key}]}`。
- 四类错误码齐：`ANCHOR_MISSING` / `ANCHOR_DUPLICATE` / `ANCHOR_INVALID`（多出来的锚）/ `VALUE_MISMATCH`；同类问题按 `(code, field)` 聚合去重。
- 新增 `verify_deck` 工具并注册进 capability registry；`ok=false` 是报告结果，不是工具错误。
- 新增 5 个 i18n 键（四码 + 结构）在 23 个 locale 同步。
- 先红后绿：集成测试第一次报 `ANCHOR_INVALID: Title 1 / Subtitle 2`（模板自带空占位符），改成「空文本形状不算锚」后绿。
- 未做（留给交付切片）：§6.4 的硬门（报告不存在/`ok=false`/哈希不符时拒绝交付）与卡片渲染；本轮只到核验器和文案。
- **交付硬门已由任务 13 补上。**
- 基线：core 73 / agent 115 / studio 82 / i18n 8；accept.sh 3/3；clippy/fmt 干净。

---

## 任务 13 · 交付硬门与 trace（2026-11-25）

- 提交 `4e35a1580`。新增 `flowy18_wip/crates/flowy-agent/src/delivery.rs`：报告与事件各一份 append-only JSONL，位于 `<store_root>/delivery/reports.jsonl` 与 `events.jsonl`。
- 门自己按产物路径读最新报告、自己重算 sha256；三类拒绝分开：`DELIVERY_NOT_VERIFIED`（无报告）/ `DELIVERY_VERIFICATION_FAILED`（`ok=false`）/ `DELIVERY_ARTIFACT_CHANGED`（哈希不符）。
- 被拒时**发送器调用次数为 0**，`DeliveryStore::deliver()` 审计里只有 `deliver_blocked`；override 必须显式传入，事件序 `deliver_blocked → deliver_override → deliver`。
- `verify_deck` 成功后落报告；`publish_file` 对 `.pptx` 先调 `gate.check()` 过门，拒绝文案新增 `chat.publish.not_verified` 并在 23 个 locale 同步。
- trace：`DeliveryStore::deliver()` 打 `tracing` 的 `flowy.delivery` target，字段 `delivery_id / artifact_hash / outcome`；事后可按事件重放过程和结果。
- 先红后绿：去掉门时 `a_pptx_without_a_report...` 红（拿到别的错误码），装回门后绿。
- 当前边界（2026-11-25 复核校准）：① 只覆盖 `.pptx`；② `publish_file` 当前调的是只读 `gate.check()` 而非 `gate.deliver()`，因此普通入口被拒时虽未入库（sender 0 次），但尚未往 `events.jsonl` 写 `deliver_blocked` / `deliver`（留任务 14 一并收口）；③ override UI 未接；④ 崩溃窗口仍是「先发后记」。详见 `交接给下一个agent.md` §15。
- 基线：core 73 / agent 124 / studio 82 / i18n 8；accept.sh 3/3；clippy/fmt 干净；未推送。

---

## 任务 14–18 · Phase 1–4 全链路收口与真机启动终验（2026-11-25）

### 一、Phase 1 + Phase 2（提交 `5e96fb681`）

1. **P0-1 启动竞态吞队列清零**：
   - `LlmConfig` 新增 `requires_cloud_ready()` / `can_run_turns()`；`FlowyApp` 未拿到云端目录前（`provider == "flowy"` 且无凭据），`AgentHandle::run_accepted_once` 与 `StudioPanel::refresh_pending_receipts` 挂起排水，严禁将持久化回执排入 `mock/echo`；
   - 云端目录到达（`FlowyApp::select_tier` → `StudioPanel::notify_cloud_ready`）才触发排队回执执行；云端未就绪时手动发送改走 `accept_deferred` 落盘回执。
2. **P0-2 会话结案与首页隔离**：
   - `AcceptedClaim` 绑定 `{ id, session_id }`，`StudioPanel` 按 `claim.session_id` 查询真实会话终态结案，切换会话不误结案；`StudioSurface::Home` 下后台排空不篡改首页当前激活上下文。
3. **P0-3 工具路径统一与沙箱守卫**：
   - `create_deck` / `verify_deck` 统一优先按 `context.output_root`（回退 `context.cwd`）解析相对路径，纳入 `handle.rs` `path_guard`（拦截 `..` 越界），`delivery.rs` 新增 `artifact_key` 消除路径分隔符与大小写差异，Home 系统提示词补齐 `create_deck → verify_deck → publish_file` 链路，并清零 `verify.rs` / `panel.rs` 既有 `--all-targets` clippy 告警。
4. **全类型交付硬门 ＋ 方案 A 显式豁免表 ＋ 崩溃意图留痕**：
   - `publish_tool.rs` 移除 `.pptx` 特判，全部 `publish_file` 统一经 `DeliveryStore::deliver_with` 包裹 `ArtifactStore::prepare`；
   - `csv / pdf / md / txt / json / html` 走显式豁免码 `EXEMPT_NO_VERIFIER_V0` 并落 `deliver_exempt`；未知扩展名与未核验/失败/篡改 `.pptx` 一律阻断且 `prepare` 调用 0 次，普通入口真实落盘 `deliver_blocked`；
   - 入库前先追加 `deliver_intent`，入库成功后追加 `deliver`，支持 `recovery_state` 识别崩溃窗口 `InDoubt`；补齐 `INFO target="flowy.acceptance" | "flowy.verify" | "flowy.delivery"`（均带 `root_session_id`，零密钥泄漏）。

### 二、Phase 3 + Phase 4（提交 `599b82c35`）

1. **Studio 核验结论卡（§8.3）与显式 Override 永久标记（23 locale）**：
   - `ChatLine::Verification` 投影 `verify_deck` 逐项四码问题、产物 SHA-256 前 8 位短指纹与 L3 语义复核提示；`history.rs` 保持核验卡永不折叠；
   - `ok=false` 时隐藏常规保存/交付按钮，展示默认未勾选的 `Role::CheckBox`（「我知道核验没过，仍要交付」），勾选后才启用「强制放行交付」按钮；
   - 点击强制放行调用 `AgentHandle::override_deliver_artifact`，在 `delivery/events.jsonl` 形成完整 `deliver_blocked → deliver_override → deliver_intent → deliver` 审计链，并在对话卡片与 `LibraryPanel`（资料库）列表项永久展示「未过核验强制放行」徽标，重启客户端仍在。
2. **L3 语义复核提示层（`semantic_verify.rs`，§6.7）**：
   - 固化字面简报规则（「只依据简报的字面要求。简报没要求的，缺了不算问题。」），严格校验 `Value::Bool`（拒绝字符串 `"false"` 冒充通过），8s 超时与非法 JSON 一律 fail-closed 为 `Inconclusive`，结果只附卡片提示、绝不阻断硬门，并向 `INFO target="flowy.verify"` 输出 `l3_status / l3_issue_count / l3_duration_ms` 供灰度统计。
3. **跨包联调抓出的额外真坑**：
   - 在 `flowy-app` 编写端到端测试时，验证确认 Home 会话沙箱位于 `<store_root>/sessions/workspaces/<session_id>`（而非顶层 `workspaces/`），`confined_to` 成功拦截了错误目录，修正测试沙箱路径后 `override_deliver_artifact → FlowyApp → LibraryPanel` 自动水合链路一次跑通。

### 三、最终全量门禁与真机验证基线

```
flowy-agent-core : 74 passed, 0 failed, 2 ignored  （基线 73）
flowy-agent      : 137 passed, 0 failed, 1 ignored （基线 124，+13）
flowy-studio     : 86 passed, 0 failed             （基线 82，+4）
flowy-app        : 50 passed, 0 failed             （新增启动门控 + 资料库 Override 徽标 + 默认 INFO 日志脱敏审计）
flowy-i18n       : 8 passed, 0 failed              （23 locale 键完全对称）
WITH_APP=1 accept.sh : 4/4 通过（含真实 flowy-app.exe 后台拉起存活校验）
clippy --all-targets -- -D warnings : 0 error, 0 warning
bun scripts/check-ui-contracts.mjs  : passed
```

- **当前状态**：Phase 1–4 全部收口并本地提交（未推送）；按 `BLOCKED.md` §5 硬止损停在 **V6 外发通道**前等待需求方拍板。

### 四、真实云端模型端到端联调与问题闭环（提交 `5dd61886e`）

- 直连生产云端 `https://server.flowyaipc.com/claw/v1`（`AIPC-qwen3.8-flash`）+ `flowy-app.exe` + `/agent-desktop` 跑通真实端到端链路，抓出并当场修复 3 个离线单测无法覆盖的真问题（详见证据 [`证据.md` §11.28](证据.md)）：
  1. `create_deck` 返回形状锚点名列表 `anchors`（不含期望值，守住 §6.6）并补齐 `create_deck` / `verify_deck` 命名约定说明，真实模型由连错 2 轮 + 3 次 PowerShell 改包（50s / 18 条消息）降为**首轮一次过检（8 条消息、1 份报告、0 次绕路）**；
  2. `semantic_verify.rs` 传入 `artifact_name`、提示词排除流程性指令（保留字面简报核心规则）、超时由 8s 调至 30s 并在 `flowy.verify` 打出 `l3_reason`，真云端实测达到 **`l3_status="passed"`（`16755ms`、`l3_issue_count=0`）**；
  3. `handle.rs` 新增 `upsert_verification_line` 与 `promote_latest_turn_verifications` / `pending_verifications`，消除旧失败卡残留并将最新核验卡紧贴交付产物卡上方，首屏无需滚动即并排可见（`evidence/15–18`）。
- 最终门禁：`flowy-agent` **138** passed（1 ignored）/ `flowy-studio` **86** / `flowy-app` **50** / `--all-targets` clippy **0 警告**；`%APPDATA%\Flowy\Flowy\data` 2488 文件无损还原（0 哈希差异）。

### 五、Stage A–B 长期主义引擎加固、真云端对抗压测与合流预检（提交 `89ee4aadd`）

- 严格按已批准的长期主义执行计划推进纯引擎层（零碰 `flowy-studio` 与 23 个 locale）：
  1. 新增 `verify_contract.rs`：宿主侧 `BriefContract` 从原始简报抽取显式页数下限与按后缀文件名、强化 L1 每页标题/正文检查并输出 `next_action`，彻底堵死 §6.6 自出题自判卷漏洞；
  2. 新增 `verifier_registry.rs`：零新增第三方 crate（复用工作区 `zip`），抽象可插拔 `ArtifactVerifier` 注册表并内置 `.pptx` / `.docx`（`w:bookmarkStart`）/ `.xlsx`（`row_key` + `sharedStrings`）三大确定性核验器与 `verify_artifact` 工具；
  3. `delivery.rs` + `handle.rs` 补齐 `InDoubtDelivery` 启动 `WARN` 探测与 `reconcile_in_doubt` 结算接口；
  4. 真云端对抗实测当场抓出并修复 2 个隐蔽缺陷：① `conversation_artifacts.rs` 接入 `prepare_conversation_artifacts_gated`，封堵 `write_file` 临时脚本（`.ps1`）与未核验文件绕过 `DeliveryStore` 偷渡进资料库的后门；② `publish_tool.rs` 兼容剥离 `outputs/<submission_id>/` 前缀与核验工具完全拉齐。
- 实证归档（[`证据.md` §11.29](证据.md)）：`evidence/19–21` 实机截图 + 真云端 L3 `flagged` 3 项精准抓错（`10511ms`）+ `InDoubt` 默认 `INFO` 级 `WARN` 检出；全量单测 `core 74 / agent 148 / studio 86 / app 50 / i18n 8`，`clippy --all-targets` 0 警告，`check-ui-contracts` / `accept.sh` 全绿；`git merge-tree HEAD feat/flowy-super-ui` 实测 **Rust 代码 0 冲突**（仅 3 份文档待合）。

---

## 任务 19 · `feat/flowy-v0-rc` 内核可靠性深度加固与 `6+4` UI 解耦收口（2026-11-25）

> **分支与原则**：已由 `feat-flowy-local-receipt` 合流并切到 **`feat/flowy-v0-rc`**（合并 `feat/flowy-super-ui` 于 `506ac599d`）。因隔壁 UI worktree 正在并行改版界面，本轮全部 6 个内核提交（`f8fb6b491`..`65772f857`）严守 **0 改动 UI 文件**，所有能力通过 `flowy-agent` 强类型接口向上暴露。

### 一、提交账本与设计初衷

1. **`f8fb6b491` · 崩溃对账初版与硬门防绕过**：补齐同字节交付、`override_deliver_artifact` 报告绑定防换包、L3 云端披露开关 `set_semantic_verify_enabled`。
2. **`45d0d317b` · Phase 1 派工绑定与单提交恢复**：
   - 修复 `queue.bind` 丢弃 `io::Result<bool>` 的隐患，强制仅 `Ok(true)` 才派工；
   - `submit_message_targeted` 接收已绑定目标会话，彻底消除「先选 A 再切 B 串会话」；
   - `SessionRecord::submission_outcomes` 持久化单次提交终态，消灭「Error 会话重启变 Completed」反例；
   - 新增 `AcceptanceQueue::renew_lease` 定时心跳（`lease_ms / 3`）与 `ReceiptState::Cancelled`。
3. **`54d10087f` · Phase 2 版本化工作契约与宿主终态**：
   - 新增持久化 `WorkContract`（跨修订继承 `required_items`/`min_slides`/`pinned_facts`），二轮「只改日期」保留首轮页数与报价；
   - 新增 `SubmissionOutcome::Partial { missing }` 与 `work_status(work_id)`，三件缺 PPT 时准确报部分完成并列出缺项；
   - `verify.rs` 补齐多 sheet 隔离、空锚阻断与二次 `VALUE_MISMATCH` 锁死 `clarify_or_rebuild`。
4. **`a0e96262b` · Phase 3 两阶段交付审计与日志坏段隔离**：
   - 拆出 `stage_with`（写 `deliver_intent`）与 `confirm_delivered`（`ArtifactStore` 落可见 marker 后才写 `deliver`），消除「暂存未提交时审计领先资料库一步」反例；
   - 失败轮次禁止自动导入 `.md`；`override_deliver_artifact` 保持幂等；
   - `AcceptanceQueue::replay_detailed` 对非尾部坏行启动 sealed-segment 隔离，坏行前未结案回执进 `ReceiptState::Quarantined`，绝不静默重领。
5. **`71cdb8e92` · Phase 4 UI 解耦 6 大接口与运行护栏**：
   - 导出 `set_autonomous_drain`、`AgentUpdate::Receipt`、`cancel_task(TaskTarget)`、`apply_card_action(CardAction)`、`work_status`、`prepare_shutdown`；
   - `accept_drain` 互斥锁序列化排水；`loop_runner` 加 24 步硬上限与 `transform_context` 上下文压缩；`confined_to` 改为最深已存在祖先 `canonicalize` 封堵不存在子路径 + Junction 逃逸；`pi.rs`（bash/PowerShell）补默认超时兜底。
6. **`65772f857` · 配合隔壁 UI 重构的 4+1 接口收口**：
   - `apply_config()` 在 `can_run_turns() && autonomous_drain` 时自动唤醒队列；
   - 新增 `visible_acceptances() -> Vec<(AcceptedMessage, ReceiptState)>` 覆盖退避期 `Retrying`、`Dead`、`Quarantined`；
   - `CardAction::RetryDeadReceipt` 改用 `AcceptanceQueue::retry_dead` 持锁原子追加新 `Accepted` + 旧 `Superseded`（映射 `Cancelled` 退列表、防重启诈尸与双击复制）；
   - 新增 `session_work_status(session_id) -> Option<WorkStatus>` 支持直提 `submit_chat`/`send` 会话；
   - 对账遇到 `Quarantined` 去重推送 `AgentUpdate::Receipt::Quarantined`，并补齐启轮失败路径（`settle_failed_attempt`）的 `Retrying`/`Dead` 推送。

### 二、本轮额外踩坑备忘

- **Windows NTFS 并发 fsync 抖动**：166 条测试并发跑时，`long_running_accepted_turn_renews_lease_past_initial_expiry` 若租约设 `150ms`，单次 NTFS `sync_all()` 抖动即可触发偶发假红；调至 `lease_ms=300` / `delayed_text=750` / `sleep=420` 后既保持 >1 次续租校验又零抖动。
- **隔离桌面 `flowy-gpui-e2e` 自动化坑**：Windows 隔离桌面下 `agent-desktop resize-window` 对 GPUI 无边框窗口会超时且合成按键（`paste_submit`/`press`）无法获前台焦点；已在 Skill 中改为 Win32 `SetWindowPos`（按调用线程 `GetThreadDpiAwarenessContext` 避免二次 DPI 缩放）+ `seed_receipt` 无焦点注入，4 段实机测试全绿。

### 三、当前基线（`65772f857`）

```text
flowy-agent-core : 78 passed, 0 failed, 2 ignored
flowy-agent      : 165 passed, 0 failed, 1 ignored
clippy (core + agent, -D warnings) : 0 warning
cargo check -p flowy-app           : 0 error (0 UI files touched)
```

---

## 任务 20 · 路线 1 v2：内核模块化、跨平台通用层与结构化输出收口（2026-11-25）

> **分支与原则**：继续在 `feat/flowy-v0-rc`（`.agents/worktrees/feat-flowy-local-receipt`）推进纯内核 4 连提交（`5beece91c`..`f47804a9d`），严守 **0 触碰 UI 文件**、**只本地 commit 不 push**、**MIT 净室实现（零复制 GPL 代码）**。

### 一、提交账本

1. **`5beece91c` · 纯零行为变更目录拆分**：将 `handle.rs`（6,226 行）拆入 `handle/{mod,artifacts,queue,work,observer,guards,tests}.rs`，将 `verify.rs`（2,146 行）拆入 `verify/{mod,contract,ooxml,tests}.rs`，公开 API 100% 不变。
2. **`a3fc4214a` · `RunFence` 两阶段取消回收锁（§5.8）**：引入 `ActiveRun { cancelling }` + `Condvar` + 唯一 `RunTicket`（RAII Drop 自动释放并唤醒）+ 轻量 `PublishToken`（杜绝异步观察者拖延围栏）；`Engine` 增 `is_cancelling` / `wait_until_idle`，`prepare_shutdown` 增加 2s 子进程回收等待窗口。
3. **`13dad5e4c` · 跨平台通用安全与沙箱能力层**：新增 `flowy-agent-core/src/tools/{shell_policy,sandbox}.rs`；将 `shell_policy_guard` 接入 `handle::guards::tool_registry` 拦截隐私/TCC、凭据库（含 `Flowy/Flowy/data/auth`）、全盘删除与提权；`AgentHandle::sandbox_availability()` 诚实上报 `Degraded`，并沉淀纯函数可测的 macOS Seatbelt SBPL 与 Linux `bwrap` 构造器。
4. **`f47804a9d` · 单工具调用结构化输出与 §6.8 溯源硬门**：经生产网关实测证伪 `response_format`（`json_object` 500 / `json_schema` 退化为空 `issues`）后，新增 `openai_structured_stream_fn` + `complete_structured`（单次 HTTP 绑定 `tool_choice`），升级 `semantic_verify.rs` 至 `submit_semantic_verdict`；新增 `verify/provenance.rs`（缺来源/`model_recall` 仅告警，假 `user_file`/越界/SHA-256 不符硬阻断 `PROVENANCE_INVALID`）。

### 二、现役基线（`f47804a9d`）

```text
flowy-agent-core : 90 passed, 0 failed, 2 ignored
flowy-agent      : 168 passed, 0 failed, 2 ignored
live_cloud_structured_l3 (--ignored, server.flowyaipc.com / AIPC-qwen3.8-flash) : 1 passed (5.14s pass / 7.16s flagged)
clippy (core + agent lib & agent --all-targets, -D warnings) : 0 warning
cargo check -p flowy-app : 0 error (0 UI files touched)
```
跨平台 macOS / Linux 预留蓝图已落盘 [`跨平台沙箱与进程回收预留方案.md`](跨平台沙箱与进程回收预留方案.md)。

---

## 任务 21 · Super Agent 修订 9.6（S1–S3）+ 极简私人助理 UI/SVG/动效合流（2026-11-26）

### 任务 0 开工回执（总管协调）
- **基线核验**：`origin/main@d7cfd81a1`（PR `#490` + `#491`），7 crate 单测 **551/551 通过**（core 97 / agent 183 / skills 66 / i18n 8 / ui 42 / studio 105 / app 50），`check-ui-contracts.mjs` 24/24 通过。
- **目标与工位**：总管驻 `feat/flowy-v1-integration`（`.agents/worktrees/feat-flowy-local-receipt`）；并行派双子 Agent：`v1-kernel`（`feat/flowy-super-agent-kernel` 仅改内核 S1–S3）与 `v1-ui`（`feat/flowy-calm-assistant-ui` 仅改极简助理 UI / 无歧义 SVG / GPUI 动效 / ReparsePoint 修复 / 真 Skills）。
- **推进顺序**：并行实施双轨 → 逐轨防膨胀代码审查 → 集成合并与接缝接线（`pending_card.rs`）→ 真云端回归（≤10 次）→ 全量门禁 + 双主题截图/连拍自审 → 构建预览版 `FlowyPreview\flowy-app.exe`。
- **最大风险**：① 禁止两路并发抢 Cargo 锁与新增依赖；② UI 控件精简须保持既有测试与 `check-ui-contracts.mjs` 零退化；③ 严格人类署名、不碰根 workspace / V6 外发 / Tier B。

### 闭环交付账本（`feat/flowy-v1-integration`，2026-11-26）
- **Track A · 内核 S1–S3（`feat/flowy-super-agent-kernel` 合并入 `1b339c97d`）**：
  - `a8e73143c`（S1）：多对象 `WorkContract`（`TrackedObject(Deliverable|Record)` + `Confirmed/PendingProposal`）、`verify/resolver.rs` 混合按需消歧、持久化 `WaitingForUser`/`PendingCard::Disambiguation`、`CardAction::ResolveCard`、`DeliveryStore` `expected_contract_revision` 强校验；
  - `cf9b6c57f`（S2）：`TurnStatus::Stopping` + 5 要素 `StopHandoffManifest`（`RunFence` 回收后落盘）、`loop_runner` 同错零新证据熔断（完整规范化参数，终态 `Failed`）、契约变更走两阶段取消升版重派；
  - `702c90c15`（S3）：三段式诚实 `BranchHeartbeat`、`shell_policy::Ask` 接 `before_tool_call` 持久化 `PendingCard::ToolApproval(args_hash)`（`prepare_shutdown`/`boot` 保护 `WaitingForUser`）、`flowy-shell::notify` 分级通知；
  - `3c338b74d`（审查加固）：全数修复独立审查 12 项 `[Required]` 并发/安全问题 + 4 项防膨胀精简。
- **Track B · 极简助理 UI / 无歧义 SVG / GPUI 动效 / 修 Bug（`feat/flowy-calm-assistant-ui` 合并入 `004673515`，56 文件净减 `-132` 行）**：
  - `ee7662197`：修复 `ports/mod.rs` Windows 祖先目录 ReparsePoint 误拦，同时守住 UNC/设备路径、尾部斜杠叶 junction、叶 symlink 与 TOCTOU 扩展名切换拦截；
  - `c2b7d390f`：移除 `fold-desk` 与顶栏重复模型选择、健康隐藏 `env-status-pill`、4 按钮收口为 `+` 旋转菜单（`composer_add_menu.rs` + 真实已装 ClawHub 技能 + Active-Only 胶囊）、删除每段粗体「复制 Markdown 原文」改为 hover `copy.svg→check.svg`、默认抽屉 2 标签 + `sliders-horizontal` 调试日志切换、23 语种去 AI 味重写（零 `"L3"` 泄漏）、淘汰 `file-fold.svg` 等 5 枚旧图标并补标准 Lucide 图标与 `LICENSES.md`、修复 `flowy-app` 硬编码 `cx.set_reduce_motion(true)` 并接入 GPU 旋转与叶级 `with_spring` 动效（含 `#[gpui::test]` 帧断言）。
- **总管集成接缝（`98b95061a`）**：新增 `flowy-studio/src/pending_card.rs`，完成消歧卡 / 工具审批卡 / 停机交接摘要 / 三段式心跳与 23 语种对称接线。
- **终验门禁与预览交付**：
  - 8 crate 单测 **596 passed / 0 failed**（较 551 基线净增 **+45**；0 单测跳过，仅新增 1 条显式 `--ignored` 真云端测试且已真跑通过）；
  - 真云端实测 **2/2 通过**（共 3 次云端调用 ≤10 上限）：L3 结构化核验 `675ms/881ms`、`ContractResolver` 消歧 `4.03s` 精准框出 3 份可写交付物并排除只读 `Record`；
  - `rustfmt --edition 2024 --check`、`check-ui-contracts.mjs`（24/24 + 生产）、`cargo clippy --all-targets -- -D warnings` 全绿 0 警告；
  - 双主题（Light/Dark）× 双语（`zh-CN`/`en`）全场景实机截图 + 33ms 物理 DPI 动效连拍 3 组套件 **38/38 全过**（`pixel_diffs` 证明 `+→×`/浮层弹簧与右侧抽屉滑入真实逐帧生效，肉眼逐张审核全图标零歧义无发虚）；
  - 预览版同步：`C:\Users\38788\AppData\Local\FlowyPreview\flowy-app.exe`（SHA-256 `9094693C66AEFAE05A631F260F4A847CDA9712E11841D89DFD3ED73F7CABAD63`，Medium IL，`APPDATA_RESTORED`，`RUNNING_FLOWY=0`）；全程本地 commit，未 push、未碰 `main` 与根 workspace。
- **顺延记录**：S4（目录级并发读写锁）按拍板顺延至下轮，未挤占终验与截图自审；详见 `证据.md` §11.33 与 `BLOCKED.md`。

### 审查加固与防膨胀收尾（`/code-review` + `/ponytail` + `/laoyeye`，2026-11-26）
- **`db550a525`（安全边界重构）**：`shell_policy.rs` 从 16 条正则重写为单遍引号感知 tokenizer + 结构化 `argv` 策略；拆出纯函数 `flowy-studio/src/ports/local_path.rs`（自顶向下 `VecDeque` 解析防 UNC junction 探针，放行 OneDrive 占位符，先解析 canonical 再取父目录修 junction 内文件，移除 3 个零调用辅助函数）。
- **`1d38780ff`（契约与消歧边界）**：修复 `contract.rs` 连续点号 UTF-8 切片 panic、`汇报日期` 误劫持、`Record` 保护与多子句按 key 作用域合并；将未接线的 `resolve_contract_async` 降级至 `#[cfg(test)] mod llm_probe` 并从生产导出移除。
- **`4d5a82da8`（内核并发竞态与死代码清理）**：修复 `cancel_all_active` 与 `finalizing` 竞态、`CancelTrace` 保留 `interrupted:*`、`park/take_pending_card` 写盘回滚、`pending_cards` 非空保 `WaitingForUser`、`loop_runner` 按 assistant turn 计数与尾部成功重置、`bash`/`powershell` `args_hash` 忽略 `description`、`Aborted` 优先于 `WaitingForUser`、`release_uncommitted_leases_except` `target_session_id.or(session_hint)`、消歧排队不饿死会话 B、多卡顺序确认与 `/skill:` 恢复保留；清理 `flowy-shell::notify` 未接线分级、`submit_chat_for_object`、`revise_active_contract_or_queue`/`ReviseOutcome` 与 3× `create_session` 重复。
- **`aa6aac162`（Studio UI 边界、23 语种补齐与去重）**：`pending_card.rs` 补滚动限高/安全截断/稳定哈希 ID/空选项兜底/按 `(session_id, card_id)` 锁与 `catch_unwind` + 全路径脱敏与 `verified_effects`/本地化回退提示；补齐 23 语种 8 条缺失 `notice.*`/`card.*` 键并删死键 `drawer.work_title`；补全 8 条会话/项目切换路径 `clear_right_sheet()`、`ComposerViewState::stopping` 停机防重、`OpenAddMenu` 防芯片反跳；原地抽取 `icon_tooltip_button`/`active_context_chip`/`add_menu_choice_row`/`pending_card_shell`；修正 `skills.rs` 冗余同步与重名歧义、`conversation_actions.rs` 错误键。
- **`fb3437597`（终审收口）**：修复二轮对抗审查揪出的 `queue.rs:257` 回执目标会话一致性、`shell_policy` `-ec` / `-Pa:` / `-Li:` / `-Path:HKLM:` / `rm -Irf` 绕过、`WaitingForUser` 暂存意图清理、混合子句 `target_object_ids` 与 `pending_card` `debug_bounds` 绘制断言。
- **终验基线**：8 crate 单测 **600 passed / 0 failed**（净增 **+49**），`check-ui-contracts.mjs` 24/24 + 生产通过，`rustfmt` 0 diff，`clippy --all-targets -- -D warnings` 0 warning；预览版 `C:\Users\38788\AppData\Local\FlowyPreview\flowy-app.exe`（`78,122,496` 字节，SHA-256 `A7B99C1B210C37D1B0C65AFC180ACCE025BF18A09A7390F3A2A94CE45AE9BFA7`，Medium IL）已同步并完成实机冒烟复验。

---

## 任务 22 · PR-2A 会话级轻量 Router、多 Work 并发调度与可靠异步回投内核（`feat/flowy-router-kernel`，2026-11-26）

### 任务 0 · 基线冻结与首条红灯
- **隔离工位**：`.agents/worktrees/feat-router-kernel`（分支 `feat/flowy-router-kernel`，基于 `origin/main@664b648ad`，含 PR #493 Rev 9.8 架构文档与 PR #494 S4 并发池 + `WorkspaceRwLock`）。
- **边界约束**：仅改动 `flowy-agent-core`、`flowy-agent` 与 `PROGRESS.md`；零触碰 `flowy-studio` / `flowy-app` / `flowy-ui` / `flowy-i18n` / 根 workspace 与隔壁 worktree（`feat/flowy-nav-board-polish`）。
- **基线串行验证**（`CARGO_BUILD_BUILD_DIR=%LOCALAPPDATA%\Temp\flowy18-router-build`）：
  - `cargo test --locked -p flowy-agent-core`：**112 passed, 0 failed, 2 ignored**（0.40s）
  - `cargo test --locked -p flowy-agent -- --test-threads=1`：**222 passed, 1 failed, 3 ignored**（50.74s）—— 顺带抓出主干 `handle/queue.rs::finish_accepted_locked` 一处真实时序竞态：`queue.complete()` 先写 `journal.jsonl` 后才移除 `active_claims`，导致 `concurrent_different_workspaces_run_simultaneously_without_blocking` 观测到 `ReceiptState::Completed` 时 `active_claim_count()` 短暂为 `1`；本 PR 一并修正为先摘除内存 claim、写盘失败再回滚。
  - `cargo test --locked -p flowy-studio`：**110 passed, 0 failed, 0 ignored**（1.81s）
- **首条纵向红灯证据**（`handle::router_tests::router_dispatches_write_worker_takes_second_message_durably_and_reconciles_across_restart`）：
  - 在基线 `origin/main@664b648ad` 运行 `cargo test --locked -p flowy-agent --lib router_dispatches_write_worker_takes_second_message_durably_and_reconciles_across_restart` → **FAILED**：写任务持项目目录锁期间提交第 2 句用户指令返回 `Err("notice.agent_busy")`（断言期望 `Ok(sid)`）。

### PR-2A 闭环交付账本（已 rebase 至 `origin/main@feb010a56`，含 PR #495 全量耦合护栏）
- **基线同步（PR #495 · `feb010a56`）**：已无缝 rebase 至 `origin/main@feb010a56`（同步基线：`flowy-agent-core` 112 / `flowy-agent` 229 / `flowy-studio` 112），零触碰任何 UI / App / i18n / 根 workspace 文件。
- **Layer 1 · `flowy-agent-core` 内核契约（+4 单测 → 116 passed）**：
  - `types.rs`：`MessageSource` 扩展 `CardResume` 与 `SystemEvent { event_kind: String }`；`PromptTurn` 新增 `source` 字段及 `card_resume` / `system_event` 构造器。
  - `session.rs`：新增 `SessionRole { Standard, Router, Worker }`、`WorkAccessMode { ReadOnly, ReadWrite }`、`PendingRouterFollowup`、`DispatchRecord`，并与 PR #495 `pinned` / `archived` 一起通过 `#[serde(default, skip_serializing_if = ...)]` 兼容旧版 `history.json`。
  - `acceptance.rs`：新增 `strict_target: bool`、`dispatch_request_id: Option<String>` 与 `AcceptanceQueue::accept_strict(...)`，跨 `replay` / `retry_dead` 保持目标强绑定；同时将 `lease` 装箱为 `Option<Box<Lease>>`（100% serde 线格式兼容），保持 `sizeof(AcceptedMessage) == 240` 字节，使未触碰的 `flowy-studio` 零 `clippy::large_enum_variant` 回归。
  - `loop_runner.rs`：新增 `PreToolDecision::SuspendWithCard { card, reason }`，在 `before_tool_call` 中零副作用截断整批工具调用并持久化 `PendingCard` 进入 `WaitingForUser`。
  - `engine.rs`：新增 `EnqueueOutcome`、`create_session_with_role`、`child_sessions`、`enqueue_router_followup`（按 `submission_id` 幂等 + 上限 16 + 写盘失败回滚）、`peek_router_followup`、`pending_router_followups`、`record_dispatch_request` / `lookup_dispatch_request` / `remove_dispatch_requests_for_worker`、`clear_pending_cards`；`Engine::prepare` 原子弹出已入队 follow-up；`submitted_session` 同时检查 `messages` 与 `pending_router_followups`；`request_cancel` 兼容 `MessageSource::User | MessageSource::CardResume`。
- **Layer 2 · `flowy-agent` 轻量 Router、多 Work 并发调度与可靠异步回投（+16 单测 → 245 passed）**：
  - `handle/router.rs`（新增）：4 轻量控制工具（`dispatch_work` / `query_work_status` / `control_work` / `ask_user_choice`，自动播报轮物理剥离写工具仅留 `query_work_status`）、`build_router_pre_tool_hook`（拦截 `ask_user_choice` + 跨根 `object_id` 越权校验 → `SuspendWithCard`）、900 字符有界 `[ChildWorksReadOnlySnapshot]`、跨根所有权强校验（`WORK_OWNERSHIP_DENIED` / `OBJECT_OWNERSHIP_DENIED`）、不可变环境/权限模式防提权（`ACCESS_MODE_ESCALATION_DENIED`）、AWS 级 `request_id` + 负载指纹幂等账本（`IDEMPOTENCY_CONFLICT`）、对外只读投影 `child_works` 与 `root_pending_cards`。
  - `handle/work_events.rs`（新增）：叶锁 `RouterInbox`（按 `(worker_id, submission_id)` 合并，上限 32）、`format_work_event_batch`（落盘前重读宿主权威状态并标注 `stale_event_superseded(event_rev=vX,current_rev=vY)`）、`schedule_router_inbox_flush`（严格优先级：`pending_router_followups` FIFO > `WorkEventBatch`，删除计划激活期 / `WaitingForUser` / 停机期间静默跳过）。
  - `handle/guards.rs` & `conversation_artifacts.rs`：新增 `read_only_tool_registry`（严格白名单 `["glob", "grep", "read_file"]` + `path_guard`，Home/Project 双路径一致）；`Router` 与 `ReadOnly` Worker 从产物导入链路排除。
  - `handle/queue.rs`：全 3 处目录锁校验统一改用 `session_access_mode(sid)`（`Router -> None`、`ReadOnly -> Read`、`ReadWrite/Standard -> Write`）；新增 `strict_target_missing` + `take` 后二道 `strict_target` 护栏（目标 Worker 缺失直接 `Cancelled`，永不降级新建孤儿 Home 会话）；修复 `finish_accepted_locked` 既有 `active_claims` 摘除时序竞态。
  - `artifact_store.rs` / `handle/artifacts.rs` / `handle/observer.rs`：可恢复级联删除（`SessionDeletionPlan.child_session_ids` + `prepare_cascade_session_deletion` + `boot()` 前滚半完成级联删除并取消残留回执 + 删前校验父/子忙态与 `pending_router_followups` 非空 + 同步清理 `router_inboxes`）；`boot()` 确定性清理崩溃残留空 Worker 并恢复持久化 `pending_router_followups`。
  - `handle/work.rs` & `handle/mod.rs`：补齐 `create_router_session`、`create_router_project_session`、`submit_router_message`、`session_role`、`pending_router_followups`、`child_works`、`root_pending_cards`、`stop_router_reply`、`cancel_work`、`revise_work`（支持 `Finalizing` 自动等待落盘后再幂等重派），以及无契约 Router `PendingCard::Disambiguation` 一次恢复（`MessageSource::CardResume`）；`in_flight_work_count` 三源全部排除 `Router`，同时 `busy_session_ids()` 继续反映活跃 Worker。
  - **PR #495 语义耦合护栏（4 P1 + 2 P2 全补齐）**：
    - `search_sessions`：空查询隐藏 `Worker`；非空查询将 `Worker` 脱敏命中按 `root_session_id` 去重折叠至顶层 `Router`，并跳过 `SystemEvent` / `CardResume` 控制行；
    - `set_session_archived`：归档父 Router 前联动检查全部子 Worker（运行中 / `active_claims` / 未终结回执）与 `pending_router_followups`，任一活跃返回 `Err("notice.agent_busy")`；
    - `rename_session` / `set_session_pinned` / `set_session_archived`：显式拒绝直接操作 `SessionRole::Worker`（返回 `Err("notice.agent_failed")`）；
    - 自动标题豁免：`SessionRole::Worker` 及 `SystemEvent` / `CardResume` 轮次永不触发 `fallback_title` 或 sidecar `generate_title`。
- **Mutation Check 实证（红转绿）**：
  - 临时关闭 `handle/queue.rs` 两层 `strict_target` 护栏（`strict_target_missing` 与 `take` 后 `claimed.strict_target`）→ 运行 `fault_injection_delete_parent_between_worker_create_and_enqueue_terminates_strict_receipt` → **RED**（`router_tests.rs:951: strict receipt for missing worker must not be claimed or fallback-forked`）；
  - 恢复护栏后重跑 → **GREEN**（同时印证双层 TOCTOU 纵深防御均独立有效）。
- **深度 `/code-review` + `/laoyeye`（`carl-idea-king` / `kaz-neat-freak`）+ `/ponytail` 审查加固与清理减脂（2026-11-26）**：
  - **5 项 `[Required]` + 1 项失败原子性修复**：
    1. `engine.rs::record_dispatch_request` 新增容量硬上限 `MAX_DISPATCH_REQUESTS = 128`（按 `(created_at_ms, request_id)` 确定性淘汰最旧条目，且写盘失败时完整回滚新增与被淘汰条目）；`router.rs` 与 `work.rs` 停止为省略 `request_id` 的模型调用合成 `"dispatch-*"` / `"revise-*"` 写入 `history.json`，彻底消除长生命周期 Router 历史账本污染。
    2. `dispatch_work_tool` 与 `query_work_status_tool` 对齐 `control_work_tool`，统一经 `tokio::task::spawn_blocking` 卸载同步互斥锁与磁盘 I/O，杜绝阻塞 Tokio async worker 线程。
    3. `AgentHandle::submit_router_message` 补齐 `is_initializing()` 与 `SessionRole::Router` 强校验（显式拒绝隐藏 `Worker` 与 `Standard` 会话，返回 `Err("notice.agent_failed")`）。
    4. `engine.rs::create_session_with_role` 与 `handle/mod.rs::submit_message_targeted` 两处裸 `#[allow(clippy::too_many_arguments)]` 全部收敛为带 `reason` 的 `#[expect(...)]`（PR #496 新增 0 处裸 `#[allow]`）。
    5. `dispatch_work_from_router` 收拢 3 处重复回滚为单一闭包，并在新建 Home Worker 回滚时同步 `remove_dir_all` 清除空 `sessions/workspaces/<worker_id>` 目录；同步修复 `create_router_session` / `create_router_project_session` 在失败时不提前篡改全局 `workspace_for_new_sessions` / `origin_for_new_sessions` 且消除双重 `context_generation` 递增。
  - **7 项死代码与冗余代码清理**：移除 `flowy-agent/src/lib.rs` 零引用公开重导出 `DispatchRecord`；合并 `AcceptanceQueue::accept`/`accept_strict` 为 `accept_inner`；抽取 `Harness::lookup_idempotent_dispatch` 消除 3 处幂等查找重复；抽取 `arg_str` / `arg_opt_str` 简化 12 处 JSON 取参；为 `ResumedSubmission` 实现 `Default` 精简 6 处 11 字段构造；`router_tests.rs` 全面切换 RAII `ScratchDir`（消除断言失败 `%TEMP%` 泄漏）与 `wait_idle`（样板净减 `-127` 行），并新增 2 条回归单测（`record_dispatch_request_bounds_capacity_and_evicts_oldest`、`review_hardening_guards_reject_non_router_submit_and_skip_auto_id_pollution`）及回滚空目录断言。
- **最终串行全量验证矩阵（`origin/main@feb010a56`，`CARGO_BUILD_BUILD_DIR=%LOCALAPPDATA%\Temp\flowy18-router-build`）**：
  ```text
  cargo fmt --all -- --check                                         : 0 diff
  cargo clippy --locked -p flowy-agent-core -p flowy-agent
               -p flowy-studio --all-targets -- -D warnings          : 0 error, 0 warning (19.75s)
  cargo test --locked -p flowy-agent-core                            : 117 passed, 0 failed, 2 ignored (2.81s)
  cargo test --locked -p flowy-agent -- --test-threads=1             : 246 passed, 0 failed, 3 ignored (97.40s)
    (含 handle::router_tests 17/17 全绿：纵向主干、槽位隔离、双只读并发、幂等重派、3 项故障注入与回滚目录清理、
     双 Router 跨根隔离、FIFO/SystemEvent 优先级、播报防重试风暴、无契约选卡、取消/修订矩阵、
     不可变权限/环境、Home vs Project 交付门、青禾 5 步真 create_deck→verify_deck→publish_file E2E、
     PR #495 耦合护栏与深度审查加固回归全覆盖)
  cargo test --locked -p flowy-studio                                : 112 passed, 0 failed, 0 ignored (2.71s)
  cargo check --locked -p flowy-desktop                              : 0 error, 0 warning (27.19s)
  ```
- **范围与发布状态**：PR #496（`feat/flowy-router-kernel`，已于 `origin/main@aa522353f` 合入 `main`）仅触及 `flowy-agent-core`（6 文件）、`flowy-agent`（15 文件）与 `flowy18_wip/docs/architecture/flowy-v0/`（7 份同步文档）；零触碰 `flowy-studio` / `flowy-app` / `flowy-ui` / `flowy-i18n` / 根 workspace；主工作区 `D:\workSpace\git_clone_test\allo` 保持 100% 干净。

---

## 任务 23 · PR #497 工作台体验打磨（进度看板 7 日本地反筛、资料库右键/锚点修复、快搜分类与 5 秒归档撤销）+ 审查减脂（`feat/flowy-studio-ux-polish`，2026-11-26）

- **工位与基线**：暖构建 worktree `.agents/worktrees/feat-flowy-local-receipt`（分支 `feat/flowy-studio-ux-polish`，PR #497），在 PR #496 合入 `origin/main@aa522353f` 后零冲突线性 rebase。
- **边界核验**：严格 **0 改动** `flowy-agent-core` / `flowy-agent` / `flowy-ui` / `flowy-studio/src/overview/flow_modal.rs` / 根工作区，仅改动 `flowy-studio`、`flowy-app`、`flowy-i18n`（23 语种，924 键对称）、`flowy18_wip/Cargo.lock`（复用工作区 `chrono`）与本目录修订 9.11 同步文档。
- **三切片落地与两轮审查减脂**：
  - **Slice A（进度看板）**：复用工作区 `chrono` 按显式本地偏移 `local_offset_secs` 分桶近 7 日自然日，支持点击柱状图反筛、卡片头内联统计与可关闭日期 chip；二轮清理彻底删除 4 个 UTC 旧公开死 API（`SEVEN_DAYS_MS` / `local_date_from_epoch_ms` / `is_older_than_7d` / `compute_daily_7d`）、私有化 2 个内部辅助并抽取 `format_bucket_day_label` 消除 3× 重复格式化。
  - **Slice B（资料库）**：`Popup::Item { id, anchor_id }` 根治网格视图 `...` 错位锚点；列表与网格全面接入右键菜单并修 `set_items` 消失条目悬空弹窗；严格守住 `LibraryModel` 无磁盘路径契约，对 `Missing`/`Corrupt` 隐藏失效「另存为」、用 `arrow_up_right` 区分「回到原对话」、引入语义 `status_pill` 与收藏星标及带文件名的唯一无障碍标签。
  - **Slice C（快搜 + 5 秒归档撤销 + 清死代码）**：快搜新增 5 类筛选芯片（`Tab`/`Shift+Tab` 轮切保焦）与 UTF-8 安全 `StyledText` 高亮（二轮将 `tokenize_query` 外提到每帧 1 次，空查询 Library/Projects 排序改借用 `Vec<&_>` 消除深拷贝）；右下角 5 秒轻量归档撤销条（`.occlude()` + `!modal_active()` + 悬浮窗避让 + 脱敏 + `was_viewing_active` 守卫，二轮将 `ArchiveUndoController` 压减至 ~30 行单一代际实现）；删死枚举 `Section::Search` 及 23 语种孤立键 `section.search`。
- **最终串行全量门禁与双主题实机 E2E**：
  - `cargo fmt --all -- --check`：0 diff；`check-ui-contracts.mjs`：24/24 自测 + 全库 PASS；
  - 9-Crate `cargo clippy --all-targets -- -D warnings`（含 `flowy-desktop`）：**0 error, 0 warning**（26.79s）；
  - 8-Crate 串行 `cargo test`：**654 / 654 PASS**（core 117 / agent 246 / studio 114 / skills 66 / app 56 / ui 46 / i18n 8 / shell 1；0 failed，5 ignored）；
  - 双主题实机 E2E（`ui-shot.ps1 -DataMode Clone`，`zh-CN`，`1280×820`）：Dark **32 / 32** + Light **31 / 31** 全绿（`ok=True, failures=0, log_errors=0, APPDATA_RESTORED, RUNNING_FLOWY=0`）；
  - 预览包 `C:\Users\38788\AppData\Local\FlowyPreview\flowy-app.exe`（`79,359,488` 字节，`SHA-256: 4171AC5CD9858DC50EEAF00B90273CE456FFE0B9983875F8049D233281F4C96A`）已同步。

# 交接给下一个 agent

> 写于 2026-11-24 16:30。这份文档的目的是让你**不用重新踩一遍我已经踩过的坑**。
> 先读第 1、5、9 节，其余的用到再查。

---

## 1. 一句话现状

**工作分支** `feat-flowy-local-receipt`，已 rebase 到 `origin/main`（`0d78e116b feat(flowy18): migrate to gpui-pre 0.3.14 + gpui-kit 0.11.0`）。
15 个提交，**只本地、未推送**。`flowy18_wip` 的「本地受理回执」纵线（V1）已打通并做过真实 UI 实测。
**下一步是 V0（`ExecInput` 类型层），但它卡在「第六条不变量从没写过」这个前置上。**

---

## 2. 仓库里有两个应用，别搞错

| | 根 workspace | **`flowy18_wip/`** |
| --- | --- | --- |
| 技术栈 | Tauri + React + `nomifun-*` + SQLite | **Rust + GPUI（`gpui-pre` + `gpui-kit`）** |
| 二进制 | `Flowy.exe` | **`flowy18_wip/target/debug/flowy-app.exe`** |
| 和本任务的关系 | **无关，一行没碰** | **全部工作在它下面** |

`docs/architecture/flowy-v0/` 里的架构方案描述的是 `flowy18_wip`。
目标书里提到的 `messages.status='pending'`、`conversation_delivery_receipts`、`bun run typecheck`
**都是根 workspace 的东西，`flowy18_wip` 里不存在**（它用 JSON 文件，没有 SQLite，也没有 TypeScript）。

---

## 3. 我到底改了什么

### 3.1 新增文件

| 路径 | 内容 |
| --- | --- |
| `flowy18_wip/crates/flowy-agent-core/src/acceptance.rs` | **受理队列**，追加式日志，约 300 行 |
| `docs/architecture/flowy-v0/`（11 份文档） | 从交接目录迁入的架构方案 ＋ PROGRESS ＋ BLOCKED ＋ 索引 |
| `docs/architecture/flowy-v0/evidence/` | 10 份实测证据（截图、真实回执日志） |

### 3.2 改动的既有文件

| 文件 | 改了什么 |
| --- | --- |
| `flowy-agent-core/src/lib.rs` | 导出 `AcceptanceQueue` / `AcceptedMessage` |
| `flowy-agent/src/handle.rs` | 新增 `accept_deferred` / `pending_acceptances`；**没动** `submit_message` 的 `agent_busy` |
| `flowy-agent/src/lib.rs` | 重导出 |
| `flowy-studio/src/composer.rs` | 忙时同时渲染「停止」＋「发送」；回车不再被 `running` 吞掉 |
| `flowy-studio/src/panel.rs` | 忙时走 `accept_deferred`；新增 `pending_receipts` 字段 ＋ 刷新；两条 `#[gpui::test]` |
| `flowy-studio/src/studio.rs` | 聊天页顶部渲染「排队中」条 ＋ 溢出计数 |
| `flowy-i18n/locales/*.json`（23 个） | 各加 2 行：`chat.pending_receipt`、`chat.pending_more` |

**代码约 600 行，其余是文档。**

### 3.3 核心设计

**追加式日志，不是快照。**

```
<store_root>/acceptance/journal.jsonl
{"event":"accepted","id":"accepted-1a208f4ef99-0","session_hint":"s-...","text":"...","at_ms":...}
{"event":"taken","id":"accepted-1a208f4ef99-0","at_ms":...}
```

- `pending()` 由重放派生
- 追加不读文件（但 `accept()` 为了生成不重复编号会重放一次全文，**这是已知性能债**）
- 尾部半行截断容忍（崩溃中断不会让下次启动失败）
- `session_hint` 是 `Option<String>`，**不是绑定** —— 受理时不知道归属，绝不能借机新建会话

---

## 4. 架构坐标：我在 V1，全程是七段路

方案 `docs/architecture/flowy-v0/最终方案.md` §12：

```
V0 地基 → V1 受理 → V2 调度 → V3 执行 → V4 产物 → V5 核验 → V6 外发
```

| 切片 | 验收 | 状态 |
| --- | --- | --- |
| V0 | `ExecInput` round-trip 字节相等 | ❌ **全仓零命中** |
| **V1** | 提交后立刻拿到编号，重启后仍在 | ✅ **完成并实测** |
| V2 | 对话不排队；显示「排队中」；不重复执行 | ⚠️ 「排队中」显示做了，**队列只写不读** |
| V3 | 同输入重放得到相同事件序列 | ⚠️ `run_turn` 在，没按这条验收过 |
| V4 | 一条指令产出三页 pptx | ⚠️ `artifact_store` 在，没跑过端到端 |
| V5 | 四类锚异常各有不同卡文案 | ❌ **全仓零命中** |
| V6 | 外发 | ❌ 没做 |

---

## 5. 踩过的坑（这一节最值钱）

### 5.1 环境类

**① 构建产物在 `build.noindex/`，不在 `target/debug/`**

`target/debug/flowy-app.exe` 可能是**别处放过来的旧副本**，时间戳会骗人。

> **判据：不看时间戳，`grep -c "特征串" target/debug/flowy-app.exe`。**
> 我被这个坑害了三次，其中一次白测一整轮。

**② 构建的包选错**

`flowy-app` 是**库**。二进制在 `flowy-desktop`：

```bash
cargo build -p flowy-desktop --bin flowy-app
```

**③ exe 被运行中的实例锁住**

Windows 下必须先杀，否则 `failed to remove file ... os error 5`：

```bash
taskkill /F /IM flowy-app.exe
powershell.exe -NoProfile -Command "Get-Process flowy-app -ErrorAction SilentlyContinue | Stop-Process -Force"
```

**④ 应用数据根是嵌套的**

不是 `AppData/Roaming/Flowy`，是 **`AppData/Roaming/Flowy/Flowy/data`**。
找错目录会以为文件没生成。

**⑤ 项目日志是 INFO 级别**

我加的探针全是 `tracing::debug!`，**不会落盘**。
「日志里什么都没有」**不能**反推「代码没执行」。

**⑥ `pgrep` 在这个 MSYS 环境不存在**

脚本判进程存活要用 `ps aux | grep -c "[f]lowy-app"`。
**命令不存在和条件为假，表现是一样的。**

### 5.2 桌面自动化（UIA）类

**⑦ `desktop_set_value` 对 GPUI 无效**

`ValuePattern.SetValue` 返回 `succeeded`，但回读 `value` 仍是空、按钮仍 `disabled`。
`focus` 也不行 —— textfield 的 `available_actions` 只有 `SetValue`，没有 `SetFocus`。

**唯一可行路径**：

```bash
agent-desktop clipboard-set "文本"
agent-desktop focus-window --window-id $W
agent-desktop mouse-click --xy <绝对X>,<绝对Y> --headed
agent-desktop press ctrl+v --window-id $W
```

**⑧ 坐标随布局变，必须每次重取**

输入框 y 在首页是 1048，在有内容的聊天页是 1449。
绝对坐标 = 窗口原点（`list-windows` 的 x/y）＋ 元素 bounds。

**⑨ `press` 要求窗口在前台**

不在前台返回 `STALE_REF`（`is_focused: false`）。先 `focus-window`。

**⑩ 无障碍树查询：`-i` 会隐藏状态文本**

`-i` 只保留可交互节点。「排队中」是 `role(Status)` 的静态文本，被过滤掉。

> **判据：查按钮用 `-i`，查状态文本用 `--surface window`。**

### 5.3 脚手架类

**⑪ 反引号在 `python -c "..."` 里会被 shell 执行**

我犯了 **4 次**。任何含反引号的补丁**必须写成脚本文件**再执行。

**⑫ `str.replace` 锚点不匹配时会静默无操作**

每次替换都要 `assert old in s` ＋ `assert s != before`。

### 5.4 方法类（最贵的三个）

**⑬ 「工具看不见 ≠ 东西不存在」—— 我犯了三次**

- `-i` 过滤掉 Status 元素 → 以为没渲染
- 时间戳骗人 → 以为二进制是旧版
- 无障碍树没有名字 → 以为溢出行没渲染（截图里明明在）

**每次都要交叉验证：无障碍树 ＋ 截图 ＋ 文件系统。**

**⑭ 回归测试必须验证「会红」**

把 bug 临时放回去跑一遍，确认测试真的失败。**不会红的回归测试等于没写。**

**⑮ 我测的全是「我想到的那条路」**

第一次验证 L3 校准时，样本标题里泄露了标签，得出假的 0% 误判率。
写测试前问三件事：① 被测对象里有没有我放进去的提示 ② 非法输入测了吗 ③ 走的是真实入口还是我顺手搭的专用路径。

### 5.5 gpui-pre 迁移（2026-11-24 新坑）

主分支 `0d78e116b` 把 `gpui-ce` 换成了 **`gpui-pre` 0.3.14 + `gpui-kit` 0.11.0**。必读
`flowy18_wip/docs/gpui-pre-and-kit.md`。我 rebase 后踩到四个：

| 旧 | 新 |
| --- | --- |
| `state.emplace(text, cx)` | `self.set_prompt_text(text, cx)`（入队，render 顶部排空） |
| `state.as_str()` | `state.value()` |
| `.aria_live(...)` | **删除，无替代** |
| `StudioPanel::new(config, ui, cx)` | `StudioPanel::new(config, ui, window, cx)`；测试用 `open_panel(cx, \|window, cx\| ...)` |

**`.aria_label()` 还在**（文档说删了，实测能编译）。
**`.set_value()` 不发 `InputEvent`** —— 「写值 → 等事件 → 落盘」会静默失效。

---

## 6. 关键决策与理由

**① 为什么不改 `submit_message` 的 `agent_busy`**

既有语义，被 `handle.rs` 的测试锁着，别处可能依赖它防重。**新增独立入口 `accept_deferred`**，它唯独不返回 `agent_busy`。

**② 为什么日志和 `sessions/history.json` 分开放**

队列在 turn 运行中写，索引在 turn 边界写。共用一个文件是白送的写竞争。

**③ 为什么从快照改成追加日志**

`pending.json` 每次整份重写，只能答「现在还有什么」，答不了「早先发生过什么」——**恰恰不能做日志追溯**。
方案 §4 自己也写：「这是事件日志格式……**要落盘可追查**」。

**④ 为什么受理不建会话**

原本调 `ensure_active_session()`，它在没有 live 会话时会 `create_session()`。
后果是全新启动后提交，回执被钉在一个**刚新建的空会话**上。
方案 §4 硬约束 2 原话：「`session_hint` 是优化不是依赖」。

**⑤ 为什么删 `notice.task_added`**

它说「已加入本次会话的任务列表」，但仓库里没有这个列表。
**用户的原话只剩在 jsonl 里，界面上再也回不去。**

---

## 7. 未完成清单

来自老爷爷（独立审查者）的判定 `needs-fix`，以及我自己的核对：

| # | 事项 | 说明 |
| --- | --- | --- |
| 1 | ~~写死「第六条不变量」~~ | **已完成**：方案 §4 已改为「六条硬约束」，第六条是 `decided` 作用域（只对当时那件事有效，不跨 work/轮次）。 |
| 2 | **V0：`ExecInput` 类型层** | 零命中。范围：类型 ＋ serde ＋ round-trip ＋「receipt → ExecInput」派生函数。**不要顺手做领取/执行** |
| 3 | **`take()` 原子性** | 现在是 replay→find→append，**无锁无 CAS**。注释写「两个调用者不会跑同一段文本」是假的 |
| 4 | **`taken` 改租约** | 现在是永久布尔：先落 `taken` 再执行，中间崩溃 → 消息不在 pending、永远不跑、没有错误。方案要「至少一次」，实现成「至多一次可能零次」 |
| 5 | **V2 队列消费** | `take()` **零生产调用方**，队列只写不读 |
| 6 | **`accept()` 每次全文重放** | 方案 §5.7 要求忙时提交 50ms 出编号，10⁵ 行就过不了 |
| 7 | ~~文档陈旧~~ | **已修**：`PROGRESS.md` 的历史段落已就地标注「旧实现，任务 7 已改为追加日志」；`BLOCKED.md` 的 `gpui-ce` 已更正为 `gpui-pre` + `gpui-kit` |

**排在第一位的是 #1，不是 #2** —— 不写死第六条，V0 的断言就是凑数。

---

## 8. 待裁决（必须问需求方，不能替拍板）

**改动目标算不算数？**

我按 `flowy18_wip` 做的，根 workspace 一行没碰。
`BLOCKED.md` 里原话是「我替领导拍的板」，老爷爷审查时说：

> 这个得需求方说一次 —— 现在「需求 A/B/C 达成」只在 `flowy18_wip` 成立，根产品改动文件里没有它。

---

## 9. 可重跑命令（全部亲手跑过）

```bash
cd .agents/worktrees/feat-flowy-local-receipt

# 编译
cd flowy18_wip
cargo check -p flowy-agent-core -p flowy-agent -p flowy-studio -p flowy-i18n

# 测试（245 个）
cargo test -p flowy-agent-core   # 56 passed
cargo test -p flowy-agent        # 103 passed
cargo test -p flowy-studio       # 78 passed
cargo test -p flowy-i18n         # 8 passed

# 一条命令验完纵线
cd .. && bash docs/architecture/flowy-v0/accept.sh
WITH_APP=1 bash docs/architecture/flowy-v0/accept.sh   # 含真实启动

# 构建二进制（必须先杀进程）
taskkill /F /IM flowy-app.exe
cargo build -p flowy-desktop --bin flowy-app

# 启动（唯一允许的方式；不要用 PowerShell Start-Process，会弹签名确认）
cd flowy18_wip && (./target/debug/flowy-app.exe >/tmp/flowy.log 2>&1 &)
```

**回执文件**：`C:/Users/38788/AppData/Roaming/Flowy/Flowy/data/acceptance/journal.jsonl`

---

## 10. 当前未提交的改动

**代码（rebase 后为适配 gpui-pre，已编译、已测试 245 全绿、已实测）：**

```
flowy18_wip/crates/flowy-studio/src/panel.rs   emplace → set_prompt_text；测试用 open_panel
flowy18_wip/crates/flowy-studio/src/studio.rs  去掉 aria_live（gpui-pre 已删除该 API）
```

**文档（知识收尾时校正）：**

```
BLOCKED.md                  技术栈 gpui-ce → gpui-pre 0.3.14 + gpui-kit 0.11.0
PROGRESS.md                 两处历史描述就地标注「旧实现」
交接给下一个agent.md        第 7 项标记为已修
```

**建议先把这 5 个文件提交，再开新活。**

---

## 11. 提交规矩（项目硬约束）

- **只在分支上 commit，不推送**
- **绝不带 AI 署名**（AGENTS.md 强制），用 `-c user.name="RiKa0-0" -c user.email="2206491416@qq.com"`
- Conventional Commits 风格
- 改完代码**不要立刻提交**，等需求方明确说「提交」

---

## 12. 给执行者的下一份任务书（可直接粘贴 `/goal`）

```
在 worktree feat-flowy-local-receipt 上，先提交现有两处 gpui-pre 适配，再补 V0。

先做（半小时）：
1. git status 确认只有 flowy-studio/src/{panel,studio}.rs 两个文件已改；提交它们。
2. 读 docs/architecture/flowy-v0/PROGRESS.md 的「任务 8」「老爷爷审查结论」两节。

然后做（主线）：
3. 在 docs/architecture/flowy-v0/最终方案.md 的 §4 找到「五条硬约束」，
   在 §12 找到「六条不变量逐条写成断言」。第六条没有内容。
   先在 最终方案.md 里把第六条写死，再往下做。写不成第六条就停下来报告原因。
4. 把 ExecInput 落成 Rust 类型（含 decided），放 flowy-agent-core，
   写 round-trip 单测，把六条不变量逐条写成断言。
   规模控制在 120 行左右。不要顺手做领取/执行路径。
5. 加一条「receipt → ExecInput」的派生函数及测试。
   receipt 里不许出现 token / capabilities / revision；ExecInput 单向带 receipt_id。

验收（全部要亲手跑过并把输出贴进 PROGRESS.md）：
- cargo test -p flowy-agent-core 全绿，且测试数 ≥ 56
- cargo check -p flowy-agent-core -p flowy-agent -p flowy-studio 零 error
- 六条不变量每条都有对应断言，逐条列出测试名

禁区：
- 不改根 workspace（crates/、ui/、apps/）任何文件
- 不改 submit_message 的 agent_busy 语义
- 不加新依赖
- 不推送，只本地 commit，不带 AI 署名

踩坑备忘（不要重新踩）：
- 二进制在 flowy-desktop 包，不是 flowy-app；准确命令 cargo build -p flowy-desktop --bin flowy-app
- 判断重新编译是否生效，用 grep -c "特征串" 二进制，不要看时间戳
- 构建前先 taskkill /F /IM flowy-app.exe，否则 os error 5
- 应用数据根是 AppData/Roaming/Flowy/Flowy/data（嵌套两层）
- 日志是 INFO 级别，tracing::debug! 不落盘
- 这个 MSYS 没有 pgrep
- 反引号在 python -c "..." 里会被 shell 执行，补丁必须写成脚本文件
- 回归测试必须验证「会红」：把 bug 放回去跑一遍

遇到卡住：写进 docs/architecture/flowy-v0/BLOCKED.md 再继续别的，不要硬闯。
```

---

## 13. 文档在哪

| 路径 | 内容 |
| --- | --- |
| `docs/architecture/flowy-v0/最终方案.md` | 架构方案（2167 行，修订 9.1）。**要动手只读这份** |
| `docs/architecture/flowy-v0/PROGRESS.md` | 我踩的每个坑、每轮实测结果。**接手先读它** |
| `docs/architecture/flowy-v0/BLOCKED.md` | 卡住的事、待裁决 |
| `docs/architecture/flowy-v0/evidence/` | 10 份实测证据 |
| `docs/architecture/flowy-v0/accept.sh` | 一条命令验完纵线 |
| `flowy18_wip/docs/gpui-pre-and-kit.md` | **写任何 GPUI 代码前必读** |
| `flowy18_wip/AGENTS.md` | 项目规则 |

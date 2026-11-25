# 交接给下一个 agent

> **现役唯一入口（2026-11-25 更新）**：**直接先读 §15（现状、不可回退口径、任务 14–18 实施计划）**，再查 §5（15 条踩坑实录）与 §9（常用命令）。
> §1–§14 保留 2026-11-24～25 演进底稿作历史与踩坑备查；凡状态描述与 §15 不一致处，**一律以 §15 为准**。

---

## 1. 一句话现状（已由 §15 接管更新）

**工作树** `.agents/worktrees/feat-flowy-local-receipt`（分支 `feat-flowy-local-receipt`，根仓库在 `main`，切勿搞混），已 rebase 到 `origin/main`（领先 34、落后 0）。
最新已提交 `03a69348f docs(flowy-v0): record the delivery gate and its trace`（代码最新提交 `4e35a1580`）；**只本地、未推送**。
V0（`ExecInput`）、V1（追加日志受理）、V2/S3b（租约泵 + 会话级队列条）、V3（`ExecInput` 执行＋重放）、V4（`create_deck`）、V5（`verify_deck` 四码）与 `.pptx` 交付检查门均已落。
实跑基线：core 73 / agent 124 / studio 82 / i18n 8；`accept.sh` 3/3；`clippy --lib` 零警告（注：`--all-targets` 在 `verify.rs:296` 有 1 处 `assert_is_empty` 遗留，任务 14 一并清零）。
**下一步实施路线（任务 14–18，V6 停）见 §15.3 与 `最终方案.md` §12.1。**

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

> 本节写于 2026-11-24，只覆盖 V1 阶段；V3/V4/V5 与交付门见 §15。

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

> 本表写于 2026-11-24；V0 等条目已完成，以 §1 与 `PROGRESS.md` 为准。

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

# 测试（287 个）
cargo test -p flowy-agent-core   # 73 passed
cargo test -p flowy-agent        # 124 passed
cargo test -p flowy-studio       # 82 passed
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

> 实时工作区状态以 §15 导语及 `git -C .agents/worktrees/feat-flowy-local-receipt status` 为准（代码工作树干净，`HEAD` 为 `03a69348f`；本轮仅整理 `docs/architecture/flowy-v0/` 文档与方案）。

---

## 11. 提交规矩（项目硬约束）

- **只在分支上 commit，不推送**
- **绝不带 AI 署名**（AGENTS.md 强制），用 `-c user.name="RiKa0-0" -c user.email="2206491416@qq.com"`
- Conventional Commits 风格
- 改完代码**不要立刻提交**，等需求方明确说「提交」

---

## 12. 给执行者的下一份任务书（可直接粘贴 `/goal`）

> 这份任务书写于 2026-11-24，已过时；当前剩余事项见 §14。

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

---

## 14. 交付硬门与 trace（2026-11-25，已并入 §15 校准）

| 项 | 现状 |
| --- | --- |
| 代码 | `flowy18_wip/crates/flowy-agent/src/delivery.rs`；报告/事件落 `<store_root>/delivery/reports.jsonl`、`events.jsonl`（append-only） |
| 门 | `verify_deck` 写 `reports.jsonl` → `publish_file` 当前对 `.pptx` 调只读 `gate.check()` 查报告与 sha256；无报告 / `ok=false` / 哈希不符分别拒绝入库 |
| `DeliveryStore::deliver()` | 单测已验：被拒不调 sender，事件只有 `deliver_blocked`；override 显式传入，事件序 `deliver_blocked → deliver_override → deliver`；`tracing` target `flowy.delivery` |
| **当前实现缝隙（重要）** | `publish_tool.rs:96` 目前调的是只读 `gate.check()` 而非 `gate.deliver()`，因此从 `publish_file` 普通入口被拒时**尚未向 `events.jsonl` 写入 `deliver_blocked`**（任务 14 与全类型硬门一并收口） |
| 已验证 | core 73 / agent 124 / studio 82 / i18n 8；`accept.sh` 3/3；`clippy --lib` 干净（`--all-targets` 有 `verify.rs:296` 1 处待清）；证据 `证据.md` §11.25 |
| 未做 | 全类型硬门 + 普通入口落 `events.jsonl`（任务 14）、override UI（任务 15）、核验结论卡（任务 16）、崩溃窗口（任务 17）、L3 提示（任务 18）、V6 外发（单向门，停等拍板） |

**排查一次交付时**：任务 14 落地前先看 `reports.jsonl` 同路径最新行的 `ok` / `issues` 与 `tracing::warn!` 日志；任务 14 打通普通入口审计后，先在 `events.jsonl` 按 `artifact_hash` 或 `delivery_id` 查 `deliver_blocked` 的 `code`，再核对 `reports.jsonl`。

---

## 15. 2026-11-25 现役总账：Phase 1–4 全部收口，停在 V6 外发单向门

> **本节是唯一现役真相，覆盖 §1–§14 的历史状态**；旧节保留作踩坑实录。
> - **注意工作树位置**：根工作区 `d:/workSpace/git_clone_test/allo` 在 `main`，不含本目录；全部代码与文档均在独立 worktree **`.agents/worktrees/feat-flowy-local-receipt`**（分支 `feat-flowy-local-receipt`）。
> - **Git 状态**：V0–V5 及 Phase 1–4（任务 14–18）代码与文档已全部在本地分支提交（**只本地、未推送**）；现停在 **V6 外发通道**（`BLOCKED.md` §5 单向门）等待需求方拍板。
> - 旧段落若残留 rebase 前哈希，一律以本节表格为准。

### 15.1 已完成切片账本（rebase 后现役哈希）

| 切片 | 提交 | 关键文件 | 一句话 |
| --- | --- | --- | --- |
| V0 地基与第六条不变量 | `66266058a` / `384ead805` | `最终方案.md` §4、`flowy-agent-core/src/exec_input.rs` | 写死 `decided` 作用域；`ExecInput` round-trip 与单向 `from_receipt` 断言齐备 |
| V1–V2 租约队列与会话路由 | `2d1a65278`..`653e6d4fd` | `flowy-agent-core/src/acceptance.rs`、`flowy-agent/src/handle.rs` | 追加式日志、租约互斥、10 次重试死信、启动即排水、按 `session_hint` 回原会话 |
| S3b 会话级队列条 | `be02dff9a` | `flowy-studio/src/panel.rs`、`studio.rs` | 队列条标注「本会话 / 会话 xxxxxxxx / 主时间线」（23 locale） |
| V3 ExecInput 执行与重放 | `679fb1acb` | `exec_input.rs`、`engine.rs`、`handle.rs` | prompt 唯一来自 `ExecInput`；同一输入重放得相同归一化回调序列与历史形状 |
| V4 环境感知产物 | `94caeb511` | `flowy-agent/src/pptx.rs` | `create_deck` 生成并读回校验页数；`FLOWY_PPTX=missing` 返回 `status:"missing"` 不失败 |
| V5 锚核验 | `c693b0ab1` | `flowy-agent/src/verify.rs` | 先 L1 页数后形状名核验 + 四码聚合去重；`verify_deck` 工具落报告 |
| 交付硬门初版 | `4e35a1580` / `03a69348f` | `flowy-agent/src/delivery.rs`、`publish_tool.rs` | `DeliveryStore` 报告/哈希三拒绝与事件留痕；`publish_file` 拦 `.pptx` 入库 |
| 巡检与计划对齐 | `d1632af92` | `V0-执行计划.md`、`证据.md` §11.26、`BLOCKED.md` | 真机巡检定位三大 P0 断层，对齐四阶段执行计划与方案 A 拍板结论 |
| **Phase 1 + Phase 2 收口** | **`5e96fb681`** | `handle.rs`、`panel.rs`、`pptx.rs`、`verify.rs`、`delivery.rs`、`publish_tool.rs` | 消除启动误排 `mock`、跨会话结案隔离、三工具 `output_root`+`path_guard` 统一、全类型硬门+方案 A 豁免表、`deliver_intent` 崩溃检测、`INFO` 业务 Trace |
| **Phase 3 + Phase 4 收口** | **`599b82c35`** | `semantic_verify.rs`、`projection.rs`、`feed.rs`、`library/*`、`flowy-app`、23 `locales/*.json` | §8.3 核验结论卡、未勾选禁点 Override、聊天与资料库永久徽标、L3 字面简报提示层与默认 `INFO` 落盘脱敏终验 |

**亲手复跑终验基线（2026-11-25，详见 [`证据.md` §11.27](证据.md)）**：`flowy-agent-core` **74** passed（2 ignored）/ `flowy-agent` **137** passed（1 ignored）/ `flowy-studio` **86** passed / `flowy-app` **50** passed / `flowy-i18n` **8** passed；`WITH_APP=1 accept.sh` **4/4**；`bun flowy18_wip/scripts/check-ui-contracts.mjs` 通过；`cargo clippy -p flowy-ui -p flowy-studio -p flowy-app -p flowy-i18n -p flowy-agent --all-targets -- -D warnings` **0 警告**。

### 15.2 六条不可回退的口径

1. **路由 A 与启动门控**：跟随提交时所在会话；忙或云端模型目录未就绪时入队等待，绝不擅自另开空会话，也绝不在 `cloud_ready=false` 时把队列排进 `mock/echo`。
2. **重放口径**：比对归一化 `TurnObserver` 回调序列 + 落盘历史 `(role, content)` 形状；不比时间戳、随机 id、工具产物字节。
3. **工具按环境与同源路径**：缺能力（`FLOWY_PPTX=missing`）返回 `status:"missing"` 成功结果；`create_deck` / `verify_deck` / `publish_file` 统一优先按 `output_root` 解析并受 `path_guard` 约束。
4. **核验四码与 L3 边界**：`ANCHOR_MISSING` / `ANCHOR_DUPLICATE` / `ANCHOR_INVALID` / `VALUE_MISMATCH`（外加 L1 `STRUCTURE`）；L3 语义复核严格遵守字面简报规则与 `Value::Bool` 校验，**只出卡片提示、绝不阻断硬门**。
5. **全类型交付硬门语义**：所有 `publish_file` 必经 `DeliveryStore::deliver_with`；无报告（`DELIVERY_NOT_VERIFIED`）/ `ok=false`（`DELIVERY_VERIFICATION_FAILED`）/ 哈希不符（`DELIVERY_ARTIFACT_CHANGED`）三拒绝（入库 0 次并写 `deliver_blocked`）；已知无核验器类型按方案 A 写 `deliver_exempt`（`EXEMPT_NO_VERIFIER_V0`）；入库前落 `deliver_intent` 以识别崩溃 `InDoubt`。
6. **Override 永久留痕**：`ok=false` 隐藏默认保存/交付按钮；必须显式勾选默认未选的确认框才放行，且对话卡片与资料库列表项永久保留「未过核验强制放行」徽标。

### 15.3 当前止损点与后续路线

1. **Phase 1–Phase 4（任务 14–18）**：✅ **已全部完成并验收**（`5e96fb681`、`599b82c35`）。
2. **V6 · 外部通道发送（SMTP / Webhook / 企业微信 / 飞书 / Discord）**：**🔴 停等需求方拍板**（见 [`BLOCKED.md` §5](BLOCKED.md)）—— 需明确外部通道类型、凭据存储方案（系统凭据库 vs 环境变量）、收件人白名单与 `submitted / failed / bounced / no_bounce` 四态回执接法（`最终方案.md` §8.5–§8.6）后再启动下一单。
3. **开发提速约定（需求方 2026-11-25 指示）**：开发阶段直接批量编码、编译构建并跑最小定向测试，遇到问题当场解决；**不必每个子阶段都跑全量测试矩阵**，待全部改动收口后再统一跑一次全量门禁。

### 15.4 环境坑与仓库硬约束（7 条）

1. **串行跑 Cargo**：绝不并发跑多个 `cargo`（抢 `build.noindex` 锁）；矩阵顺序：`flowy-agent-core` → `flowy-agent -- --test-threads=1` → `flowy-studio` → `flowy-i18n`。
2. **Windows 格式化**：在 `flowy18_wip/` 下对改动文件跑 `rustfmt --edition 2024 <文件>`，不要在根或超长路径直接跑 `cargo fmt --all`。
3. **偶发文件锁**：Windows 清理临时目录偶发 `AlreadyExists` / `PermissionDenied`，单独重跑确认，不要误改业务被测逻辑。
4. **Git 纪律**：提交身份固定 `-c user.name="RiKa0-0" -c user.email="2206491416@qq.com"`，严禁任何 AI 署名或 `Co-authored-by`，Conventional Commits，**只本地 commit、不 push**；未获明确指令前不自动 commit。
5. **GPUI 铁律**：写界面代码前必读 `flowy18_wip/docs/gpui-pre-and-kit.md`；UI 变更后跑 `bun flowy18_wip/scripts/check-ui-contracts.mjs` 并用 `agent-desktop` 实测。
6. **PowerShell 下的 `bash` 陷阱**：Windows PowerShell 裸跑 `bash` 会命中 `C:\Windows\System32\bash.exe`（WSL Relay）报 `execvpe(/bin/bash) failed`；必须显式调 Git Bash：`"D:\Git\bin\bash.exe" docs/architecture/flowy-v0/accept.sh`。
7. **CI `--all-targets` Pedantic Clippy 门禁**：`.github/workflows/flowy18-ui.yml` 对 `flowy-agent` 等五 crate 检查 `--all-targets -- -D warnings`（含单测里的 `clippy::assert_is_empty`、`default_trait_access`）；只跑 `--lib` 会漏掉测试代码里的 lint 错误。

### 15.5 下一单任务书（Phase 1 + Phase 2 核心门禁收口，完整四阶段见 [`V0-执行计划.md`](V0-执行计划.md)）

```text
【目标】按 docs/architecture/flowy-v0/V0-执行计划.md 推进 Phase 1（堵住真机三大 P0 断层 + INFO 业务 Trace）与 Phase 2（全类型交付硬门 + 方案 A 显式豁免表 + deliver_intent 崩溃窗口 + 普通入口审计落盘），不碰根工作区，不碰 V6 外发。

【工作区】独立 worktree .agents/worktrees/feat-flowy-local-receipt（分支 feat-flowy-local-receipt）。先读 V0-执行计划.md、证据.md §11.26、BLOCKED.md、最终方案.md §6.4/§12.1/§13.1。

【现状】HEAD 03a69348f。基线：core 73 / agent 124 / studio 82 / i18n 8；accept 3/3。真机巡检（证据.md §11.26）暴露：① 登录态模型目录未到达时启动排水把 journal.jsonl 排进 mock/echo；② AcceptedClaim 未绑 session_id 致跨会话误结案且污染 Home active；③ create_deck/verify_deck 用 cwd 而 publish_file 用 output_root 且缺 path_guard 与路径规范化；④ publish_tool 仅拦 .pptx 且只读 check 不写 events.jsonl，默认 INFO 零业务日志，verify.rs:296 有一处 clippy::assert_is_empty。

【任务】
1. Phase 1（P0 修复 + Trace）：修复 verify.rs:296 clippy；统一 create_deck/verify_deck 优先按 output_root 解析并纳入 handle.rs path_guard，delivery.rs 规范化路径键，Home 系统提示词补 create_deck -> verify_deck -> publish_file；AcceptedClaim 绑定 session_id 独立判终态，保护 Home active 不被后台排水篡改；云端目录加载中挂起排水、未就绪时手动发送入 accept_deferred；补齐 INFO target="flowy.acceptance"|"flowy.verify"（含 root_session_id/session_id/receipt_id/attempt_id）。
2. Phase 2（全类型硬门 + 崩溃窗口）：去掉 publish_tool.rs 的 .pptx 特判，统一经 DeliveryStore 包裹 ArtifactStore::prepare；实现方案 A 显式豁免表（csv/pdf/md/txt/json/html -> EXEMPT_NO_VERIFIER_V0），未知扩展名与未核验 .pptx 阻断并写 deliver_blocked（prepare 调用 0 次）；落盘 deliver_intent -> deliver 以识别 in_doubt，打 INFO target="flowy.delivery"。
3. 补齐先红后绿回归单测（覆盖启动不误排 echo、会话切换不误结案、三工具同轮贯通、.. 越界拦截、非豁免无报告被拒写 events.jsonl、豁免留痕、篡改哈希被拒、崩溃 in_doubt），同步更新 PROGRESS.md 与 证据.md。
4. 在 flowy18_wip 下串行跑满 8 条门禁：
   - cargo test -p flowy-agent-core
   - cargo test -p flowy-agent -- --test-threads=1
   - cargo test -p flowy-studio
   - cargo test -p flowy-i18n
   - "D:/Git/bin/bash.exe" ../docs/architecture/flowy-v0/accept.sh
   - cargo clippy -p flowy-ui -p flowy-studio -p flowy-app -p flowy-i18n -p flowy-agent --all-targets -- -D warnings
   - rustfmt --edition 2024 --check <全部改动 .rs 文件>
   - bun scripts/check-ui-contracts.mjs

【验收】8 条门禁全绿；单测较基线净增 ≥ 7 条；反向验证注释门禁时新增测试必红；后续紧接 Phase 3（核验卡 + Override UI）与 Phase 4（L3 提示层 + agent-desktop 真机终验）。
```

### 15.6 接手者第一小时检查清单

1. 进 `.agents/worktrees/feat-flowy-local-receipt`，读 [`V0-执行计划.md`](V0-执行计划.md) + [`证据.md` §11.26](证据.md) + [`BLOCKED.md`](BLOCKED.md)。
2. `git log --oneline -8`、`git branch --list "*flowy*"` 与 `git status` 核对分支、HEAD 与未提交文件。
3. 串行跑一遍 §15.5 第 4 步基线门禁（注意 PowerShell 下用 `"D:\Git\bin\bash.exe"` 跑 `accept.sh`，真机测试务必先备份并在 `finally` 还原 `%APPDATA%\Flowy\Flowy\data`）。
4. 按 [`V0-执行计划.md`](V0-执行计划.md) Phase 1 → Phase 4 依序推进；遇到需拍板边界先查/写 `BLOCKED.md`，绝不猜着碰 V6 外发或根工作区。

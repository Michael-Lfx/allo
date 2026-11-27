# 交接给下一个 agent

> **现役唯一入口（2026-11-27 修订 9.19 更新）**：**直接先读 §27 与 §27.1（`origin/main@3e66290a9`：PR #512 与 `#514–#528` 十六连 PR 主干全账、Wave 2 五大核心模块目录映射、`dispatch_receipt_id` 完结、`persona.rs` 暂缓与在研 worktree 协同边界）**，再按需查 §26（PR #505–#511 合流基线与 S5/安全/IO/唤醒队列收口）、§25（PR #507 假评分下线、Dot 分阶段气泡与单条引用 703/703 基线）、§24（PR #504 桌面壳可靠化与类 GPT Dot 净化）、§23（PR #503 七项修复）、§22（PR #498 合入 + 修订 9.13 真云端 UX 收口）、§21（PR-2B Router UI 接线）、§19（PR-2A 冻结 `AgentHandle` 契约表）、§20（PR #497 看板/资料库/快搜收口）、[`../router-work-and-tool-extension.md`](../router-work-and-tool-extension.md)（PR #523 Router/Work 工具扩展架构规范）、§5（环境与 UIA 踩坑实录）、§15.4（仓库硬约束）与 [`gpui-pre-and-kit.md`](../../gpui-pre-and-kit.md)。
> §1–§26 保留 2026-11-24～27 演进底稿作历史与契约备查；凡分支/合流状态与 §27–§27.1 不一致处，**一律以 §27–§27.1 为准**。

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

`flowy18_wip/docs/architecture/flowy-v0/` 里的架构方案描述的是 `flowy18_wip`。
目标书里提到的 `messages.status='pending'`、`conversation_delivery_receipts`、`bun run typecheck`
**都是根 workspace 的东西，`flowy18_wip` 里不存在**（它用 JSON 文件，没有 SQLite，也没有 TypeScript）。

---

## 3. 我到底改了什么

> 本节写于 2026-11-24，只覆盖 V1 阶段；V3/V4/V5 与交付门见 §15。

### 3.1 新增文件

| 路径 | 内容 |
| --- | --- |
| `flowy18_wip/crates/flowy-agent-core/src/acceptance.rs` | **受理队列**，追加式日志，约 300 行 |
| `flowy18_wip/docs/architecture/flowy-v0/`（11 份文档） | 从交接目录迁入的架构方案 ＋ PROGRESS ＋ BLOCKED ＋ 索引 |
| `flowy18_wip/docs/architecture/flowy-v0/evidence/` | 10 份实测证据（截图、真实回执日志） |

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

方案 `flowy18_wip/docs/architecture/flowy-v0/最终方案.md` §12：

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
cd .. && bash flowy18_wip/docs/architecture/flowy-v0/accept.sh
WITH_APP=1 bash flowy18_wip/docs/architecture/flowy-v0/accept.sh   # 含真实启动

# 构建二进制（必须先杀进程）
taskkill /F /IM flowy-app.exe
cargo build -p flowy-desktop --bin flowy-app

# 启动（唯一允许的方式；不要用 PowerShell Start-Process，会弹签名确认）
cd flowy18_wip && (./target/debug/flowy-app.exe >/tmp/flowy.log 2>&1 &)
```

**回执文件**：`<APPDATA>/Flowy/Flowy/data/acceptance/journal.jsonl`

---

## 10. 当前未提交的改动

> 实时工作区状态以 §15 导语及 `git -C .agents/worktrees/feat-flowy-local-receipt status` 为准（代码工作树干净，`HEAD` 为 `03a69348f`；本轮仅整理 `flowy18_wip/docs/architecture/flowy-v0/` 文档与方案）。

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
2. 读 flowy18_wip/docs/architecture/flowy-v0/PROGRESS.md 的「任务 8」「老爷爷审查结论」两节。

然后做（主线）：
3. 在 flowy18_wip/docs/architecture/flowy-v0/最终方案.md 的 §4 找到「五条硬约束」，
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

遇到卡住：写进 flowy18_wip/docs/architecture/flowy-v0/BLOCKED.md 再继续别的，不要硬闯。
```

---

## 13. 文档在哪

| 路径 | 内容 |
| --- | --- |
| `flowy18_wip/docs/architecture/flowy-v0/最终方案.md` | 架构方案（2167 行，修订 9.1）。**要动手只读这份** |
| `flowy18_wip/docs/architecture/flowy-v0/PROGRESS.md` | 我踩的每个坑、每轮实测结果。**接手先读它** |
| `flowy18_wip/docs/architecture/flowy-v0/BLOCKED.md` | 卡住的事、待裁决 |
| `flowy18_wip/docs/architecture/flowy-v0/evidence/` | 10 份实测证据 |
| `flowy18_wip/docs/architecture/flowy-v0/accept.sh` | 一条命令验完纵线 |
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
> - **注意工作树位置**：根工作区 `<REPO_ROOT>` 在 `main`，不含本目录；全部代码与文档均在独立 worktree **`.agents/worktrees/feat-flowy-local-receipt`**（分支 `feat-flowy-local-receipt`）。
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
| **真云端联调问题闭环** | **`5dd61886e`** | `pptx.rs`、`verify.rs`、`semantic_verify.rs`、`handle.rs` | 暴露 `create_deck` 锚点名、调优云端 L3 语义复核（实测 `passed` 16.8s）、核验卡去重并紧贴交付卡上方 |
| **Stage A–B 引擎深度加固与对抗验收** | **`89ee4aadd`** | `verify_contract.rs`、`verifier_registry.rs`、`verify.rs`、`delivery.rs`、`conversation_artifacts.rs`、`publish_tool.rs`、`handle.rs` | 根治 §6.6 自出题漏洞、可插拔 `.pptx/.docx/.xlsx` 确定性核验与 `verify_artifact`、`InDoubt` 启动探测、封堵 `conversation_artifacts` 绕门后门、拉平 `publish_file` 路径前缀 |

**亲手复跑终验基线（2026-11-25，详见 [`证据.md` §11.27–§11.29](证据.md)）**：`flowy-agent-core` **74** passed（2 ignored）/ `flowy-agent` **148** passed（1 ignored）/ `flowy-studio` **86** passed / `flowy-app` **50** passed / `flowy-i18n` **8** passed；`accept.sh` **3/3**（`WITH_APP=1` 4/4）；`bun flowy18_wip/scripts/check-ui-contracts.mjs` 通过；`cargo clippy -p flowy-ui -p flowy-studio -p flowy-app -p flowy-i18n -p flowy-agent --all-targets -- -D warnings` **0 警告**；真云端正向 + 对抗实测全通过（`evidence/15–21`）；`git merge-tree HEAD feat/flowy-super-ui` Rust 代码 **0 冲突**。

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
6. **PowerShell 下的 `bash` 陷阱**：Windows PowerShell 裸跑 `bash` 会命中 `C:\Windows\System32\bash.exe`（WSL Relay）报 `execvpe(/bin/bash) failed`；必须显式调 Git Bash：`"<GIT_BASH>" flowy18_wip/docs/architecture/flowy-v0/accept.sh`。
7. **CI `--all-targets` Pedantic Clippy 门禁**：`.github/workflows/flowy18-ui.yml` 对 `flowy-agent` 等五 crate 检查 `--all-targets -- -D warnings`（含单测里的 `clippy::assert_is_empty`、`default_trait_access`）；只跑 `--lib` 会漏掉测试代码里的 lint 错误。

### 15.5 下一单任务书（Phase 1 + Phase 2 核心门禁收口，完整四阶段见 [`V0-执行计划.md`](V0-执行计划.md)）

```text
【目标】按 flowy18_wip/docs/architecture/flowy-v0/V0-执行计划.md 推进 Phase 1（堵住真机三大 P0 断层 + INFO 业务 Trace）与 Phase 2（全类型交付硬门 + 方案 A 显式豁免表 + deliver_intent 崩溃窗口 + 普通入口审计落盘），不碰根工作区，不碰 V6 外发。

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
   - "<GIT_BASH>" flowy18_wip/docs/architecture/flowy-v0/accept.sh
   - cargo clippy -p flowy-ui -p flowy-studio -p flowy-app -p flowy-i18n -p flowy-agent --all-targets -- -D warnings
   - rustfmt --edition 2024 --check <全部改动 .rs 文件>
   - bun scripts/check-ui-contracts.mjs

【验收】8 条门禁全绿；单测较基线净增 ≥ 7 条；反向验证注释门禁时新增测试必红；后续紧接 Phase 3（核验卡 + Override UI）与 Phase 4（L3 提示层 + agent-desktop 真机终验）。
```

### 15.6 接手者第一小时检查清单

1. 进 `.agents/worktrees/feat-flowy-local-receipt`，读 [`V0-执行计划.md`](V0-执行计划.md) + [`证据.md` §11.26](证据.md) + [`BLOCKED.md`](BLOCKED.md)。
2. `git log --oneline -8`、`git branch --list "*flowy*"` 与 `git status` 核对分支、HEAD 与未提交文件。
3. 串行跑一遍 §15.5 第 4 步基线门禁（注意 PowerShell 下用 `"<GIT_BASH>"` 跑 `accept.sh`，真机测试务必先备份并在 `finally` 还原 `%APPDATA%\Flowy\Flowy\data`）。
4. 按 [`V0-执行计划.md`](V0-执行计划.md) Phase 1 → Phase 4 依序推进；遇到需拍板边界先查/写 `BLOCKED.md`，绝不猜着碰 V6 外发或根工作区。

---

## 16. 2026-11-25 现役总账（`feat/flowy-v0-rc@65772f857`，接管 §15）

> **本节为唯一现役真相（覆盖 §1–§15 的状态描述）**：当前工作树 `.agents/worktrees/feat-flowy-local-receipt` 已在分支 **`feat/flowy-v0-rc`** 上合并 `feat/flowy-super-ui`（`506ac599d`），并完成 6 轮纯内核加固与 `6+4` UI 解耦契约收口（`f8fb6b491`..`65772f857`），**只本地 commit、未 push、0 触碰 UI 文件**。

### 16.1 新增内核提交链（紧接 §15.1 之后）

| 提交 | 内容 | 关键文件 |
| --- | --- | --- |
| `506ac599d`..`97efb3113` | 合入 `feat/flowy-super-ui`，清理 `ExecInput` 空壳并统一 `roxmltree` OOXML 核验、修 L3 折叠提示与 i18n `{count}` | `flowy-agent`、`flowy-studio`、`flowy-ui`、`flowy-i18n` |
| `f8fb6b491` | 崩溃恢复初版对账、同字节交付、绑定覆盖与 L3 披露开关 | `acceptance.rs`、`delivery.rs`、`handle.rs` |
| `45d0d317b` | `queue.bind` fail-closed、显式目标会话路由、逐 `submission_id` 终态（`SubmissionOutcome`）、租约心跳、`ReceiptState::Cancelled` | `acceptance.rs`、`session.rs`、`engine.rs`、`handle.rs` |
| `54d10087f` | 持久化版本化 `WorkContract`、`SubmissionOutcome::Partial`、`work_status(work_id)`、加固 `verify.rs`（多 sheet/空锚/二次锁死） | `session.rs`、`verify.rs`、`handle.rs` |
| `a0e96262b` | 两阶段交付审计（`stage_with` / `confirm_delivered`）、失败轮次禁自动导入、幂等 `override_deliver_artifact`、坏中间段 `Quarantined` | `acceptance.rs`、`delivery.rs`、`publish_tool.rs`、`conversation_artifacts.rs`、`handle.rs` |
| `71cdb8e92` | `6` 项 UI 解耦控制 API、`accept_drain` 互斥、`loop_runner` 24 步与 `transform_context`、`confined_to` 最深祖先规范化、`pi.rs` 超时兜底 | `loop_runner.rs`、`tools/{builtin,pi}.rs`、`handle.rs`、`lib.rs` |
| **`65772f857`** | `4+1` 项 UI 对齐补丁：`apply_config` 自动唤醒、`visible_acceptances()`、`RetryDeadReceipt` 原子 `Superseded`、`session_work_status(session_id)`、`Quarantined` 去重推送 | `acceptance.rs`、`handle.rs`、`artifact_integration_tests.rs` |

### 16.2 内核 → UI `6+4` 强类型解耦契约（隔壁 UI worktree 对接清单）

| 接口 | 用途与不变量 |
| --- | --- |
| `set_autonomous_drain(true)` + `apply_config(cfg)` | 宿主 Tokio runtime 自治排水；`apply_config` 切到可执行路由后自动唤醒队列，UI 无需轮询 |
| `visible_acceptances() -> Vec<(AcceptedMessage, ReceiptState)>` | 返回全部非 `Completed`/非 `Cancelled` 回执（含退避期 `Retrying`、`Dead`、`Quarantined`），按 `created_at_ms` 升序 |
| `AgentUpdate::Receipt { id, state }` | 状态变迁单向推送（含 `Quarantined` 去重推送及启轮失败推送） |
| `cancel_task(TaskTarget::Session / Receipt) -> Result<CancelOutcome, _>` | 脱离 UI 当前激活会话的精确取消；保护已进入 `Finalizing` 的交付 |
| `apply_card_action(CardAction) -> Result<CardActionOutcome, _>` | 卡片动作统归内核：`OverrideDeliver`、`RetryDelivery`、`RetryDeadReceipt`（原子追加新回执并 `Superseded` 关闭旧死信，杜绝重启诈尸与重复点击） |
| `work_status(work_id)` / `session_work_status(session_id)` | 同一工作编号或直提会话均可查询 `WorkStatus`（修订号、终态、缺项、可见交付产物）；日志/会话/资料库三处一致 |
| `prepare_shutdown()` | 优雅退出钩子：停自治排水、取消可取消忙轮次、刷盘索引（崩溃正确性不依赖调用它） |

### 16.3 与《最终方案》的已知偏差与下一阶段优先级

详见 [`最终方案.md` §12.2](最终方案.md) 完整矩阵，核心四条留给下一里程碑：
1. **常驻 1 秒不变式 Router 独立对话 loop + 多 Worker 并发池**：当前仍为单 Worker 串行排队，无独立不排队对话 Router 与 `revise` steering 插队。
2. **Provider 原生 Structured Outputs（`response_format: json_schema`）**：L3 语义复核当前靠 Prompt + 宿主严格 parser 兜底。
3. **OS 级执行沙箱与进程树强杀**：文件工具已硬隔离（`confined_to`），但 `pi_shell` 仍在宿主权限下运行且未接 Windows Job Object 强杀孙进程树。
4. **图表内嵌工作簿核验（`ppt/embeddings/*.xlsx`）、`provenance` 写进产物、§5.18 运维四件套与 V6 外发（`BLOCKED.md` §5 仍硬止损）**。

### 16.4 现役验证命令与基线（`65772f857`）

```powershell
# 在 .agents/worktrees/feat-flowy-local-receipt/flowy18_wip 下串行执行：
cargo test -p flowy-agent-core                                 # 78 passed, 0 failed, 2 ignored
cargo test -p flowy-agent                                      # 165 passed, 0 failed, 1 ignored
cargo clippy -p flowy-agent-core -p flowy-agent -- -D warnings # 0 warnings
cargo check -p flowy-app                                       # 0 errors (UI 100% 兼容)
```
GUI 自动化实测工具：`<USER_HOME>\.gemini\config\skills\flowy-gpui-e2e\scripts\ui-shot.ps1`（支持 `-Preflight`、原子 AppData 隔离、`-KeepAuth`、`seed_receipt` + `restart` 热重启、Win32 DPI 感知 `resize`）。

---

## 17. 2026-11-25 现役最新总账（`feat/flowy-v0-rc@f47804a9d`，接管 §16）

> **唯一最新真相（接管 §16.1 / §16.3 / §16.4；§16.2 的 `6+4` UI 契约 100% 保持兼容并新增 `sandbox_availability()` 与 `PROVENANCE_INVALID` 导出）**：在 `65772f857` 与修订 9.3 文档（`10b99bac0`）之后，本轮完成 4 个纯内核提交（`5beece91c`..`f47804a9d`），**仍保持 0 触碰 UI 文件、只本地 commit 未 push**。

### 17.1 本轮新增提交链（紧接 `10b99bac0`）

| 提交 | 内容 | 关键目录 / 文件 |
| --- | --- | --- |
| `5beece91c` | 纯零行为变更拆分超大单文件（对外公开 API 100% 不变） | `flowy-agent/src/handle/`（7 文件）、`flowy-agent/src/verify/`（4 文件） |
| `a3fc4214a` | `RunFence` 两阶段取消回收锁（`ActiveRun.cancelling` + `Condvar` + `RunTicket` RAII + `PublishToken`）+ `prepare_shutdown` 2s 回收等待 | `flowy-agent-core/src/{run_fence,engine,lib}.rs`、`flowy-agent/src/handle/work.rs` |
| `13dad5e4c` | MIT 净室跨平台 Shell 静态安全策略 + 平台中立沙箱能力层（诚实上报 `Degraded` + SBPL/`bwrap` 纯构造器） | `flowy-agent-core/src/tools/{shell_policy,sandbox,mod}.rs`、`flowy-agent/src/handle/{guards,mod,tests}.rs` |
| **`f47804a9d`** | 单次 HTTP 工具调用结构化输出（`complete_structured` + `openai_structured_stream_fn`）升级 L3 `submit_semantic_verdict`；§6.8 来源溯源校验 `PROVENANCE_INVALID` | `flowy-agent-core/src/{transport,loop_runner,lib}.rs`、`flowy-agent/src/semantic_verify.rs`、`flowy-agent/src/verify/{provenance,mod,tests}.rs` |

### 17.2 新内核目录速查（接手修改不再碰 6,000 行巨型文件）

- `flowy-agent/src/handle/`：`mod.rs`（主入口）、`queue.rs`（排队与自治排水）、`work.rs`（`WorkStatus`/`cancel_task`/`apply_card_action`/`prepare_shutdown`）、`artifacts.rs`（交付/Override）、`observer.rs`（投影/回填）、`guards.rs`（`path_guard` + `shell_policy_guard`）、`tests.rs`
- `flowy-agent/src/verify/`：`mod.rs`（工具注册与报告落盘）、`contract.rs`（`BriefContract` 继承）、`ooxml.rs`（`.pptx`/`.docx`/`.xlsx`）、`provenance.rs`（§6.8 来源校验）、`tests.rs`
- 跨平台沙箱与进程回收蓝图（macOS Seatbelt / Linux `bwrap` / Windows `AppContainer` 约束与真机验收清单）：[`跨平台沙箱与进程回收预留方案.md`](跨平台沙箱与进程回收预留方案.md)

### 17.3 现役验证命令与最新基线（`f47804a9d`）

```powershell
# 在 .agents/worktrees/feat-flowy-local-receipt/flowy18_wip 下串行执行：
cargo test -p flowy-agent-core                                 # 90 passed, 0 failed, 2 ignored
cargo test -p flowy-agent                                      # 168 passed, 0 failed, 2 ignored
cargo clippy -p flowy-agent-core -p flowy-agent -- -D warnings # 0 warnings
cargo clippy -p flowy-agent --all-targets -- -D warnings       # 0 warnings
cargo check -p flowy-app                                       # 0 errors (UI 100% 兼容)
# 可选真云端 L3 回归（需本机 Flowy 登录态，消耗少量额度）：
cargo test -p flowy-agent live_cloud_structured_l3_verifies_pass_and_flagged -- --ignored --nocapture
```

---

## 18. 2026-11-26 现役最新总账（PR #492 · `feat/flowy-v1-integration`，接管 §17）

> **唯一最新真相（对应 `最终方案.md` 修订 9.7 §12.6 与 `证据.md` §11.33）**：基于最新 `origin/main@a12440075` 线性推进并提交 PR #492（仅改 `flowy18_wip/`，0 触碰根 workspace / `Cargo.toml` / `Cargo.lock` / `check-ui-contracts.mjs`）。已完成 **Super Agent 内核 S1–S3**、**极简私人助理 UI + 无歧义 Lucide SVG + 原生 GPUI 动效 + Windows 路径/真实技能修复** 以及 **5 笔对抗审查防膨胀加固**。

### 18.1 新增与重点演进模块速查

- **内核（`flowy-agent-core` / `flowy-agent` / `flowy-shell`）**：
  - `flowy-agent/src/verify/{contract,resolver}.rs`：多对象 `WorkContract`（`Deliverable | Record`、`Confirmed / PendingProposal`）、混合按需消歧器（未接线 LLM 异步入口收进 `#[cfg(test)] mod llm_probe`）。
  - `flowy-agent-core/src/{engine,session,loop_runner}.rs`：`TurnStatus::{Stopping, WaitingForUser}`、`PendingCard::{Disambiguation, ToolApproval}`、5 要素 `StopHandoffManifest`、三段式 `BranchHeartbeat`、按 turn 计数熔断与尾部成功重置、`Aborted` 优先。
  - `flowy-agent-core/src/tools/shell_policy.rs`：引号感知 tokenizer + 结构化 `argv` 策略（覆盖 PowerShell 缩写与 `rm -Irf`），`Ask` 接 `before_tool_call` 持久化 `ToolApproval(args_hash)`。
  - `flowy-agent/src/{delivery,publish_tool}.rs` + `handle/{mod,work,queue,guards,observer}.rs`：`expected_contract_revision` 强校验、`StagedIntentLog` 回滚（`Aborted | WaitingForUser`）、`CardAction::ResolveCard`、并发会话公平与多卡顺序确认；`flowy-shell` 已裁回 OTA-only。
- **UI / Studio / App / i18n（`flowy-ui` / `flowy-studio` / `flowy-app` / `flowy-i18n`）**：
  - `flowy-studio/src/ports/local_path.rs`：纯函数自顶向下 `VecDeque` Windows 重解析点安全校验。
  - `flowy-studio/src/composer_add_menu.rs`：`+` 旋转菜单（工作目录、真实已装技能、连接器占位、自动检查开关）+ Active-Only 可移除胶囊。
  - `flowy-studio/src/pending_card.rs`：消歧卡、工具审批卡、停机交接摘要（含 `verified_effects` 与本地化回退提示）与实时心跳条，带滚动限高、脱敏、`(session_id, card_id)` 锁与 `catch_unwind`。
  - `flowy-ui/src/{icons,motion,overlays,button}.rs` + `assets/icons/{panel-right,file-text,arrow-up,plug}.svg` + `LICENSES.md`：无歧义 Lucide SVG、`Icon` GPU 变换与叶级 `with_spring`；`flowy-app/src/app/{mod,skills,conversation_actions}.rs` 修硬编码 `set_reduce_motion(true)` 与真实 `skill_catalog()`。
  - `flowy-i18n/locales/*.json`（23 语种）：全量去 AI 味重写，891 键严格对称。

### 18.2 现役验证命令与最新基线（PR #492）

```powershell
# 在 flowy18_wip/ 下严格串行执行（禁止并发跑 cargo）：
node scripts/check-ui-contracts.mjs --self-test               # 24/24 passed
node scripts/check-ui-contracts.mjs                           # PASS
cargo fmt --all -- --check                                    # 0 diff
cargo clippy --locked -p flowy-ui -p flowy-i18n -p flowy-studio -p flowy-app -p flowy-agent-core -p flowy-agent -p flowy-shell -p flowy-skills --all-targets -- -D warnings  # 0 warning
cargo test --locked -p flowy-agent-core                       # 112 passed (2 ignored)
cargo test --locked -p flowy-shell                            # 1 passed
cargo test --locked -p flowy-skills                           # 66 passed
cargo test --locked -p flowy-i18n                             # 8 passed
cargo test --locked -p flowy-ui                               # 45 passed (43 lib + 2 int)
cargo test --locked -p flowy-studio                           # 110 passed
cargo test --locked -p flowy-app                              # 51 passed
cargo test --locked -p flowy-agent -- --test-threads=1        # 207 passed (3 ignored)
# 合计 8 crate：600 passed / 0 failed（较 551 基线净增 +49）
```

### 18.3 下一 PR 接手优先级

1. **S4（目录级 Workspace RWLock + Brief/权限双裁剪）**：将 `handle/queue.rs` 单槽串行锁升级为按规范化项目根路径并发 RWLock，并按子任务裁剪 Brief 与 `confined_to`。
2. **S5（休眠/唤醒跳表探针 + 真机 OS 沙箱 `Enforced`）**：结合 [`跨平台沙箱与进程回收预留方案.md`](跨平台沙箱与进程回收预留方案.md) 在三端真机实测逃逸阻断后升为 `Enforced`。
3. **V6 外发通道**：仍受 [`BLOCKED.md`](BLOCKED.md) §5 硬阻塞，未经拍板严禁动工。

---

## 19. 2026-11-26 现役最新总账（PR-2A · `feat/flowy-router-kernel` / PR #496，接管 §18）

> **唯一最新真相（对应 `最终方案.md` 修订 9.10 §12.9 与 `证据.md` §11.37）**：在 PR #493（修订 9.8）、PR #494（PR-1 S4 并发池 + `WorkspaceRwLock`）与 PR #495（导航/全局快搜/进度看板 v2/会话右键菜单）全部合入 `origin/main@feb010a56` 后，隔离 worktree `.agents/worktrees/feat-router-kernel`（分支 `feat/flowy-router-kernel`，**PR #496 当前为 open 状态**）已落地 **PR-2A 会话级轻量 Router 与多 Work 并发调度内核**（纯内核，0 触碰 UI / App / i18n / 根 workspace），并完成 PR #495 耦合护栏与深度 `/code-review` + `carl-idea-king` + `/ponytail` 加固清理。

### 19.1 PR-2A 新增与重点演进模块速查

- **`flowy-agent-core`（6 文件）**：`types.rs`（`MessageSource::{CardResume, SystemEvent { event_kind }}`）、`session.rs`（`SessionRole`、`WorkAccessMode`、`PendingRouterFollowup`、`DispatchRecord`）、`acceptance.rs`（`accept_strict`、`strict_target`、`lease: Option<Box<Lease>>` 保 `240B`）、`loop_runner.rs`（`PreToolDecision::SuspendWithCard`）、`engine.rs`（`create_session_with_role`、`child_sessions`、`enqueue_router_followup` 上限 16 + `Engine::prepare` 原子弹出、`record_dispatch_request` 上限 `MAX_DISPATCH_REQUESTS = 128` 确定性淘汰与写盘失败回滚、`clear_pending_cards`）、`lib.rs`（+5 单元测试）。
- **`flowy-agent`（15 文件）**：
  - `handle/router.rs`（新增）：4 亚秒控制工具（统一 `spawn_blocking`，播报轮仅留 `query_work_status`）、`build_router_pre_tool_hook`、900 字符 `[ChildWorksReadOnlySnapshot]`、跨根所有权校验、`lookup_idempotent_dispatch`、不可变环境/防提权、单一回滚闭包（同步清理 Home 空工作目录）。
  - `handle/work_events.rs`（新增）：叶锁 `RouterInbox` 合并防抖、`format_work_event_batch`（落盘前重读权威态标 `stale_event_superseded`）、`schedule_router_inbox_flush`（`pending_router_followups` 严格优先于 `WorkEventBatch`）。
  - `handle/{mod,queue,work,guards,artifacts,observer}.rs`、`artifact_store.rs`、`conversation_artifacts.rs`、`lib.rs`：只读三工具白名单（`glob`/`grep`/`read_file`）、双层 `strict_target` 护栏、可恢复级联删除、PR #495 快搜折叠脱敏 / 归档联动查子 / Worker 豁免自动标题、`submit_router_message` 角色校验；`handle/router_tests.rs`（17 条纵深/故障注入/青禾 5 步 E2E/PR #495 耦合/审查加固单测，RAII `ScratchDir`）。

### 19.2 PR-2B UI 接线契约（冻结的 `AgentHandle` 公开 API）

PR-2B（`flowy-studio` / `flowy-app` / `flowy-i18n`）只需消费以下强类型接口，禁止绕过 `AgentHandle` 直访内部状态：

| API / 类型 | 语义与约定 |
| --- | --- |
| `create_router_session()` / `create_router_project_session(workspace)` | 创建顶层 `SessionRole::Router` 会话（失败不篡改全局默认 workspace，成功后设为 `active`） |
| `submit_router_message(router_id, submission, text)` | 脱离 UI `active` 直投顶层 Router（忙时持久化进 `pending_router_followups` FIFO；拒绝非 Router/Worker） |
| `session_role(id)` / `pending_router_followups(router_id)` | 查询会话角色与待续用户跟进队列 |
| `child_works(root_session_id) -> Vec<ChildWorkSummary>` | 读取名下全部子 Worker 宿主权威状态、心跳、交付物、缺项与停机交接 |
| `root_pending_cards(root_session_id) -> Vec<RoutedPendingCard>` | 聚合顶层 Router 选卡与所有子 Worker 审批/消歧卡（携带真实 `owner_session_id`） |
| `stop_router_reply(router_id)` / `cancel_work(root_id, worker_id)` / `revise_work(root_id, worker_id, brief, req_id)` | 独立止停 Router 回复（不动子 Worker）、按所有权取消或幂等修订指定子 Worker |

### 19.3 现役验证命令与最新基线（PR #496）

```powershell
# 在 .agents/worktrees/feat-router-kernel/flowy18_wip 下串行执行：
$env:CARGO_BUILD_BUILD_DIR = Join-Path $env:LOCALAPPDATA 'Temp\flowy18-router-build'
cargo fmt --all -- --check                                                            # 0 diff
cargo clippy --locked -p flowy-agent-core -p flowy-agent -p flowy-studio --all-targets -- -D warnings  # 0 warning
cargo test --locked -p flowy-agent-core                                               # 117 passed (2 ignored)
cargo test --locked -p flowy-agent -- --test-threads=1                                # 246 passed (3 ignored)
cargo test --locked -p flowy-studio                                                   # 112 passed
cargo check --locked -p flowy-desktop                                                 # 0 error
# 三 crate 合计：475 passed / 0 failed
```

### 19.4 下一 PR 接手优先级

1. **PR-2B（Studio / App / i18n Router UI 接线）**：待 PR #496 合入 `main` 后，新建会话改走 `create_router_*`、`panel.rs` 解除后台 `Worker` 对发送框阻断、`ports/board.rs` 与 `deliverables.rs` 接入 `child_works` / `root_pending_cards`、补齐 23 语种对称键与实机双主题 E2E。
2. **S5（休眠/唤醒跳表探针 + 真机 OS 沙箱 `Enforced`）**：结合 [`跨平台沙箱与进程回收预留方案.md`](跨平台沙箱与进程回收预留方案.md) 在三端真机实测逃逸阻断后升为 `Enforced`。
3. **V6 外发通道**：仍受 [`BLOCKED.md`](BLOCKED.md) §5 硬阻塞，未经拍板严禁动工。

---

## 20. 2026-11-26 现役最新总账（PR #496 已合入 `main` + PR #497 · `feat/flowy-studio-ux-polish`，接管 §19 状态）

> **唯一最新真相（对应 `最终方案.md` 修订 9.11 §12.10 与 `证据.md` §11.38）**：PR #496（PR-2A Router 纯内核）已合入 `origin/main@aa522353f`；工作树 `.agents/worktrees/feat-flowy-local-receipt`（分支 `feat/flowy-studio-ux-polish`，**PR #497**）已零冲突线性 rebase 至 `origin/main@aa522353f`，完成进度看板 7 日本地反筛、资料库网格锚点/右键修复、快搜分类筛选与 5 秒归档撤销以及两轮审查去膨胀收口。**保留该暖构建工作树（切勿删除）**以加速后续 PR-2B 开发。

### 20.1 PR #497 改动速查与零冲突边界

- **严格 0 触碰**：`flowy-agent-core` / `flowy-agent` / `flowy-ui` / `flowy-studio/src/overview/flow_modal.rs` / 根 workspace（与 §19.2 冻结的 PR-2B 契约零冲突）。
- **`flowy-studio`（`ports/board.rs`、`ports/mod.rs`、`overview/mod.rs`、`work_actions.rs`、`library/view.rs`、`Cargo.toml`）**：复用工作区 `chrono` 实现本地自然日 7 日分桶与点击反筛（已清净旧 UTC 死公开 API）；修复资料库网格 `...` 错位锚点、补全列表/网格右键菜单与状态徽章/收藏星标、隐藏 `Missing`/`Corrupt` 无效「另存为」并守住 `LibraryModel` 无磁盘路径契约。
- **`flowy-app`（`quick_search.rs`、`conversation_actions.rs`、`dock.rs`、`mod.rs`）**：快搜新增 5 类筛选芯片（`Tab`/`Shift+Tab` 保焦）与 UTF-8 安全 `StyledText` 高亮（每帧单次分词 + 借用排序）；右下角 5 秒归档撤销条（`.occlude()` + `!modal_active()` + 悬浮窗避让 + 脱敏 + 精简单一代际控制器）；删除死枚举 `Section::Search`。
- **`flowy-i18n`（23 语种）**：同步删净孤立键 `section.search`，全量 **924 键**严格对称。

### 20.2 现役验证命令与最新全量基线（PR #497 @ `origin/main@aa522353f`）

```powershell
# 在 .agents/worktrees/feat-flowy-local-receipt 下严格串行执行（禁止并发跑 cargo）：
node flowy18_wip/scripts/check-ui-contracts.mjs --self-test                  # 24/24 passed
node flowy18_wip/scripts/check-ui-contracts.mjs                              # PASS
cargo fmt --manifest-path flowy18_wip/Cargo.toml --all -- --check            # 0 diff
cargo clippy --manifest-path flowy18_wip/Cargo.toml -p flowy-agent-core -p flowy-agent -p flowy-i18n -p flowy-ui -p flowy-studio -p flowy-app -p flowy-skills -p flowy-shell -p flowy-desktop --all-targets -- -D warnings  # 0 warning
cargo test --manifest-path flowy18_wip/Cargo.toml -p flowy-agent-core        # 117 passed (2 ignored)
cargo test --manifest-path flowy18_wip/Cargo.toml -p flowy-agent -- --test-threads=1  # 246 passed (3 ignored)
cargo test --manifest-path flowy18_wip/Cargo.toml -p flowy-i18n -p flowy-ui -p flowy-studio -p flowy-app -p flowy-skills -p flowy-shell  # 8 + 46 + 114 + 56 + 66 + 1 = 291 passed
# 8-Crate 合计：654 passed / 0 failed (5 ignored)；双主题实机 E2E Dark 32/32 + Light 31/31 全绿
# 预览包 FlowyPreview\flowy-app.exe SHA-256: 4171AC5CD9858DC50EEAF00B90273CE456FFE0B9983875F8049D233281F4C96A
```

### 20.3 下一 PR 接手优先级

1. **PR-2B（Studio / App / i18n Router UI 接线）**：PR #496 现已合入 `main`；待 PR #497 合流后直接基于最新 `origin/main` 消费 §19.2 冻结的 `AgentHandle` 契约完成 Router 新建会话、`panel.rs` 解除后台 `Worker` 发送阻断、`ports/board.rs` 与 `deliverables.rs` 多分支泳道/待人卡路由与双主题实机 E2E。
2. **S5（休眠/唤醒跳表探针 + 真机 OS 沙箱 `Enforced`）**：按 [`跨平台沙箱与进程回收预留方案.md`](跨平台沙箱与进程回收预留方案.md) 推进三端真机实测。
3. **V6 外发通道**：仍受 [`BLOCKED.md`](BLOCKED.md) §5 硬阻塞，未经拍板严禁动工。

---

## 21. 2026-11-26 现役最新总账（PR #497 已合入 `origin/main@eb30a4d21` + PR-2B · `feat/flowy-router-ui` 修订 9.12 收口）

> **唯一最新真相（对应 `最终方案.md` 修订 9.12 §12.11 与 `证据.md` §11.39）**：PR #496（PR-2A）与 PR #497（工作台 UX 打磨）均已合入 `origin/main@eb30a4d21`；工作树 `.agents/worktrees/feat-router-kernel`（分支 `feat/flowy-router-ui`，**PR-2B**）已完成线性 rebase、全部 Q1–Q6 Router UI 接线、`/code-review` + `/laoyeye` + `/ponytail` 深度修复与减脂，通过 8-Crate **661 / 661** 全绿、9-Crate `clippy --all-targets -D warnings` 0 警告、真云端同会话双消息非阻塞 E2E（`1/1`）与双主题双语视觉审计。**保留该暖构建工作树（切勿删除）**以加速后续开发。

### 21.1 PR-2B 改动速查与核心不变量

- **严格 0 触碰**：`flowy-agent-core` / `flowy-ui` / `flowy-skills` / V6 外发通道 / 根 workspace；主工作区 `d:/workSpace/git_clone_test/allo` 保持 100% 干净。
- **`flowy-agent`（薄接缝）**：`submit_router_chat_if_active`（原子上下文校验 + 延迟建 Router + 失败回滚）；`chat_history()` 确定性投射子 Worker `Verification`（含 `source_session_id`）与 `Artifact`；`override_deliver_artifact` 联动通知父 Router；`session_needs_attention` 聚合子 Worker；`boot()` 恢复 `pending_router_followups` 气泡对齐 `observer.rs:1501` 排除 `SystemEvent`；清理重复私有 `AgentHandle::root_session_id_for`。
- **`flowy-studio` & `flowy-app`**：
  - `router_works.rs`（新增）+ `panel.rs`：事件驱动缓存 `popover_works: PopoverWorksSnapshot`（O(1) 渲染零每帧锁扫），Chat 状态槽、Home 恢复胶囊与双区弹层（`本会话任务 / 其他会话任务`）共享同一真相源与关注态；删除生产死 API `StudioPanel::create_task` 与隐式薄壳 `confirm_override_delivery`。
  - `ports/router_projection.rs`（新增）+ `ports/board.rs` + `overview/flow_modal.rs`：折叠子 Worker 与 `strict_target` 回执入父 Router 组，每条子分支携带 `owner_session_id` 使 Flowchart Modal 覆盖/重试/停止精准命中子 Worker（无报告审批卡直提供「打开对话」跳父 Router），并用 `fold_group_updated_at` 合并子 Worker 时间戳兼容 PR #497 7 日日历下钻。
  - `work_actions.rs` + `deliverables.rs` + `pending_card.rs` + `feed.rs` + `projects.rs` + `dock.rs` + `library_actions.rs`：`stop_session_turn` 单一按 `SessionRole` 分流停止；`OverrideDeliver` 显式带 `session_id`；`root_pending_cards` 聚合审批不锁主输入框。
- **`flowy-i18n`（23 语种）**：新增 9 键（在 PR #497 的 924 键基础上增至 **933 键**全对称，LF 换行）。

### 21.2 现役验证命令与最新全量基线（PR-2B @ `origin/main@eb30a4d21`）

```powershell
# 在 .agents/worktrees/feat-router-kernel/flowy18_wip 下严格串行执行（禁止并发跑 cargo）：
$env:CARGO_BUILD_BUILD_DIR = Join-Path $env:LOCALAPPDATA 'Temp\flowy18-router-build'
node scripts/check-ui-contracts.mjs --self-test                  # 24/24 passed
node scripts/check-ui-contracts.mjs                              # PASS
cargo fmt --all -- --check                                       # 0 diff
cargo clippy --locked --all-targets -p flowy-agent-core -p flowy-agent -p flowy-i18n -p flowy-ui -p flowy-studio -p flowy-app -p flowy-skills -p flowy-shell -p flowy-desktop -- -D warnings  # 0 warning
cargo test --locked -p flowy-agent-core                          # 117 passed (2 ignored)
cargo test --locked -p flowy-shell -p flowy-skills -p flowy-i18n -p flowy-ui -p flowy-studio -p flowy-app  # 1 + 66 + 8 + 46 + 119 + 56 = 296 passed
cargo test --locked -p flowy-agent -- --test-threads=1           # 248 passed (4 ignored)
cargo check --locked -p flowy-desktop                            # 0 error
# 8-Crate 合计：661 passed / 0 failed (6 ignored)；真云端 Router E2E 1/1 PASS；双主题双语视觉审计 9 PNG 全绿
```

### 21.3 下一 PR 接手优先级

1. **合流 PR-2B（`feat/flowy-router-ui`）**：确认推送并合入 `main` 后，可按需在真实桌面再补 1 轮看板 Flowchart Modal 子 Worker 交互截图与 `FlowyPreview\flowy-app.exe` 刷新。
2. **S5（休眠/唤醒跳表探针 + 真机 OS 沙箱 `Enforced`）**：按 [`跨平台沙箱与进程回收预留方案.md`](跨平台沙箱与进程回收预留方案.md) 在三端真机实测逃逸阻断后升级 `Enforced`。
3. **V6 外发通道**：仍受 [`BLOCKED.md`](BLOCKED.md) §5 硬阻塞，未经拍板严禁动工。

---

## 22. 2026-11-26 现役最新总账（PR #498 已合入 `origin/main@83a9aef16` + 修订 9.13 真云端全链路 UX 修复与 669 基线）

> **唯一最新真相（对应 `最终方案.md` 修订 9.13 §12.12 与 `证据.md` §11.40）**：PR #498（PR-2B Router UI）已合入 `origin/main@83a9aef16`；暖构建工作树 `.agents/worktrees/feat-flowy-local-receipt`（分支 `fix/flowy-live-cloud-ux-polish`）已完成零冲突线性 rebase、真云端全链路 8 项 UX 修复、PR #498 耦合优化（共享心跳本地化、投影层防重停、导航回 Chat 与 23 语种桌面通知文案）及两轮审查收口，通过 8-Crate **669 / 669** 全绿、真云端 `live_cloud_` **3 / 3** 全绿、9-Crate `clippy --all-targets -D warnings` 0 警告与双主题真云端桌面视觉 E2E（9 PNG 目检）。**保留该暖构建工作树（切勿删除）**以加速后续开发。

### 22.1 修订 9.13 改动速查与核心不变量

- **严格 0 触碰**：技能市场、`flowy-agent-core` / `flowy-skills` / V6 外发通道 / 根 workspace；主工作区 `d:/workSpace/git_clone_test/allo` 保持 100% 干净。
- **`flowy-agent`（`handle/mod.rs`、`handle/router.rs`）**：共用 `BASE_BEHAVIOR_PROMPT` 锁死回复语种；`workspace_has_user_files` 识别历史 `outputs/<prior>` 防多轮误报空目录。
- **`flowy-ui`（`overlays.rs`）**：`resolve_popover_vertical` 自动垂直翻转并贴边夹紧。
- **`flowy-studio`（`markdown.rs`、`history.rs`、`pending_card.rs`、`composer.rs`、`router_works.rs`、`feed.rs`、`panel.rs`、`overview/{mod,flow_modal}.rs`）**：非交互流式 Markdown 与 CommonMark 围栏补全；共享本地化心跳 `localize_heartbeat_action` / `format_heartbeat_line_localized` 覆盖全部 6 类 observer 状态；删除 `FoldMeta::duration_secs`、结算锁定 `live = false`；`ChatFeed` 投影层条件省略冗余尾行 `已停止`；`set_surface(Chat)` 与全入聊路径置 `session_opened_by_user = true`；诚实看板进度与对称去省略号标题去重。
- **`flowy-app` & `flowy-i18n`（`appearance.rs`、`flex.rs`、`settings.rs`、`dock.rs`、23 份 `locales/*.json`）**：8 处单选/选中控件（含 `theme_card`）恢复 Windows UIA `InvokePattern`；`dock.rs` 标题截断、邮箱去重与动态套餐徽章；23 语种 `settings.notify` 桌面通知文案（**933 键**对称，LF 换行）。

### 22.2 现役验证命令与最新全量基线（修订 9.13 @ `origin/main@83a9aef16`）

```powershell
# 在 .agents/worktrees/feat-flowy-local-receipt 下严格串行执行（禁止并发跑 cargo）：
bun flowy18_wip/scripts/check-ui-contracts.mjs --self-test                  # 24/24 passed
bun flowy18_wip/scripts/check-ui-contracts.mjs                              # PASS
cargo fmt --manifest-path flowy18_wip/Cargo.toml --all -- --check           # 0 diff
cargo clippy --manifest-path flowy18_wip/Cargo.toml -p flowy-agent-core -p flowy-agent -p flowy-i18n -p flowy-ui -p flowy-studio -p flowy-app -p flowy-skills -p flowy-shell -p flowy-desktop --all-targets --locked -- -D warnings  # 0 warning
cargo test --manifest-path flowy18_wip/Cargo.toml --locked -p flowy-agent-core -p flowy-shell -p flowy-skills -p flowy-i18n -p flowy-ui -p flowy-studio -p flowy-app  # 117+1+66+8+47+124+57 = 420 passed
cargo test --manifest-path flowy18_wip/Cargo.toml --locked -p flowy-agent -- --test-threads=1  # 249 passed (4 ignored)
cargo test --manifest-path flowy18_wip/Cargo.toml -p flowy-agent live_cloud_ -- --ignored --nocapture --test-threads=1  # 3/3 PASS
cargo check --manifest-path flowy18_wip/Cargo.toml --locked -p flowy-desktop  # 0 error
# 8-Crate 合计：669 passed / 0 failed (6 ignored)；真云端 live_cloud_ 3/3 PASS；双主题桌面视觉 E2E 9 PNG 全绿
# 预览包 FlowyPreview\flowy-app.exe SHA-256: F5ADC622E8CFCBBEFDDAB93C2AE092C3572315408797628476EC4704EF024FE1
```

### 22.3 下一 PR 接手优先级

1. **S5（休眠/唤醒跳表探针 + 真机 OS 沙箱 `Enforced`）**：按 [`跨平台沙箱与进程回收预留方案.md`](跨平台沙箱与进程回收预留方案.md) 在三端真机实测逃逸阻断后升级 `Enforced`。
2. **V6 外发通道**：仍受 [`BLOCKED.md`](BLOCKED.md) §5 硬阻塞，未经拍板严禁动工。

---

## 23. 审查复核七项修复与收口（2026-11-26 · `fix/flowy-router-review-fixes`）

> 承接 `origin/main@3099c4bbf`（PR #501 合入后）接力审查结论，本分支（`.agents/worktrees/feat-router-kernel`，待提 PR）已完成全部 4 项 `[Required]` + 3 项 `[Optional]` 修复与审查清理，`[FYI]` #8（`flowy-shell::notify` 联动 `prefs.notify_browser` / `persona.rs`）显式保留到后续独立 PR。

### 23.1 本分支关键改动清单

1. **多字节中文路径安全（`flowy-studio/src/ports/local_path.rs`）**：抽取 `strip_windows_unc_prefix(raw: &str)` 使用 `raw.get(..8)?`，修复 `C:\项目\报告.md` 在 `display_path_str` 与 `strip_windows_verbatim_local` 字节切片触发 UTF-8 边界 panic。
2. **冷启动项目与 `SessionRole::Router` 意图保真（`acceptance.rs`、`queue.rs`、`handle/mod.rs`、`panel.rs`）**：`AcceptedMessage` 与 `JournalEntry::Accepted` 持久化 `Option<Box<DraftTarget>>`（装箱维持枚举紧凑），配合 `AcceptanceQueue::accept_with_target`、`AgentHandle::accept_deferred_draft` / `accept_deferred_router`（`strict_target = true`）、`Harness::create_background_session_with_role(..., make_active: false, ...)`；模型目录加载期间，新项目草稿首发与已有 Router 续聊统一延迟入队（移除 `library.loading` 阻塞分支），并在消费者执行时精确落盘回原项目且不抢夺后来切换到的草稿焦点。
3. **交付抽屉核验终态过滤（`flowy-studio/src/feed.rs`）**：`collect_latest_deliverables` 中的 `ChatLine::Verification` 统一校验 `verification_settled`（且单次绑定 `norm` 消除重复分配），不再因先前“通过”条目而把后期复验失败产物放进抽屉。
4. **看板心跳卡片本地化与死函数清理（`board.rs`、`router_projection.rs`、`work_actions.rs`、`pending_card.rs`）**：`BoardDataSource::build_board_snapshot` 至投影层统一透传 `localize: &dyn Fn(&str) -> String` 并改用 `format_heartbeat_line_localized`，删除未使用的 `format_heartbeat_line` 与 `Harness::create_background_home_session`。
5. **父时间线子卡按调度轮次锚定 + 零克隆索引（`engine.rs`、`handle/mod.rs`、`router_tests.rs`）**：新增 `Engine::child_session_ids(root_id)` 消除 `append_projected_child_cards`、`set_session_archived` 与 `session_needs_attention` 克隆子会话历史（50 历史 Router + 2 并发流式 Worker `[M1-BENCH]` 实测 `chat_history() x100 = 2993us (29.93us/call)`）；新增 `AgentHandle::collect_work_dispatch_turns`（以跨 `ChatLine::User` 清零的 `VecDeque` 配对批处理工具调用）与 `CardSlot` 将子 Worker `Verification`/`Artifact` 锚定到对应 `dispatch_work` / `control_work(action="revise")` 轮次末尾（跨轮 `dispatch_receipt_id` 细粒度拆分留待后续评估）。

### 23.2 当前基线与下一 PR 顺位

- **本地已绿门禁（提交 `86b0f755c` 对齐后）**：8-Crate 串行测试 **679 / 679 PASS**（+10 净增回归用例，0 失败，6 ignored）、9-Crate `--all-targets` Clippy **0 告警**、`cargo check -p flowy-desktop --locked` 通过、`cargo fmt --all -- --check` 通过、`check-ui-contracts.mjs` 通过；`--ignored` 真云端测试与桌面真机截图 E2E 未在本分支重跑。
- **下一 PR 优先级**：`[FYI]` #8 桌面通知偏好联动（`flowy-shell::notify` ↔ `prefs.notify_browser` / `persona.rs`）→ **S5** → **V6**（阻塞中）。

---

## 24. 桌面壳可靠化、类 GPT Dot 对话净化与竞态收口（2026-11-27 · `fix/flowy-desktop-shell-polish`）

> 承接 `origin/main@0c1d517c5`（PR #503 合入后）在本分支（`.agents/worktrees/feat-flowy-local-receipt`）闭合 **Track A**（桌面系统通知、单实例唤醒、启动前会话防丢快照、设置空壳收敛、`loop_runner` 错误保真与 `router`/`history` 首轮折叠）+ **Track B**（展开态思考链竖线与去按钮、`UiObserver` 内部播报错误隔离、会话预览过滤、并行兄弟 `[SiblingWorks]` 节流提示、`reconcile_committed_acceptances` 活认领竞态修复）；原 `[FYI]` #8 的 `flowy-shell::notify` ↔ `prefs.notify_browser` 联动现已闭合。

### 24.1 关键改动清单

1. **桌面通知与单实例唤醒（`flowy-shell/src/{notify,wake,lib}.rs`、`flowy-desktop/src/main.rs`、`flowy-app/src/app/mod.rs`、`flowy-i18n` 943 键）**：二次启动遇 `InstanceLock` 占用即写入 `wake.request` 后干净退出，前台实例在 250ms 轮询中还原置顶主窗；`StudioEvent::TurnFinished` 结合 `prefs.notify_browser` 在后台静默或异常/需确认时触发系统通知，点通知呼出主窗。
2. **启动前非空会话滚动快照 + 数据目录直达 + 诚实设置收敛（`session.rs`、`main.rs`、`prefs.rs`、`settings.rs`、`dock.rs`）**：持锁后、`Engine::open` 前以 FNV-1a 指纹去重并滚动保留最近 5 份 `backups/sessions-*`（空库绝不覆写历史有效备份）；设置「数据管理」开放 `打开目录` 按钮；移除空壳开关、`settings.later` 占位与底部静态图标（保留 `#[serde(default)]` 字段兼容旧配置）。
3. **类 GPT Dot 对话纯净度 + 错误日志与内部播报隔离（`loop_runner.rs`、`history.rs`、`feed.rs`、`handle/mod.rs`、`observer.rs`、`work_events.rs`、`router.rs`）**：`failure_detail` 优先提取 `error_message`；`Projection` 导出 `ack_rows`/`process_rows` 联动 `feed.rs` 收起冗余复制按钮与重复 `Flowy` 头并为展开态中间 `Assistant` 套 `.border_l_1()` 思考链竖线（0 改 `ChatLine`）；`Live.active_turn_is_internal` / `active_turn_prev_status` 隔离后台 `[WorkEventBatch]` `Failed`，防止健康对话底部追加 `chat.status_error`；`extract_session_preview` 跳过紧邻 `Reasoning`/`ToolCall`/`Tool`/`ToolResult` 的中间旁白；`sibling_work_counts` 在 `[WorkEventBatch]` 标明活跃兄弟数量并约束 `is_announcement` 仅出 1 句简报。
4. **并发与测试稳定性收尾（`queue.rs`、`router_tests.rs`）**：`run_accepted_once` 透传当前进程存活 `(receipt_id, attempt_id)` 给 `reconcile_committed_acceptances` 消除先写 `Completed` 再清 `active_claims` 的并发窗口竞态；`router_tests` 补齐 `escalation-checked` 同步门与终态等待。

### 24.2 当前基线与下一优先级

- **本地已绿门禁**：10-Crate `--all-targets` Clippy **0 告警**、8-Crate 串行测试 **695 / 695 PASS**（+16 净增回归用例，0 失败，6 ignored）、`cargo check -p flowy-desktop --locked` & `cargo build --bin flowy-app --locked` 通过、`cargo fmt --all -- --check` 零差异、`check-ui-contracts.mjs` 通过，双主题真机视觉 E2E `6 / 6 PNG`（Dark `zh-CN` + Light `en`）`ok=True, failures=0, log_errors=0`。
- **后续优先级**：`persona.rs` 注入 / 跨轮 `dispatch_receipt_id` 细粒度重锚 → **S5**（休眠/唤醒跳表探针 + 真机 OS 沙箱 `Enforced`）→ **V6**（仍受 [`BLOCKED.md`](BLOCKED.md) §5 硬阻塞，未经拍板不动工）。

---

## 25. Dot 分阶段圆角气泡、悬浮复制引用与假评分下线收口（2026-11-27 · `feat/flowy-dot-bubbles-and-quote`）

- **代码与契约状态**：基于 `origin/main@08d0aebbe`（PR #504）新建特性分支 `feat/flowy-dot-bubbles-and-quote`，已完成 5 笔干净提交（`c8cbc5eae` / `e139bbfd6` / `cc4638bb5` / `c4902ded4` / `6d8827619`）。严格保留 PR #503 / Track B 内核接缝（`collect_work_dispatch_turns`、`ChatLine::Verification::source_session_id`）及 `check-ui-contracts.mjs` 全规则。
- **核心改动摘要**：
  1. 彻底清除无后端假五星评分（`chat.rs` `rating_row`/`rate_result`、`panel.rs` & `app/mod.rs` `StudioEvent::Rated`、23 语种 `chat.rate_prompt`/`chat.rate_thanks`/`ui.rating`），`rg` 扫描归零，反向缺键证伪生效。
  2. `feed.rs` 落地 Dot 分阶段圆角气泡（去顶部 `sparkles`/`Flowy` 标题，区分右上 `User` 药丸、左上紧凑 `ack_rows` 气泡、独立终稿 `Assistant` 卡片与 `.border_l_1()` 展开态思考链），窄视口 `360px` 有界不溢出。
  3. `flowy-ui` 引入 Lucide `quote.svg`，23 语种 LF 对称补齐 3 键（维持 943 键），实现 `h(px(26.))` `.group("msg-hover-{row_id}")` 零抖动悬浮栏（仅复制+引用，UIA 标签带上下文摘要）与微信式单条 `QuoteBanner` + `✕` + `/skill:` 保首的 `> ❝ <role>: <excerpt>` 拼拆/`sync_quote_session()` 单源状态同步。
- **最近一次实测基线（详见 `证据.md` §11.43）**：
  - `cargo fmt --all -- --check` + `check-ui-contracts.mjs --self-test`（24/24）+ 全仓扫（943 键 × 23 语种 LF）+ `cargo check -p flowy-desktop --locked` + 11-Crate `--all-targets` Clippy：**全绿 0 告警**。
  - 8-Crate 串行 `cargo test --locked`：**703 / 703 PASS**（+8 新增单测/GPUI 测试，0 失败，6 ignored；`flowy-agent` `-- --test-threads=1` 为 257 PASS / 4 ignored；`flowy-studio` 为 140 PASS）。
  - 双主题真机 E2E（`ui-shot.ps1`，Dark `zh-CN` + Light `en`）：**25 / 25 步全过**（`ok=True, failures=0, screenshots=4, log_errors=0`）。
- **接手注意事项**：当前已收敛待推分支发 PR（提交信息与 Git 作者严格使用人类身份）；`QuoteBanner` 当前有意保留引用摘录原始 Markdown 符号以保 `/skill:` 与代码文本忠实透传，若后续产品希望前端显示层剥离 `##`/`**` 标记，仅需局部调整 `feed.rs`/`composer.rs` 渲染显示函数，不影响底层存储协议。


---

## 26. 六连 PR 并行收口与主干同步基线（2026-11-27 · `origin/main@c4da1ac65`，PR #505–#511）

- **主干现状**：截至 `origin/main@c4da1ac65`，Wave 1–3 五笔性能优化/安全加固 PR（`#505` / `#506` / `#508` / `#509` / `#510`）与桌面单实例唤醒修复（`#511`）已全部合入 `main`。

| PR | 分支 / Merge SHA | 关键改动落点 | 合流前主要门禁 |
| --- | --- | --- | --- |
| `#505` | `perf/flowy18-markdown-library-trace` (`c8f183daf`) | `markdown` 块级哈希缓存、`library` 流式摘要与分块预览、`trace` 尾截脱敏 | `check-ui-contracts`、`studio` 139 PASS、CI 7/7 绿 |
| `#506` | `fix/flowy18-shell-and-skills-hardening` (`c3e79ab70`) | `shell_policy` PS 反引号+子壳递归、`sandbox` 凭据路径、`skills` 250/20k 预算与转义 | `agent-core` 123 / `skills` 70 / `guard` 1 PASS、CI 7/7 绿 |
| `#508` | `feat/flowy18-ooxml-chart-and-verify-polish` (`26489817d`) | PPTX 内嵌 `.xlsx` 图表核验、`finalize_report` 聚合摘要、`sha256:` 严格 64-hex | `flowy-agent` 260 PASS、4-Crate Clippy 0 告警（手动合入） |
| `#509` | `feat/flowy18-spill-read-and-view-gc` (`58a0f6629`) | 流式行分片 `read_file`、`views-meta/` 侧车 SHA-256 防篡改 GC（3 天 5 份） | `agent-core` 131 / `agent` 263 PASS、5-Crate Clippy（手动合入） |
| `#510` | `perf/flowy18-s5-clock-and-io-hardening` (`d7a4e48d0`) | `queue` 单调调度+5s 跳表探针+`active_claims` 跳过、`Engine` 零克隆投影、`acceptance`/`delivery` 尾读缓存与安全压缩 | `agent-core` 137 / `agent` 272 / `studio` 150 PASS、CI 6/7 绿（手动合入） |
| `#511` | `fix/desktop-wake-queue-and-cold-deep-link` (`c4da1ac65`) | `flowy-shell::wake` 有界 FIFO `wake/` 队列、冷启动深链与登录后邀请弹窗保护 | `shell` 11 / `desktop` 1 / `app` 63 PASS + Windows 真机实测 |

- **主干 `c4da1ac65` 复核（2026-11-27）**：`bun scripts/check-ui-contracts.mjs --self-test && bun scripts/check-ui-contracts.mjs`（24/24）、`cargo fmt --check`、`cargo check --locked -p flowy-desktop`（24.69s），以及 `flowy-shell` 11、`flowy-i18n` 9、`flowy-skills` 70、`flowy-ui` 47（45+2）、`flowy-agent-core` 137（2 ignored）、`flowy-studio` 150、`flowy-app` 63 实跑全绿；`flowy-desktop`（#511 已测 1/1）与 `flowy-agent`（#510 已测 272/272）按纯文档收口指令免重跑（详见 `证据.md` §11.44）。
- **后续优先级**：
  1. **OS 沙箱子进程真机 `Enforced` 接入**（`sandbox.rs` macOS Seatbelt / Linux `bwrap` 真机挂载与 Windows AppContainer 验证）。
  2. **`persona.rs` 注入与跨轮 `dispatch_receipt_id` 细粒度重锚**（§24.2 遗留）。
  3. **V6 外发通道**（仍受 [`BLOCKED.md`](BLOCKED.md) §5 🔴 硬阻塞，未经拍板不动工）。
- **工作树提醒**：暖构建 worktree `.agents/worktrees/feat-router-kernel`（build dir `Temp\flowy18-router-build`）与 `.agents/worktrees/fix-windows-hotpatch`（build dir `Temp\flowy18-s4-build`）已保留备用，严禁双 worktree 共用同一 `CARGO_BUILD_BUILD_DIR`。

---

## 27. `origin/main@331e7a535` PR #512–#527 合流与 `panel/` 拆分收口基线（2026-11-27 · 修订 9.19）

- **主干与分支基线**：`origin/main` 已合入至 `331e7a535`（含 `PR #512` `3a944f8da`、`PR #513` `b0129f38d`、`PR #514` `b528defbb`、`PR #515` `35f6aac8b`、`PR #516` `e5b04ff9b`、`PR #517` `fa2645a68`、`PR #518` `802150b06`、`PR #520` `20addf87f`、`PR #521` `e9048d7bf`、`PR #519` `881c198d3`、`PR #522` `811733fc6`、`PR #523` `de8cd800f`、`PR #524` `8a41e08cf`、`PR #525` `2c0bc9444`、`PR #526` `d16911e75`、`PR #527` `331e7a535`）；当前 PR 分支 `refactor/flowy18-split-panel` 在其基础上完成 `flowy-studio/src/panel.rs` 目录化拆分与文档对账。
- **核心改动速查**：
  1. **Wave 1 CI 与仓内治理（PR #514 / #516 / #520）**：`flowy18-ui.yml` 覆盖 14/14 活跃 crate；清零生产 `.unwrap()` 与移除孤立 `crates/flowy-chat`；修复 Ubuntu APT 与 Windows `resolve_deepest_ancestor` 对 `..` 的路径处理。
  2. **Router/Work 架构文档、Mission Control 看板与按回执重锚（PR #523 / #525 / #526）**：新增 `docs/architecture/router-work-and-tool-extension.md`；上线 Direction C Master-Detail 看板 UI、23 语言 locale key 与 pdf/markdown 主题 token；通过 `dispatch_receipt_id` 完成修订 Worker 卡片逐轮重锚（原 §24.2 遗留项**已完结**）。
  3. **Wave 2 五大核心模块目录映射**：
     - `crates/flowy-agent/src/handle/`（14 文件，PR #519）：`mod.rs`(2301) / `observer.rs`(2112) / `work.rs`(1761) / `router.rs`(1387) / `queue.rs`(1312) / `submit.rs`(1028) / `artifacts.rs`(895) / `workspace_lock.rs`(673) / `sessions.rs`(637) / `boot.rs`(551) / `guards.rs`(460) / `work_events.rs`(328) / `tests.rs`(3316) / `router_tests.rs`(5736)。
     - `crates/flowy-agent/src/artifact_store/`（5 文件，PR #522）：`mod.rs`(1596) / `views_gc.rs`(588) / `deletion_plan.rs`(563) / `io_helpers.rs`(385) / `tests.rs`(1547)。
     - `crates/flowy-studio/src/feed/`（7 文件，PR #524）：`mod.rs`(873) / `cards.rs`(899) / `bubbles.rs`(646) / `process_blocks.rs`(424) / `settlement.rs`(209) / `indicator.rs`(154) / `tests.rs`(1663)。
     - `crates/flowy-studio/src/panel/`（5 文件，本分支）：`mod.rs`(1170) / `submit.rs`(490) / `nav.rs`(733) / `drawer.rs`(606) / `tests.rs`(3087)；`mod/submit/nav/drawer` 全部采用显式非通配符导入，`#[cfg(test)] mod tests;` 固定置底以保持 `check-ui-contracts.mjs` 全量扫描，`tests.rs` 同步 `include_bytes!("../../examples/fixtures/preview/sample.pptx")` 相对路径且未改任何测试断言。
     - `crates/flowy-app/src/app/`（25 文件，PR #527）：`mod.rs`(1451) / `skills.rs`(2876) / `dock.rs`(1811) / `quick_search.rs`(1065) / `settings.rs`(1045) / `conversation_actions.rs`(909) / `gallery.rs`(892) / `projects.rs`(844) / `login.rs`(806) / `more.rs`(778) / `artifact_preview.rs`(600) / `ota.rs`(555) / `titlebar.rs`(541) / `invite.rs`(538) / `bootstrap.rs`(487) / `flex.rs`(483) / `cloud_sync.rs`(455) / `appearance.rs`(433) / `login_flow.rs`(380) / `skills_jobs.rs`(376) / `library_actions.rs`(357) / `artifact_cleanup.rs`(285) / `state.rs`(221) / `tests.rs`(1516) / `artifact_tests.rs`(494)。
- **实测验证基线（2026-11-27）**：`cargo fmt --all -- --check` 0 diff；`bun scripts/check-ui-contracts.mjs --self-test`（24 夹具 PASS）与 `bun scripts/check-ui-contracts.mjs` PASS；`cargo test -p flowy-studio --locked` **175 passed; 0 failed; 2 ignored**；`cargo clippy --workspace --all-targets --locked -- -D warnings` 0 告警；`cargo check -p flowy-desktop --locked` PASS；`panel/` 5 文件均为纯 LF（`hasCR=False`），生产文件 0 `::*;` 与 0 `.unwrap(`。
- **后续优先级**：
  1. **在研并行分支协同**：`.agents/worktrees/feat-work-capability-seam`（分支 `feat/flowy18-work-capability-provider`，触及 `flowy-agent-core/src/tools/{provider,registry}.rs`、`flowy-agent/src/handle/provider_tests.rs`、`docs/architecture/{README,router-work-and-tool-extension}.md`）仍在开发中，注意避免接口冲突。
  2. **OS 沙箱子进程真机 `Enforced` 接入**（`sandbox.rs` macOS Seatbelt / Linux `bwrap` / Windows AppContainer）。
  3. **`persona.rs` 注入**（原 §24.2 `dispatch_receipt_id` 已由 PR #526 完成，此处仅余 `persona.rs`）。
  4. **V6 外发通道**（仍受 [`BLOCKED.md`](BLOCKED.md) §5 🔴 硬阻塞，未经拍板不动工）。
- **工作树现状清单（仅盘点，仍严格遵守一 worktree 一独立 `CARGO_BUILD_BUILD_DIR`）**：
  - 保留暖构建工作树：`.agents/worktrees/fix-windows-hotpatch`（本收口分支 `refactor/flowy18-split-panel`）
  - 保留在研工作树：`.agents/worktrees/feat-work-capability-seam`（`feat/flowy18-work-capability-provider`）
  - 已合主干可确认后清理候选：`.agents/worktrees/docs-router-work-arch`（PR #523 已合）、`.agents/worktrees/feat-router-kernel`（PR #525 已合）、`.agents/worktrees/feat-flowy-local-receipt`（PR #526 已合）

### 27.1 修订 9.19 合流收口补正（`origin/main@3e66290a9` · PR #528 合入后全账对齐）

- **PR #528 合流确认**：上文 §27 中所述 `refactor/flowy18-split-panel` 已于 `2026-11-27T04:16:54Z` 以 `3e66290a9` 正式合入 `origin/main`（成为 `PR #528`）。自修订 9.17（`c4da1ac65`）以来，主干已累计合入 **16 笔 PR**：`#512` 与 `#514–#528`（其中 `#513` `b0129f38d` 为修订 9.17 文档收口 PR）。
- **`persona.rs` 明确暂缓边界**：§27 后续优先级 #3 中的 `persona.rs` 注入**已由需求方明确拍板暂缓（「个性化先不做」）**，后续排期勿主动开工，待需求重开后再启动。
- **跨 Crate 最新指标与删除项补齐**：
  - 最新单测与基准真值：`flowy-agent-core` **138 passed / 2 ignored**（`#526`）、`flowy-agent` 串行 **279 passed / 4 ignored**（`#526`）、`flowy-studio` **175 passed / 2 ignored**（`#528`）、`flowy-app --locked` **67 passed**（`#527`）、全仓 14 活跃 Crate Wave 1 基线 **846 / 846 PASS**（`#516`）；`ChatFeed::sync_history` 200-tick 流式均值降至 **`1,459 µs`**（`#521`，降幅 `-72.2%`），`[M1-BENCH]` 零克隆回执查表 **`9.11 µs/call`**（`#526`）。
  - 23 语种纯 LF 词典（`ar.json`..`zh-TW.json`）在 `#525` 新增 Mission Control 键并裁剪 19 个退役 `board.*` 键后当前对称维持 **975 键**；`flowy-studio/src/overview/flow_modal.rs` 已于 `#525` 物理删除（由 `overview/inspector.rs` 替代）。
  - **跨账本导航**：设计详情见 [`最终方案.md`](最终方案.md) §12.17、全量实测见 [`证据.md`](证据.md) §11.45、待裁决更新见 [`BLOCKED.md`](BLOCKED.md) §6、Router/Work 工具扩展架构见 [`../router-work-and-tool-extension.md`](../router-work-and-tool-extension.md)。
- **并线协同说明**：为避免与在研 worktree `.agents/worktrees/feat-work-capability-seam`（`feat/flowy18-work-capability-provider`）发生合并冲突，修订 9.19 全账同步未触碰 `flowy18_wip/docs/architecture/README.md`。


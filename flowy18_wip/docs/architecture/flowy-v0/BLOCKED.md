# BLOCKED · 待裁决清单

> 拿不准的事写这里，跳过继续做别的。执行者不停下来等。

## 现役待裁决总览（2026-11-27）

| 编号 | 事项 | 当前状态 | 裁决结论与执行边界 |
| --- | --- | --- | --- |
| **§1** | 改动目标锁定在 `flowy18_wip/`（不碰根工作区 Tauri/React） | ✅ **已于 2026-11-25 拍板结案** | 需求方确认目标就是修改 `flowy18_wip/` 下的代码；绝不碰根工作区（`crates/`、`ui/`、`apps/`） |
| **§2** | `flowy18_wip` 目标 crate 与文件定位 | ✅ **已于 2026-11-23 核实结案** | 已落在 `flowy-agent-core` / `flowy-agent` / `flowy-studio` / `flowy-i18n` / `flowy-obs` |
| **§3** | `flowy18_wip` 首次冷构建与二进制入口 | ✅ **已于 2026-11-23 核实结案** | `cargo build -p flowy-desktop --bin flowy-app` |
| **§4** | 任务 14 全类型硬门：无核验器类型的显式豁免边界 | ✅ **已于 2026-11-25 拍板结案** | 采用推荐方案 A：`.pptx` 强制走核验报告；尚无核验器的已知类型（`csv/pdf/md/txt/json/html`）走显式豁免表（`EXEMPT_NO_VERIFIER_V0`）并写审计事件；未知扩展名默认阻断 |
| **§5** | V6 外发通道、凭据存储与单向门策略 | 🔴 **硬阻塞（停等拍板）** | **严禁动工**；不接邮件/Webhook/IM 发送，停在本地交付门与卡片层 |
| **§6** | Super Agent 后续切片排期（#494–#547 已合流 `origin/main@604691e65` · Batch 1+2 `ScrollArea` 全域收口 · `rawScrollAllowlist` 30→3 · Linux/macOS OS 沙箱 CI 与个人自动化 /门禁/缓存治理已落 · Windows AppContainer / 具体 MCP/Skill Provider / `persona.rs` 与 V6 状态） | ✅ **#512–#547 与 `feat/studio-scroll-area` 完成 `ScrollArea` 非特例 100% 收口**（`PR #546` @ `6bc2b231d` 修订 9.23、`PR #547` @ `604691e65` `flowy-app` 17 处清零、Batch 2 `feat/studio-scroll-area` `flowy-studio` 安全 10 处清零；23 语种对称 **1,043 键**；`rawScrollAllowlist` 仅剩 **3 处显式特例** [`overlays.rs#L875` 与 `markdown.rs#L745, #L850`]） · 📋 **下一步：`markdown.rs` 加盐前置重构（独立消息/轮次盐后可考虑再迁）、Windows AppContainer 强制隔离与具体 MCP/可执行 Skill `ToolProvider` 接入** · ⏸️ **`persona.rs` 个性化继续暂缓** · 🔴 **V6（§5）仍硬阻塞** | Batch 1（`flowy-app` 17 处，`PR #547` @ `604691e65`）与 Batch 2（`flowy-studio` 10 处，`feat/studio-scroll-area`）已完成低频滚区迁入 `ScrollArea` 与水平/按轴内缩契约硬化；`overlays.rs#L875` 继续豁免防弹层双层滚动条，`markdown.rs#L745/#L850` 因缺少消息/轮次盐值仍保留显式豁免；Windows AppContainer 与首个真实 `ToolProvider`（MCP / 可执行 Skill）顺位接续，`persona.rs` 继续暂缓（「个性化先不做」），V6（§5）仍硬阻塞 |

---

## 🔴 1. 严重：仓库里有两个项目，我第一轮调研错了对象

**记录时间**：2026-11-23（第 2 轮）

### 事实

这个仓库里有**两个互不相干的应用**：

| | 根 workspace | `flowy18_wip/`（嵌套独立 workspace） |
| --- | --- | --- |
| 技术栈 | Tauri + React | **GPUI（`gpui-pre` 0.3.14 + `gpui-kit` 0.11.0）+ pi-rust** |
| 包名 | `Flowy`（`apps/desktop/Cargo.toml`） | **`flowy-app`**（`crates/flowy-app`） |
| 产物 | `target/debug/Flowy.exe` | **`flowy18_wip/target/debug/flowy-app.exe`** |
| 与方案的关系 | **无关** | **本方案描述的就是它** |

**方案文档里写的是 pi-rust、GPUI、`flowy-agent-core`** —— 全部指向 `flowy18_wip`。

### 我第一轮做错了什么

我在**根 workspace** 里查了半天的「受理链路」：

- `ui/src/renderer/components/chat/SendBox/index.tsx`
- `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx`
- `crates/backend/nomifun-conversation/src/service.rs`
- `allowSendWhileLoading`、`executeSteer`、`NomiSendBox:2355` 的 `disabled`

**这些全都属于 Tauri/React 那个产品，跟 `flowy18_wip` 一点关系都没有。**

**已经写进 `PROGRESS.md` 的那一整节「受理链路：与审查报告不一致」，对本单无意义，需要重做。**

### 我上一轮的「二进制名更正」也是错的

我说任务书里的 `flowy-app.exe` 应改成 `Flowy.exe`。**反了。**

- `flowy-app.exe` **是对的** —— 它是 `flowy18_wip` 的产物
- `Flowy.exe` 是另一个产品的产物

`PROGRESS.md` 里那条更正已撤回。

### 启动命令的正确形态

`flowy18_wip` 有自己的 `target/`，所以相对路径的 cwd 是 `flowy18_wip/`：

```bash
cd flowy18_wip
(./target/debug/flowy-app.exe >/dev/null 2>&1 &)
```

主工作区里已有一个可用的旧二进制（`flowy18_wip/target/debug/flowy-app.exe`），但**它不在我的 worktree 里**——worktree 内的 `flowy18_wip/target/` 是空的，要冷构建。

### 裁决记录（✅ 2026-11-25 已结案）

- **2026-11-25 需求方正式拍板**：「目标就是修改 `wip` 下的代码，其他的按照你的推荐来；备份分支可以删除，执行计划可以更改更新。」
- 本项正式结案；全部后续开发与验收均以 `flowy18_wip/` 为准。以下保留 2026-11-23 原始记录备查。

### 需要裁决（截至 2026-11-23 仍未确认）

- **我按这个走法做了**：本单的代码目标就是 `flowy18_wip/`，根 workspace 一行不碰。
- 如果本单其实要改 Tauri 那个产品，**请纠正我**，我把目标整体换过去。

> **2026-11-23 老爷爷审查时的原话**：「改目标是自行批准的：你写『我替领导拍的板』，
> 而 `V0-执行计划.md` 只允许改根 workspace 路径。我按 lead 口径接受 `flowy18_wip`，
> 但这句话得需求方说一次 —— 现在『需求 A/B/C 达成』只在 `flowy18_wip` 成立，
> 根产品改动文件里没有它。」

**所以这条必须由需求方拍一次，不能由我代劳。** 在此之前，任何「需求 A/B/C 已达成」
的说法都只在 `flowy18_wip` 这个范围内成立，**不能当成整个 Flowy 产品的结论**。

### 为什么这条必须先记

**这正是我自己在 §11.23b 写下的那个病：「我测的全是我想到的那条路」。**
这次更糟——我连「哪条路」都选错了对象。**如果不记下来，下一轮就会在这个错误前提上继续写代码。**

---

## 2. 该看的是 `flowy18_wip` 里哪几个 crate（✅ 已于 2026-11-23 核实结案）

> **结案结论**：实际落点为 `flowy-agent-core`（`acceptance.rs`、`exec_input.rs`、`engine.rs`）、`flowy-agent`（`handle.rs`、`pptx.rs`、`verify.rs`、`delivery.rs`、`publish_tool.rs`）、`flowy-studio`（`composer.rs`、`panel.rs`、`studio.rs`）、`flowy-i18n`（23 个 `locales/*.json`）。以下保留第 2 轮原始排查记录备查。

本单要动的三处，在 `flowy18_wip` 里分别对应（**待逐条核实**）：

| 要改的东西 | 根 workspace（错） | 应该是（待核实） |
| --- | --- | --- |
| 输入框 / 提交 | `ui/src/renderer/...` | `crates/flowy-chat` 或 `crates/flowy-ui` |
| 会话受理 / 回执 | `crates/backend/nomifun-conversation` | `crates/flowy-agent` / `crates/flowy-core` |
| pi-rust loop | — | `crates/flowy-agent-core` |

**下一轮第一件事：把这张表逐条落实成「文件:行号」，核实完才准写代码。**

---

## 3. 首次冷构建（✅ 已于 2026-11-23 核实结案）

> **结案结论**：二进制入口在 `flowy-desktop` 包（`cargo build -p flowy-desktop --bin flowy-app`，冷构建约 14 分钟）。以下保留第 2 轮原始记录。

- 根 workspace `cargo check -p nomifun-ai-agent`：**4m57s**（已测，但属于错对象）
- `flowy18_wip` `cargo build -p flowy-app`：**进行中，耗时未知**

---

## 4. ✅ 全类型交付硬门：无核验器产物的显式豁免边界（2026-11-25 已拍板选 A）

- **拍板结论（2026-11-25）**：需求方确认采用**推荐方案 A**。`.pptx` 强制查核验报告与 sha256；尚无核验器的已知类型（`csv` / `pdf` / `md` / `txt` / `json` / `html`）必须匹配显式豁免表（`EXEMPT_NO_VERIFIER_V0`）并向 `delivery/events.jsonl` 写入带 `exemption_code` 的审计记录；未在表内的未知扩展名一律 `DELIVERY_NOT_VERIFIED` 阻断。以下保留背景记录。
- **背景**：当前 `publish_file`（`flowy18_wip/crates/flowy-agent/src/publish_tool.rs:92-115`）只对 `.pptx` 调用 `DeliveryStore::check`，其余扩展名（如 `.csv`、`.pdf`、`.md`）直接静默入库，且现有集成测试 `artifact_integration_tests.rs` 依赖 `sales.csv` 与 `sample.pdf` 在无核验报告下完成 fixture 交付。
- **待需求方拍板**：在 `doc-pack` / `sheet-pack` 核验器尚未迁入 Rust 前，非 `.pptx` 产物是：
  1. **选项 A（当前默认，可逆）**：维护一份显式豁免表（列明扩展名 + 豁免原因码，如 `EXEMPT_NO_VERIFIER_V0: csv/pdf/md/txt/json`），命中豁免时必须向 `delivery/events.jsonl` 写入带 `exemption_code` 的审计记录，未在表内的未知扩展名一律 `DELIVERY_NOT_VERIFIED` 阻断；
  2. **选项 B（最严）**：除 `.pptx` 外全部拒绝 `publish_file`，直到对应类型的核验器落地（需同步改造或豁免现有 `sales.csv` / `sample.pdf` 集成测试）。

---

## 5. 🔴 V6 外发通道、凭据与单向门策略（硬阻塞，2026-11-25）

- **原因**：对外发送（SMTP / Webhook / 企业微信 / 飞书 / Discord）属于**不可撤回的单向门动作**，涉及真实外部通道选择、凭据存储位置（系统凭据库 vs 环境变量）、收件人白名单与 `submitted / failed / bounced / no_bounce` 四态回执接法（`最终方案.md` §8.5–§8.6）。
- **纪律**：在需求方明确拍板通道类型、测试沙箱与凭据方案前，**停在任务 18（本地交付门 + override UI + 核验结论卡 + L3 提示），一行外发代码不写**。

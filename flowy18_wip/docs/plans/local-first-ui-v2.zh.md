# Flowy Local-First UI v2 与内核 6+4 接口解耦架构 · 技术方案与溯源手册

> **状态**：✅ 已实现、收尾并就绪待合并（分支 `feat/flowy-local-ui-v2`，已无冲突线性 `rebase` 至 `feat/flowy-v0-rc` @ `10b99bac0`，完整对接内核 `71cdb8e92` + `65772f857` 全部 6+4 接口）  
> **提交溯源**：`10992fbe8`（UI v2 看板/弹窗/驾驶舱初版）→ `df62c4d98`（代码审查 13 项深度收敛）→ `1c8161bcb`（`flowy-agent` 6 接口架构重构）→ `6977dda51`（落盘设计文档）→ `6d81e6fd2`（模块拆解与组件去重）→ `a0fe5df13`（深度审查 5 项修复 + 接入 `65772f857` 4 项只读/唤醒接口）  
> **前序基线**：[super-agent-ui-and-motion.zh.md](super-agent-ui-and-motion.zh.md)  
> **核心原则**：**内核自治调度，UI 降维为纯订阅展示层；以本机办公交付为主体（全局看图形 KPI，细节进居中弹窗看流程图），严守 0 冲突文件硬边界、零生产 mock、零磁盘直读、零隐式内部格式耦合、外部打开扩展名白名单门禁、23 语言全对称。**

---

## 1. 背景、设计初衷与痛点演化

### 1.1 业务定位与设计初衷
Flowy Desktop（`flowy18_wip`）定位为**本机优先（Local-First）的办公自动化桌面工作台**，面向需要同时推进多个本机文件任务（Excel 汇总、PPT 汇报、Word 合同、本机脚本与归档）的用户，而非纯云端多 Bot 聊天室。

本轮重构围绕三个核心初衷展开：
1. **解决窄屏响应式塌陷**：早期方案试图在三栏并排主界面中常驻横向流程图与右侧审批抽屉，在 960–1280px 常见笔记本视口下必然出现 KPI 竖排挤压、流程图裁切与文字重叠。因此确立**「主画布只做自适应图形 KPI + 按会话分层摘要卡，点击任意会话弹出宽幅居中流程图 Modal」**的信息分层。
2. **彻底剥离 UI 侧队列调度器**：此前 `StudioPanel` 在 UI 33ms 节拍器里内嵌了 5 秒定时轮询，手动推断 `has_active_claim()` / `claimed_session_status()` 并调用 `finish_accepted()` / `run_accepted_once()`，既与内核职责倒挂，又容易在后台队列推进时干扰当前焦点会话。在内核 `feat/flowy-v0-rc`（`71cdb8e92` + `65772f857`）导出 6+4 解耦接口后，我们在未发布期果断一次性完成对接重构并清除全部临时补偿，消除历史债。
3. **跨 worktree 零冲突协作**：内核侧与 UI 侧并行演进，必须通过窄投影适配器（[ports/mod.rs](../../crates/flowy-studio/src/ports/mod.rs)）和严格文件白名单实现硬隔离，确保双方合并 0 冲突、0 返工。

### 1.2 演进前后能力对比矩阵

| 维度 | 改造前（v0 基线 `f8fb6b491`） | UI v2 第一阶段（`10992fbe8` / `df62c4d98`） | UI v2 × 6+4 接口闭环（`1c8161bcb`..`a0fe5df13`） |
| --- | --- | --- | --- |
| **全局任务可视性** | 仅侧边栏文字列表，无法一眼识别卡点与产物分布 | 自适应 `2×2 / 4×1` GPU 环形图 KPI + 会话分层卡 + 居中流程图弹窗 | `visible_acceptances()` 全 7 态持久化投影、退避期/死信/隔离跨重启不丢失、契约交付进度条 |
| **队列调度归属** | UI 5s 定时泵手动调 `finish_accepted` / `run_accepted_once` | 暂留旧泵（准备切换） | 生产态 `set_autonomous_drain(true)` 移交内核 Tokio 自治，`apply_config` 自动唤醒，UI 纯订阅 `AgentUpdate::Receipt` |
| **任务取消控制** | `stop_if_active` 隐式依赖当前焦点会话，无法取消排队回执 | 抽屉可停后台会话，仍走 `stop_if_active` | 统一 `cancel_task(TaskTarget::Session \| Receipt)`，登出全量取消在跑会话，诚实反馈 `FinalizingNotCancellable` |
| **卡片动作入口** | 核验放行、交付重试分散直调底层方法，无死信恢复 | 弹窗补齐两步核验放行，仍分头调用 | 统一收口 `apply_card_action(OverrideDeliver \| RetryDelivery \| RetryDeadReceipt)`，内核自动取代旧死信 |
| **契约进度与溯源** | 仅展示单文件核验卡与聊天流 | 新增脱敏 Trace 与 JSONL 导出 | `work_status(work_id)` + `session_work_status(session_id)` 覆盖队列与直提全场景契约进度及缺失告警 |
| **安全与关停边界** | 直接 `cx.quit()` 无刷盘钩子；预览外部打开无白名单 | 同前 | `FlowyApp::new` 单一 `cx.on_app_quit` 异步 `prepare_shutdown()`；外部打开强制 `can_open_with_default_app` + `is_file()` 双门禁 |

---

## 2. 方案全景与架构设计

### 2.1 端到端数据流与控制流拓扑

```mermaid
flowchart LR
  subgraph App["flowy-app 宿主壳层"]
    Dock["三段侧边栏 (dock.rs)<br/>新任务 · 工作台看板 · 资料库 · 技能<br/>本机项目折叠行 + 最近任务"]
    Host["顶层弹层、设置与退出总闸 (mod.rs / settings.rs)<br/>OverlayHost · 顶层流程图 Modal<br/>cx.on_app_quit · 语言同步 · 登出清理"]
  end

  subgraph Studio["flowy-studio 纯订阅展示层"]
    Intent["意图与控制 (panel.rs / work_actions.rs)<br/>send_prompt / accept_deferred / prepare_for_sign_out<br/>cancel_task / apply_card_action / open_file_externally"]
    Sub["事件订阅泵 (33ms)<br/>消费 AgentUpdate 全量事件<br/>同步 visible_acceptances() · 关闭 5s 手动调度"]
    Ports["窄投影适配器 (ports/)<br/>board.rs 脏缓存快照 · session_work_status<br/>trace.rs 脱敏日志 · local_env.rs 环境检查"]
    Views["四大交互面<br/>overview/ 看板与居中弹窗 · menus.rs 弹层<br/>composer.rs 4 筹码驾驶舱 · deliverables.rs 三模抽屉"]
  end

  subgraph UI["flowy-ui 零全局原语库"]
    Charts["charts.rs<br/>donut_chart / progress_ring / flow_connector"]
    Overlays["overlays.rs<br/>centered_modal_sheet"]
    Tokens["theme.rs<br/>双主题语义 Token"]
  end

  subgraph Kernel["flowy-agent 宿主内核 (65772f857 · 本分支零修改)"]
    Drain["Autonomous Drain<br/>Tokio 自治推进 · apply_config 自动唤醒"]
    Ctrl["控制与卡片网关<br/>cancel_task / apply_card_action / 死信取代"]
    Truth["持久化真相源<br/>visible_acceptances / session_work_status<br/>SessionIndex / WorkContract / DeliveryStore"]
    Shut["优雅关停<br/>prepare_shutdown"]
  end

  Dock -->|"Section::Board / Studio"| Views
  Host -->|"SkillChip · 语言同步 · 顶部渲染槽"| Views
  Views -->|"用户操作"| Intent
  Intent -->|"background_executor / 同步审计"| Ctrl
  Intent -->|"apply_config 自动唤醒"| Drain
  Host -->|"异步等待刷盘"| Shut
  Drain --> Truth
  Ctrl --> Truth
  Shut --> Truth
  Truth -->|"AgentUpdate::Receipt / Session / Artifacts"| Sub
  Sub -->|"结构化变更或 1.2s 节流"| Ports
  Ports --> Views
  Views --> Charts
  Views --> Overlays
  Charts --> Tokens
```

### 2.2 核心目标 vs 明确非目标

| 类别 | 明确边界 |
| --- | --- |
| **核心目标** | ① 全局可视化工作台 + 宽幅居中流程图弹窗；② 4 筹码本机驾驶舱与三模抽屉；③ 全面切换 `71cdb8e92` + `65772f857` 全部 6+4 接口并将 UI 降维为纯订阅层；④ 外部打开白名单门禁、23 语言对称与双主题零硬编码；⑤ 模块解耦与 0 冲突文件边界。 |
| **明确非目标** | ① 严禁修改 `flowy-agent`、`flowy-agent-core`、`flowy-shell` 及共享文档 `docs/architecture/flowy-v0/*`；② 严禁引入新第三方 Rust crate 或调用 `gpui_kit::init(cx)`；③ 严禁在生产代码中塞入 mock 数据或直读磁盘状态文件；④ 严禁 UI 猜测并拼接内核内部 `submission-{id:032x}` 格式。 |

### 2.3 五条架构不变量（后续维护不可打破）
1. **0 冲突硬边界**：UI 侧只允许改动 `flowy18_wip/crates/{flowy-ui, flowy-studio, flowy-app, flowy-i18n}/` 与 `flowy18_wip/docs/plans/`。需要新内核只读能力时一律通过协同需求单由内核侧补充。
2. **双轨隔离兼容单测**：`StudioPanel::new_background`（生产 `FlowyApp`）必须开启 `agent.set_autonomous_drain(true)`；`StudioPanel::new`（`cfg!(test)` 确定性单测）保持 `false`，所有旧手动 drain 代码必须锁在 `if !self.agent.autonomous_drain()` 守卫内，不得在生产路径运行。
3. **零渲染帧 I/O**：`Render::render` 与空闲 33ms 节拍严禁执行 `fs::metadata`、`Path::is_file()` 或磁盘扫表；`WorkBoardSnapshot` 与 `LocalEnvSnapshot` 仅在结构化 `AgentUpdate`、项目/配置/语言变更、打开看板或流式 1.2s 节流窗口更新；渲染期外部打开按钮仅做纯字符串白名单检查，常规文件存在性校验放进点击回调。
4. **严格不透明 ID 与安全打开契约**：`work_status(work_id)` / `session_work_status(session_id)` 仅传内核不透明 ID；`CardAction::OverrideDeliver` 必须传 `ChatLine::Verification` 里的真实非空 `report_id`；外部 `open::that` 必须统一经 `open_file_externally` 通过 `flowy_agent::can_open_with_default_app` 门禁。
5. **单一退出总闸**：关闭刷盘必须统一收口在 `FlowyApp::new` 的 `cx.on_app_quit`，禁止在各个按钮或窗口回调里重复调用 `prepare_shutdown()` 造成遗漏或竞态。

---

## 3. 核心设计与模块规范

### 3.1 模块职责与代码锚点

| Crate | 文件 | 职责与关键改动 |
| --- | --- | --- |
| `flowy-ui` | [theme.rs](../../crates/flowy-ui/src/theme.rs)、[charts.rs](../../crates/flowy-ui/src/charts.rs)、[overlays.rs](../../crates/flowy-ui/src/overlays.rs) | 双主题 `file_kind_*` 语义 Token；基于底层 `Arc`/`Pie` + `canvas` 自绘 `donut_chart` / `progress_ring` / `flow_connector`（免全局 Theme、唯一动画 ID）；免 `viewport_size()` 的 `centered_modal_sheet`。 |
| `flowy-studio` | `ports/`（[mod.rs](../../crates/flowy-studio/src/ports/mod.rs)、[board.rs](../../crates/flowy-studio/src/ports/board.rs)、[trace.rs](../../crates/flowy-studio/src/ports/trace.rs)、[local_env.rs](../../crates/flowy-studio/src/ports/local_env.rs)） | `BoardDataSource`、`WorkBoardSnapshot`、`WorkContractProgress`（映射 `work_status` + `session_work_status`）、`LocalEnvSnapshot`、`collect_session_traces`（含 JSON/JWT 密钥脱敏与缺失项告警）、`display_path`（剥离 Windows `\\?\` 与 `\\?\UNC\`）。 |
| `flowy-studio` | [panel.rs](../../crates/flowy-studio/src/panel.rs)、[work_actions.rs](../../crates/flowy-studio/src/work_actions.rs)、[menus.rs](../../crates/flowy-studio/src/menus.rs) | `set_autonomous_drain(true)` 生产启用；`AgentUpdate::Receipt` 与 `visible_acceptances()` 无状态同步；`cancel_task_target` / `apply_card_action` / `prepare_for_sign_out` / 外部打开白名单门禁 `open_file_externally`；本机环境与驾驶舱弹出菜单。 |
| `flowy-studio` | `overview/`（[mod.rs](../../crates/flowy-studio/src/overview/mod.rs)、[flow_modal.rs](../../crates/flowy-studio/src/overview/flow_modal.rs)、[trace_panel.rs](../../crates/flowy-studio/src/overview/trace_panel.rs)）、[deliverables.rs](../../crates/flowy-studio/src/deliverables.rs)、[studio.rs](../../crates/flowy-studio/src/studio.rs)、[composer.rs](../../crates/flowy-studio/src/composer.rs) | 看板 KPI 与会话分层卡、顶层居中流程图弹窗（含两步放行/停止/取消排队/死信重投/契约进度/白名单文件打开）、三模右侧抽屉（含 `session_work_status` 契约进度）、4 筹码驾驶舱。 |
| `flowy-app` | [mod.rs](../../crates/flowy-app/src/app/mod.rs)、[dock.rs](../../crates/flowy-app/src/app/dock.rs)、[settings.rs](../../crates/flowy-app/src/app/settings.rs)、[projects.rs](../../crates/flowy-app/src/app/projects.rs)、[skills.rs](../../crates/flowy-app/src/app/skills.rs) | `cx.on_app_quit` 挂接 `agent.prepare_shutdown()`；`sign_out` 调 `prepare_for_sign_out`；`choose_locale` 联动刷新技能清单；顶层 `flow_modal_overlay` 渲染槽；三段侧边栏待拍板优先级修复；项目菜单一键跳转筛选看板。 |
| `flowy-i18n` | `locales/*.json`（全 23 语种） | 对称新增 UI v2 与 6+4 接口状态/提示全部 key，经 `every_locale_has_the_same_keys` 与 `check-ui-contracts.mjs` 强校验。 |

### 3.2 关键异步交互时序

#### 时序 A：启动云端延迟就绪 → `apply_config` 自动唤醒 → `visible_acceptances()` 与死信自动取代

```mermaid
sequenceDiagram
  autonumber
  participant App as FlowyApp
  participant Panel as StudioPanel
  participant Agent as AgentHandle (65772f857)

  App->>Panel: StudioPanel::new_background (disconnected_config)
  Panel->>Agent: with_config_background + set_autonomous_drain(true)
  Note over Agent: Initialized 时 !can_run_turns()，首轮 kick 直接退出
  App->>Agent: apply_config(live_cloud_config)
  Note over Agent: can_run_turns() && autonomous_drain -> 内核自动 kick_autonomous_drain()
  App->>Panel: notify_cloud_ready() -> sync_receipts() + refresh_board_snapshot()
  Agent-->>Panel: AgentUpdate::Receipt { id, Running -> Retrying -> Dead }
  Panel->>Agent: visible_acceptances()（持久化真相源，跨重启可见）
  Panel->>Agent: background_executor: apply_card_action(RetryDeadReceipt { id })
  Agent-->>Panel: Receipt(old_id, Cancelled) + Receipt(new_id, Queued)
  Panel->>Panel: sync_receipts() 自动移除旧死信并刷新看板与浮层
```

#### 时序 B：显式取消、核验两步放行与单一退出总闸

```mermaid
sequenceDiagram
  autonumber
  participant User as 用户操作
  participant Panel as StudioPanel / FlowyApp
  participant Agent as AgentHandle (65772f857)

  User->>Panel: 点击「停止此项」或「取消排队」
  Panel->>Agent: background_executor: cancel_task(TaskTarget::Session / Receipt)
  alt 可取消
    Agent-->>Panel: Ok(CancelOutcome::Cancelled) + AgentUpdate::Receipt / Session
    Panel->>Panel: 刷新聊天流、队列与看板
  else 正在交付收尾
    Agent-->>Panel: Ok(CancelOutcome::FinalizingNotCancellable)
    Panel->>User: 显示 notice.cancel_finalizing 诚实提示
  end

  User->>Panel: 流程图弹窗勾选确认 -> 点击「强制放行交付」
  Panel->>Agent: 同步 apply_card_action(OverrideDeliver { session_id, report_id, artifact_path })
  Agent-->>Panel: Ok(ArtifactDelivered) + 写入审计链
  Panel->>Panel: 同步刷新 feed / board 并发射 ArtifactsChanged

  User->>Panel: 托盘 QuitApp / Ctrl+Q / 关窗按钮 / OS 关闭
  Panel->>Agent: FlowyApp cx.on_app_quit -> background_executor: prepare_shutdown()
  Agent-->>Panel: 暂停自治排空、停止可取消任务、刷盘后退出进程
```

### 3.3 `ReceiptState` 全生命周期 UI 投影矩阵

| `ReceiptState` | 进度估算 | 语义色 Token | 看板 KPI 归类 | `has_blocker` | 弹窗 / 抽屉可执行动作 |
| --- | --- | --- | --- | --- | --- |
| `Queued` | `15%` | `theme.muted()` | `running_count`（活跃队列） | `false` | 取消排队（`TaskTarget::Receipt`） |
| `Running` | `45%` | `theme.status_active()`（脉冲点） | `running_count` | `false` | 取消排队 / 停止会话 |
| `Retrying` | `25%` | `theme.status_attention()`（脉冲点） | `running_count` | `false` | 取消排队（`TaskTarget::Receipt`） |
| `Quarantined` | `10%` | `theme.status_attention()` | `approval_count`（需关注卡点） | `true` | 查看契约/Trace 详情 |
| `Dead` | `10%` | `theme.danger()` | `approval_count`（需关注卡点） | `true` | 重新入队执行（`CardAction::RetryDeadReceipt`，带重入锁） |
| `Completed` | `100%` | `theme.status_verified()` | 退出队列（由交付产物分支体现） | `false` | 仅查看产物与 Trace |
| `Cancelled` | `0%` | `theme.muted()` | 不计入活跃 | `false` | 无 |

---

## 4. 关键决策与权衡矩阵（ADR）

| 编号 | 决策点 | 选定方案 | 放弃的替代方案与理由 |
| --- | --- | --- | --- |
| **D1** | 流程图展示形态 | **全局看板看图形 KPI + 顶层居中 Modal 展示宽幅流程图** | ❌ 三栏主区常驻流程图：960–1280px 窗口下横向空间严重不足，导致 KPI 竖排挤压与流程节点裁切。 |
| **D2** | GPUI 图表与环形图实现 | **基于已锁死 `gpui_kit::base::plot::shape::{Arc, Pie}` + `gpui::canvas` 封装零全局原语（`flowy-ui/src/charts.rs`）** | ❌ 调 `gpui_kit::init(cx)` 用高层图表组件：会覆盖现有 `ResolvedTheme` 体系并引入全局状态耦合；❌ 引入新绘图 crate：增加 Windows 构建链风险。 |
| **D3** | 自治队列开关策略 | **生产 `StudioPanel::new_background` 开启 `set_autonomous_drain(true)`；测试 `StudioPanel::new` 保持 `false`** | ❌ 无条件全局开启：会破坏 `panel.rs` 中 4 个依赖同步确定性 `refresh_pending_receipts` 的既有 GPUI 回归单测。 |
| **D4** | 云端就绪唤醒路径 | **依赖内核 `65772f857` 在 `apply_config()` 内自动唤醒；`notify_cloud_ready` 仅刷新 `visible_receipts` 与看板** | ❌ 在 UI 长期保留防御性重调 `set_autonomous_drain(true)`（仅作为 `71cdb8e92` 过渡兜底，已于 `a0fe5df13` 清理）。 |
| **D5** | 死信/退避期回执展示与防重投 | **统一消费 `agent.visible_acceptances()` + `retrying_receipt` 重入锁；依赖内核 `65772f857` 重投时自动取代旧死信** | ❌ UI 维护内存态 `failed_receipts`（跨重启丢失死信且退避期任务闪烁，已于 `a0fe5df13` 删除）；❌ UI 直读磁盘 `AcceptanceQueue`：违反内核分层红线。 |
| **D6** | 契约进度查询覆盖范围 | **队列回执走 `work_status(&receipt.id)` + 真实会话走 `session_work_status(&session_id)`（看板/抽屉/Trace 全覆盖）** | ❌ 对直提会话自行拼装 `"submission-{id:032x}"`：耦合内核私有内部格式且重启即失效。 |
| **D7** | 退出刷盘钩子落点 | **在 `FlowyApp::new` 单一注册 `cx.on_app_quit`，异步执行 `agent.prepare_shutdown()`** | ❌ 在 `titlebar.rs` 与 `on_window_should_close` 分散埋点：会被 `flowy_shell::QuitApp`（托盘退出 + `Ctrl+Q`）直接绕过，且需跨 crate 改 `flowy-shell`。 |
| **D8** | 卡片动作同步 vs 异步执行 | **`OverrideDeliver` 保持同步调用；`RetryDelivery` / `RetryDeadReceipt` / `cancel_task` 走 `background_executor`** | ❌ 把 `OverrideDeliver` 改成异步：直接破坏既有同步单测 `override_delivery_writes_audit_chain_and_persists_badge_across_reopen`；❌ 把 `RetryDelivery`/`cancel_task` 放 UI 线程：存在锁与磁盘 I/O 卡顿风险。 |
| **D9** | 看板与环境检查刷新频率 | **脏缓存 + 结构化 `AgentUpdate` 触发 + 流式输出 1.2s 节流；`local_env` 仅状态变化时重算** | ❌ 每帧 `render()` 或每 token 重建快照：会导致每秒数十次 `fs::metadata` 与 `session_lines` 扫描，直接拖垮帧率。 |
| **D10** | 外部文件打开安全门禁 | **渲染期纯字符串检查 `flowy_agent::can_open_with_default_app`，点击期经 `open_file_externally` 二次校验 `Path::is_file()`** | ❌ 直接 `open::that(&path)`：Windows 上点击 Agent 产出的 `.exe`/`.ps1`/`.bat` 会直接执行；❌ 渲染期调 `Path::is_file()`：违反零渲染帧磁盘 I/O 原则。 |

---

## 5. 踩坑实录与故障溯源手册（Pitfalls & Root-Cause Playbook）

> 供后续维护者与新接入 agent 快速溯源：每一条均为本分支研发、`/code-review` 与对抗式审查中**真实发生并已修复**的问题。

### 5.1 GPUI / `flowy-ui` 渲染与契约坑
1. **`ElementId` 元组类型不匹配（E0277）**
   - **现象**：`charts.rs` 中将 `(elem_id, 1_u64)` 传给 `.id(...)` 编译报错。
   - **根因**：`ElementId` 未实现 `(ElementId, u64)` 嵌套元组，仅支持 `(&'static str, u64)` 或 `(ElementId, &'static str)` 等固定组合。
   - **解法**：改用 `(elem_id, "ring")` / `(elem_id, "flow")` / `(elem_id, "donut")` / `(panel_elem_id, "enter")`；所有动态列表项坚持 `("prefix", idx as u64)`。
2. **弹窗遮罩只盖住中栏、不盖侧边栏与标题栏**
   - **现象**：初版把 `flow_modal_overlay` 放在 `StudioPanel::workspace` 内部渲染，导致弹窗被侧栏裁剪、960px 窗口横向溢出。
   - **根因**：`StudioPanel` 仅占右侧内容区，不拥有整窗视口。
   - **解法**：在 `df62c4d98` 提升至顶层 [mod.rs](../../crates/flowy-app/src/app/mod.rs) 的 `FlowyApp::render` 渲染槽，并收紧流程图节点宽度（`148 / 156 / 172px` + `22px` 连线），使 960px 窗口一屏完整无滚动裁切。
3. **`check-ui-contracts.mjs` 静态门禁拦截**
   - **现象**：任何裸 `rgb(...)`、`div().on_click(...)`、或 23 套语言字典少 key / 占位符不一致都会直接红灯。
   - **解法**：交互容器统一用 `flowy_ui::interactive(id, label, &self.theme)`；颜色一律走 `self.theme.*`；所有新 key 必须一次同步进全部 23 份 `locales/*.json`。

### 5.2 UI 状态机、安全与交互逻辑坑（来自两轮 `/code-review` 修复）
1. **挂载技能发送后输入框草稿无法清空**
   - **现象**：选中技能 chip 发送消息后，提示词仍残留在输入框。
   - **根因**：`send_prompt` 在文本前拼了 `[Skill: ...]\n`，随后异步回调拿拼接后的 `text` 去比对 `prompt_text(cx).trim()`，永远不等。
   - **解法**：在 `df62c4d98` 保留未拼前缀的 `trimmed_draft` 专门用于 `draft_matches` 校验。
2. **`!cloud_ready` 时发送按钮静默失效**
   - **现象**：未连接云模型时点击发送或按回车无任何反馈，用户误以为应用卡死。
   - **解法**：保持按钮可点并派发 `ComposerEvent::CloudNotReady`，自动打开模型菜单并弹出 `notice.model_setup_required` 引导。
3. **0 交付物时右侧抽屉打不开 Steps / Trace**
   - **现象**：抽屉可见性原绑定在 `self.deliverables.is_some()`，对话还没产出文件时用户点不开步骤与 Trace。
   - **解法**：拆分独立的 `right_drawer_open: bool` 开关，无文件时默认切到 `RightDrawerTab::Steps`。
4. **Windows `\\?\` 路径前缀泄漏与 `project_busy` 掩盖待拍板**
   - **现象**：核验卡与抽屉偶现 `\\?\D:\...` 路径；侧栏项目有任务同时在跑和待审批时只显示青绿运行点，隐藏了琥珀待拍板提示。
   - **解法**：统一经 `ports::display_path_str` 剥离 `\\?\` / `\\?\UNC\`；侧栏徽标优先级固定为 `attention > running > idle`。
5. **Trace 脱敏漏掉 JSON 键值对与 JWT**
   - **现象**：早期脱敏仅匹配 `key=val`，对 `{"api_key":"sk-..."}` 与裸 `eyJ...` JWT 失效。
   - **解法**：扩展 `redact_trace_text` 同时识别 `:` 分隔键值、多 token 续行及 `sk-` / `ghp_` / `eyJ` 前缀，并保留 2KB 载荷上限。
6. **外部文件打开绕过安全白名单 & 语言切换/登出状态残留（`a0fe5df13`）**
   - **现象**：Flowchart 弹窗与交付物预览曾直调 `open::that(&path)`；切语言时看板缓存与技能菜单未刷新；登出仅调旧 `agent.stop()` 且未清空 UI v2 状态；会话消失时 `flow_modal_session` 未自清导致 Esc 误拦。
   - **解法**：统一收口 `open_file_externally` 白名单门禁、`choose_locale` + `sync_i18n` 联动刷新、`prepare_for_sign_out` 全量取消与清理、`refresh_board_snapshot` 自动清理失效 Modal。

### 5.3 内核契约与并发控制坑（来自 `carl-idea-king` 对抗审查，已全部闭环）
1. **`apply_config()` 不唤醒 `autonomous_drain`（P1 → 已于 `65772f857` + `a0fe5df13` 根治）**
   - **溯源**：`71cdb8e92` 的 `apply_config()` 未调 `kick_autonomous_drain()`，启动期 `disconnected_config()` 下积压的回执会在云就绪后永久挂起。
   - **最终解法**：内核 `65772f857` 在 `apply_config()` 检测到 `config.can_run_turns() && autonomous_drain` 时自动唤醒；UI 侧 `a0fe5df13` 删除了原 `notify_cloud_ready` 防御性重调补偿。
2. **`pending_acceptances()` 过滤退避期与死信态 + `RetryDeadReceipt` 不关旧死信（P1 → 已于 `65772f857` + `a0fe5df13` 根治）**
   - **溯源**：`acceptance.rs` 中 `pending()` 仅返回 `is_claimable(now_ms)`，退避期 `Retrying`、`Dead`、`Quarantined` 均不可见；且 `71cdb8e92` 重投死信不终结旧条目。
   - **最终解法**：内核 `65772f857` 新增 `visible_acceptances()` 并在重投时自动取代旧死信；UI 侧 `a0fe5df13` 删除了内存态 `failed_receipts` 与 `record_receipt_state`，实现跨重启无状态投影。
3. **`work_status(work_id)` 无法覆盖非队列直提会话（P1 → 已于 `65772f857` + `a0fe5df13` 根治）**
   - **溯源**：`71cdb8e92` 中 `SessionSummary` 与 `ChatLine` 均无 `submission_id`。
   - **最终解法**：内核 `65772f857` 新增 `session_work_status(session_id)`；UI 侧 `a0fe5df13` 在看板分支投影、会话检视抽屉与脱敏 Trace 全量接入，零耦合内部 ID 格式。
4. **`QuitApp` 绕过窗口关闭回调（P2 → 已于 `1c8161bcb` 根治）**
   - **溯源**：[flowy-shell/src/lib.rs](../../crates/flowy-shell/src/lib.rs#L98-L100) 注册了 `cx.on_action(|_: &QuitApp, cx| cx.quit())`，托盘退出与 `Ctrl+Q` 不经过 `on_window_should_close`。
   - **解法**：实证 `gpui-pre 0.3.14` 具备 `Context::on_app_quit`，在 `FlowyApp::new` 一处拦截所有 `cx.quit()` 路径并异步完成 `prepare_shutdown()`。

### 5.4 Windows 构建与 GUI 自动化环境坑
1. **沙箱 `0xC0000142` 与构建命令选择**：在无交互式 Win32 窗口站的受限沙箱中，普通 `cargo check` 会因 `gpui-pre` build script 退出码 `0xC0000142` 失败，且拉起 `flowy-app.exe` 或运行链接 GPUI/Win32 的测试二进制会在进入 `main()` 前由 `user32.dll` 返回 `0xC0000142`（`STATUS_DLL_INIT_FAILED`）；而 `cargo clippy --tests`、`cargo test --no-run`、`cargo build -p flowy-desktop --bin flowy-app` 可完整编译链接（`rc.exe` 仅告警保留默认图标，退出码 0）。
2. **Windows Defender `os error 1224`**：高频并发写同一大源码文件（如 `panel.rs`）或改完立刻跑 `cargo fmt` 偶发 `user-mapped section open` 锁定，按单文件串行编辑并重试即可。
3. **本机实机 GUI E2E 与截图 6 坑**：详见已沉淀的全局 skill `flowy-gpui-e2e`（单次 `.ps1` 会话内用 Git Bash 后台拉起、过滤 `113×113` 无标题悬浮球、`title -eq 'Flowy'`、查静态文本不带 `-i`、双层 `%APPDATA%\Flowy\Flowy\data` 原子备份还原）。

---

## 6. 验收标准、验证梯子与内核侧协同闭环

### 6.1 验收用例与当前覆盖状态

| 编号 | 验收场景 | 断言口径 | 当前状态 |
| --- | --- | --- | --- |
| **TC-UI-01** | 0 冲突文件边界 | `git diff --name-only feat/flowy-v0-rc...HEAD` 0 命中内核、`flowy-shell` 与共享文档 | ✅ 已通过（51 个文件全在 UI 白名单） |
| **TC-UI-02** | UI 静态契约与 23 语言对称 | `check-ui-contracts.mjs` + `every_locale_has_the_same_keys` 0 错误 | ✅ 已通过 |
| **TC-UI-03** | 零全局 GPU 图表与动效原语 | `flowy-ui` 42 项单测（含 `charts_paint_without_global_component_theme_init`） | ✅ 已通过（42/42） |
| **TC-UI-04** | 严格静态检查与二进制编译 | `cargo clippy --tests -- -D warnings` 0 告警 + `flowy-desktop` 编译通过 | ✅ 已通过 |
| **TC-UI-05** | 契约投影（含 `session_work_status`）与死信卡点分类 | `ports::board::tests::{projection_classifies_failed_verification_as_blocker_with_report_id, work_contract_progress_projects_partial_missing_and_dead_receipts_as_blockers, idle_qa_session_keeps_kpi_donut_counts_consistent, orphan_receipt_for_deleted_session_surfaces_in_unassigned_group, kpi_counts_all_branches_before_six_branch_display_truncation}` | ✅ 已实跑通过（`flowy-studio` 101/101） |
| **TC-UI-06** | 死信/隔离提示、登出同步取消与 `visible_acceptances()` 同步 | `panel::tests::{dead_receipt_lifecycle_surfaces_notice_and_syncs_visible_receipts, prepare_for_sign_out_cancels_queued_receipts_and_invalidates_submission, bind_default_workspace_preserves_prompt_draft, cloud_ready_gate_blocks_when_offline_and_allows_during_catalog_loading}` | ✅ 已实跑通过（`flowy-studio` 101/101） |
| **TC-UI-07** | `apply_config` 自动唤醒 + 云就绪队列同步 | `panel::tests::autonomous_drain_apply_config_and_notify_cloud_ready_syncs_queue` | ✅ 已实跑通过（`flowy-studio` 101/101） |
| **TC-UI-08** | 核验两步放行审计链持久化 | `panel::tests::override_delivery_writes_audit_chain_and_persists_badge_across_reopen` | ✅ 已实跑通过（`flowy-studio` 101/101） |
| **TC-UI-09** | 外部打开白名单安全门禁与 Trace 结构化脱敏 | `deliverables::tests::external_open_gate_blocks_executables_and_missing_files` + `ports::trace::tests::{redact_trace_text_masks_secrets_and_clamps_long_payloads, redact_trace_text_masks_compact_json_non_first_keys_and_url_queries, collect_session_traces_surfaces_unassigned_queue_receipts}` | ✅ 已实跑通过（`flowy-studio` 101/101） |
| **TC-UI-10** | 双主题 × 960px/1280px 实机视觉、弹层交互、离线拦截与远端真实积分闭环 | 通过加固后的 `flowy-gpui-e2e`（Low-IL 自动暂存）实机执行离线 960×700（35/35 步）+ 远端真实 API 1280×820（20/20 步）并全量目检 18 张截图 | ✅ 已实机通过（`log_error_count=0`，`APPDATA_RESTORED`） |

### 6.2 验证梯子（Verification Ladder）

```powershell
# 1. 0 冲突文件边界审计
git diff --name-only feat/flowy-v0-rc...HEAD

# 2. 格式与 UI 契约门禁
cargo fmt --manifest-path flowy18_wip/Cargo.toml --check
node flowy18_wip/scripts/check-ui-contracts.mjs

# 3. 严格 Clippy 与无头单测
cargo clippy --manifest-path flowy18_wip/Cargo.toml --tests -p flowy-i18n -p flowy-ui -p flowy-studio -p flowy-app -- -D warnings
cargo test --manifest-path flowy18_wip/Cargo.toml -p flowy-ui
cargo test --manifest-path flowy18_wip/Cargo.toml -p flowy-i18n

# 4. Studio/App 测试构建 + 桌面二进制构建（宿主机普通桌面终端可直接去掉 --no-run 执行全套 GPUI 单测）
cargo test --manifest-path flowy18_wip/Cargo.toml --no-run -p flowy-studio -p flowy-app
cargo build --manifest-path flowy18_wip/Cargo.toml -p flowy-desktop --bin flowy-app
```

### 6.3 隔壁内核 `65772f857` 4+1 项接口对接闭环记录

> 隔壁内核分支 `feat/flowy-v0-rc` 已于 `65772f857` 落地全部 4+1 项接口，本分支已无冲突 `rebase` 并完成全量接入与 UI 侧临时补偿代码删除：

1. **✅ `apply_config()` 自动唤醒自治队列**：内核在 `config.can_run_turns() && autonomous_drain` 时自动调用 `kick_autonomous_drain()`；UI 侧 `StudioPanel::notify_cloud_ready` 已删除防御性重调 `set_autonomous_drain(true)` 的补偿代码。
2. **✅ `AgentHandle::visible_acceptances(&self) -> Vec<(AcceptedMessage, ReceiptState)>`**：UI 已删除内存态 `failed_receipts` 与 `record_receipt_state`，统一以 `visible_receipts` 驱动工作台看板、后台任务浮层与 Trace 投影；`Retrying`（退避期）、`Quarantined` 与 `Dead` 回执跨重启持久化可见，且看板构建不再逐条重读磁盘状态。
3. **✅ `CardAction::RetryDeadReceipt` 自动取代旧死信**：内核重投成功后自动在日志中终结旧死信并推送 `Cancelled` + 新 `Queued`；UI 侧 `retry_dead_receipt` 删除了本地 `failed_receipts.retain` 手动清理。
4. **✅ `AgentHandle::session_work_status(&self, session_id: &str) -> Option<WorkStatus>`**：看板分支投影（`ports/board.rs`）、右侧会话检视抽屉（`deliverables.rs`）与脱敏 Trace（`ports/trace.rs`）已全面接入，直提会话无需拼装内部 `submission-*` ID 即可跨重启展示完整契约进度与缺失项告警。
5. **✅ `Quarantined` 事件与外部打开安全门禁加固**：同步修复深度审查发现的 5 项问题——Flowchart 弹窗与交付物预览外部打开强制接入 `flowy_agent::can_open_with_default_app` + `Path::is_file()` 白名单门禁（防 `.exe`/`.ps1` 误执行）、语言切换同步刷新看板与技能清单、`sign_out` 统一 `prepare_for_sign_out` 显式取消全量在跑会话并清理 UI v2 状态、切资料库复用 `close_all_popovers()`、会话删除自动关闭过期 Modal、清理 `charts.rs` 死代码与硬编码动画 ID。

### 6.4 深度审查与实机 E2E 闭环收口记录（2026-11-25）

> 经 `/code-review` + `/laoyeye` 深度对抗审查与 Windows 实机 E2E 闭环验证，严守 **0 冲突文件边界**（零改动内核、`flowy-shell` 及 23 语种字典），补齐并验证以下 14 项收口修复：

1. **消灭渲染帧磁盘 I/O（不变量 #3）**：
   - `SessionBoardGroup` 缓存 `session_contract: Option<WorkContractProgress>`，`deliverables.rs` 的 `inspect_session_view` / `inspect_receipt_view` 改为纯读取 `board_snapshot` 与 `visible_receipts`；
   - `StudioPanel` 新增 `cached_trace: Option<(String, Vec<RedactedTraceEntry>)>` 与 `refresh_trace_cache()`，在切 Trace 页签、更新看板、切换会话与关闭弹窗/抽屉时脏刷新；`modal_trace_panel` 零调用 `collect_session_traces`，并将 `format_traces_jsonl` 移入「复制 JSONL」点击回调。
2. **看板 KPI 守恒与孤儿回执收拢（`ports/board.rs`）**：
   - `accumulate_kpi` 移至 `branches.truncate(6)` 之前执行（截断前按卡点/运行优先稳定排序），且不再对 `BranchPhase::Idle` / `Completed` / `Cancelled` 虚增 `total_branches`，确保 `total_branches == approval_count + running_count + delivered_count` 严格守恒；
   - 收拢 `receipts_by_session` 中所有未归属或指向已删除会话的孤儿回执至 `UNASSIGNED_QUEUE_SESSION_ID`，标题复用既有对称 i18n key `"chat.bg_tasks_title"`（「后台任务队列」/ `"Background tasks"`），消除标题与副标题重复。
3. **Trace 未归属队列与密钥结构化脱敏（`ports/trace.rs`）**：
   - `collect_session_traces` 显式支持 `UNASSIGNED_QUEUE_SESSION_ID`（跳过 `session_lines`，收集全部无归属/孤儿回执及契约缺失告警），`deliverables.rs` 回执 Trace 同步接入兜底；
   - 重写 `redact_trace_text` 结构化分隔符扫描器，全面覆盖紧凑 JSON 非首键（`{"user":"a","api_key":"..."}`）、URL Query 参数（`?x=1&api_key=...`）、`Authorization: Bearer <token>`（含 JSON 内嵌）及非敏感键下的 `sk-`/`ghp_`/`eyJ` 凭据。
4. **`!cloud_ready` 发送门禁（方案 A）与登出竞态收口（`panel.rs` / `work_actions.rs`）**：
   - 修正 `StudioPanel::can_submit_prompt()` 布尔反转（`self.cloud_ready || self.catalog_loading || !self.agent.requires_cloud_ready()`），冷启动加载期仍安全入队，断网/未就绪时正确呈现琥珀警示态并触发 `ComposerEvent::CloudNotReady` + `notice.model_setup_required`；
   - `prepare_for_sign_out` 改为先置位 `submission_stop` 并 `invalidate_submission` → 立即 `apply_config(disconnected)` 锁死自治排空 → 同步取消全部 `busy_session_ids` 与非终态 `visible_receipts`（防跨账号扣费竞态）；
   - `retrying_receipt` 升级为 `HashSet<String>` 支持多条死信并发重投锁；新增 `bind_default_workspace()` 供工作区菜单解绑项目时保留输入草稿。
5. **960px 响应式布局、无障碍/UIA 交互与图表测试（`overview/mod.rs` / `studio.rs` / `composer.rs` / `starter/mod.rs` / `charts.rs`）**：
   - `board_kpi_strip` 改为两组 2 卡配对容器，实现 1280px `4×1` 与 960px 对称 `2×2`（消灭 `3+1` 孤行）；
   - 移除 `env-status-pill` 与 `cockpit_chip` 的 `.aria_expanded(...)`（规避 `gpui-pre 0.3.14` 丢弃 `accesskit::Action::Expand` 导致 Windows UIA 点击失效），并为已绑定工作区的 `workspace_chip`（`项目: {name}`）与看板 `board-spark`（`活跃会话 · {title}`）补充作用域无障碍名，彻底消除与侧栏项目行/看板卡片的重名冲突；
   - 放宽英文启动卡技能徽章 `max_w(144px)`（消除 `Code Script Helper` 截断）、增大输入框 `min_h(48px)` + `pb(2px)`、参数化 `charts.rs` `ChartHarness` 真实覆盖空甜甜圈与 `MotionPolicy::Reduced`。
6. **全量验证证据**：
   - **静态门禁**：`git diff --name-only 10b99bac0..HEAD` 0 越界；`cargo fmt --check`、`node flowy18_wip/scripts/check-ui-contracts.mjs`、`cargo clippy --tests -p flowy-i18n -p flowy-ui -p flowy-studio -p flowy-app -- -D warnings` 全部 0 告警通过。
   - **全量单元测试（5 组 201/201 实跑通过）**：`anchor_measurement` 2/2、`flowy-ui` 40/40、`flowy-i18n` 8/8、`flowy-studio` 101/101（含 8 项新增对抗单测）、`flowy-app` 50/50（含此前因工作区筹码重名失败的 3 项项目行交互测试）。
   - **实机 E2E 闭环（`flowy-gpui-e2e` 自动暂存直接指向 `flowy18_wip`）**：
     - 离线 960×700 深/浅 + 中/英（0 积分）：**35/35 步通过**，11 张截图目检通过，`log_error_count=0`，`APPDATA_RESTORED`；
     - 远端真实 API 1280×820（`offline: false`，消耗 10 真实积分 `223,280 -> 223,270`）：**20/20 步通过**，7 张截图目检通过，验证纯对话看板甜甜圈 `0` 守恒、缓存 Trace 真实事件、聊天时间线与余额实时刷新，`log_error_count=0`，`lingering_flowy_app=0`。

---

## 附录：原始技术底稿与历史归档 (Historical & Technical Reference Archive)

> **归档说明**：以下 100% 逐字完整保留初版 UI v2 执行方案底稿（基于 `f8fb6b491` 编写、并于 `c5170e55a` 追加 §6），供历史演进对比与协议审计，未做任何删减。

---

# Flowy Local-First UI v2（可视化工作台看板 + 0 冲突端口体系）执行方案

> **状态**：✅ 已实施并验收（基于 `feat/flowy-v0-rc` @ `f8fb6b491` 切出分支 `feat/flowy-local-ui-v2`）  
> **前序基线**：[super-agent-ui-and-motion.zh.md](super-agent-ui-and-motion.zh.md)  
> **核心原则**：**严格遵守与内核 worktree 的 0 冲突硬边界（仅修改 `flowy-ui` / `flowy-studio` / `flowy-app` / `flowy-i18n` 与 `docs/plans/`，零触碰 `flowy-agent-core` / `flowy-agent` 及共享进度文档）；以本机电脑环境为主体，通过 UI 侧窄投影适配器 `ports.rs` 纯订阅渲染，零生产 mock、零磁盘直读、全 23 语言对称。**

---

## 1. 背景与范式定调

### 1.1 本机优先办公桌面范式
- **非纯云端多 Bot 聊天室**：以「任务 + 可选本机项目目录 + 按需技能挂载」为主干，聚焦真实本机交付（Excel/PPT/Word/脚本/归档）与目录安全边界；
- **全局看图形数据，细节进弹窗看流程图**：解决三栏并排时中栏缩窄导致 KPI 竖排与流程图裁切的响应式痛点；
- **严格零新增第三方 crate**：复用已锁死依赖 `gpui-pre =0.3.14` 与 `gpui-kit =0.11.0` 底层的 `gpui_kit::base::plot::shape::{Arc, Pie}`（避开依赖全局 `gpui_component::init` 会 panic 的高层组件）。

### 1.2 0 冲突硬隔离与内核契约（基于 `feat/flowy-v0-rc` @ `f8fb6b491`）

| 边界项 | 严格约定 |
| --- | --- |
| **UI 允许修改范围** | 仅限 `flowy18_wip/crates/{flowy-ui, flowy-studio, flowy-app, flowy-i18n}/` 与 `flowy18_wip/docs/plans/` |
| **严禁触碰范围** | `flowy-agent-core/`、`flowy-agent/`、`docs/architecture/flowy-v0/{PROGRESS.md, 交接给下一个agent.md, 证据.md}`、根 Tauri/React 工作区 |
| **调度边界** | UI 仅做纯订阅展示层，不围绕 `has_active_claim` / `run_accepted_once` 建新状态机 |
| **核验放行边界** | `override_deliver_artifact(session_id, report_id, artifact_path)` 严格透传 `ChatLine::Verification` 的真实非空 `report_id`，保留两步确认门禁并承接内核报错 |
| **存储与性能边界** | 严禁 UI 直读 `sessions/{history,index}.json`、`delivery/*.jsonl`、`acceptance/journal.jsonl`；磁盘查询类 `AgentHandle` 方法（`overridden_artifact_hashes` / `receipt_state`）仅在 `AgentUpdate` / 项目变更 / 打开看板时重建缓存快照，绝不在 `render()` 或空闲 33ms 节拍调用 |

---

## 2. 端到端界面与投影架构

```mermaid
flowchart TD
  subgraph App["flowy-app 宿主与三段侧边栏"]
    Dock["三段侧边栏 (dock.rs)<br/>新任务 / 工作台看板(角标) / 资料库 / 技能<br/>本机项目(折叠不掩盖待拍板) + 最近任务"]
    Host["OverlayHost & Skills 同步 (mod.rs / skills.rs)<br/>ESC 统一收口弹窗与弹层 · 同步真实技能列表"]
  end

  subgraph Studio["flowy-studio 工作面与投影适配器"]
    Ports["ports.rs (BoardDataSource / Trace / LocalEnv)<br/>单向只读订阅 AgentHandle · 剥离 Windows 前缀 · 脱敏 Trace"]
    Board["overview.rs 可视化工作台看板<br/>自适应 2x2 / 4x1 图形 KPI + 按会话分层卡"]
    Modal["centered_modal_sheet 居中流程图弹窗<br/>Chief Router -> 连线 -> 进度环节点 -> 审批/交付 + Trace"]
    Cockpit["composer.rs 4 筹码本机驾驶舱<br/>模型/Auto · 工作区目录 · L3 语义核验 · 技能(/)"]
    Drawer["deliverables.rs 右侧三模抽屉<br/>交付物(定位目录/升级项目) | 步骤 | Trace 脱敏日志"]
  end

  subgraph UI["flowy-ui 零全局 GPU 原语"]
    Charts["charts.rs<br/>donut_chart / progress_ring / flow_connector"]
    Overlays["overlays.rs<br/>centered_modal_sheet (免 viewport_size)"]
    Theme["theme.rs<br/>file_kind_{sheet,slides,doc,other} 双主题 Token"]
  end

  subgraph Kernel["flowy-agent (只读对接 · 本分支零修改)"]
    Handle["AgentHandle (f8fb6b491)<br/>list_sessions / session_lines / busy_session_ids<br/>pending_acceptances / receipt_state / overridden_artifact_hashes<br/>override_deliver_artifact / set_semantic_verify_enabled"]
  end

  Dock -->|"Section::Board / Studio"| Board
  Host -->|"SkillChip 列表"| Cockpit
  Board -->|"点击会话"| Modal
  Board -->|"读取脏缓存快照"| Ports
  Modal -->|"两步核验放行 (含 report_id)"| Handle
  Cockpit -->|"L3 开关 / 技能前缀提交"| Handle
  Drawer -->|"脱敏 Trace & 升级项目"| Ports
  Ports -->|"仅在 AgentUpdate 刷新"| Handle
  Board --> Charts
  Modal --> Charts
  Modal --> Overlays
  Charts --> Theme
```

---

## 3. 核心模块实现清单

### 3.1 `flowy-ui` 零全局图表与弹窗原语
- [theme.rs](../../crates/flowy-ui/src/theme.rs)：新增 `file_kind_sheet`、`file_kind_slides`、`file_kind_doc`、`file_kind_other` 语义 Token，同时适配暖墨浅色与暖炭深色主题。
- [charts.rs](../../crates/flowy-ui/src/charts.rs)：基于 `gpui_kit::base::plot::shape::{Arc, ArcData, Pie}` + `gpui::canvas` + `PathBuilder` 实现 `donut_chart`、`progress_ring`、`flow_connector`，受 `MotionPolicy` 门控，不依赖 `gpui_kit::init(cx)`，附带 2 项 `#[gpui::test]` 回归测试。
- [overlays.rs](../../crates/flowy-ui/src/overlays.rs)：新增 `centered_modal_sheet`，全窗口 flex 居中 + 响应式最大宽度，零 `window.viewport_size()` 调用。

### 3.2 `flowy-studio` 数据投影、可视化看板、4 筹码驾驶舱与三模抽屉
- [ports.rs](../../crates/flowy-studio/src/ports.rs)：定义 `WorkBoardSnapshot`、`BoardDataSource`、`SessionBoardGroup`、`WorkBranchSummary`、`BranchPhase`、`RedactedTraceEntry`、`LocalEnvSnapshot`、`SkillChip` 与 `display_path`（剥离 Windows `\\?\` 扩展前缀）；内置密钥/Token 脱敏与载荷截断。
- [overview.rs](../../crates/flowy-studio/src/overview.rs)：实现自适应 `2×2 / 4×1` 图形 KPI 条（`donut_chart` 分支环 + 卡点脉冲卡 + 产物格式分布条 + 会话活跃柱）、按会话分层的视觉管道卡（历史会话自动折叠为单行色点管道）、以及 `centered_modal_sheet` 居中流程图弹窗（含两步核验放行与脱敏 Trace 切页）。
- [composer.rs](../../crates/flowy-studio/src/composer.rs)：升级为 4 筹码本机驾驶舱（`模型/Auto` · `工作区/项目` · `L3 语义核验开关` · `技能 /`），修复 `!cloud_ready` 静默吞回车问题。
- [deliverables.rs](../../crates/flowy-studio/src/deliverables.rs)：升级为 `交付物 | 步骤 | Trace` 三模抽屉，展示剥离 `\\?\` 的清晰路径，补齐 `定位文件夹` 与一键 `升级为项目` 动作，支持复制脱敏 JSONL。
- [studio.rs](../../crates/flowy-studio/src/studio.rs) 与 [panel.rs](../../crates/flowy-studio/src/panel.rs)：接入 `StudioSurface::Board`（看板页不常驻右抽屉）、顶栏 `● 本机就绪` 胶囊与体检弹层（含 `L3 语义复核` 开关直连 `agent.set_semantic_verify_enabled`）、脏缓存 `refresh_board_snapshot()` 与单次 `overridden_artifact_hashes()` 查表。

### 3.3 `flowy-app` 三段侧边栏与 23 语言对称字典
- [dock.rs](../../crates/flowy-app/src/app/dock.rs)：顶部核心 4 入口收口为 `新建任务` · `工作台看板`（带待拍板琥珀/活跃青绿实时角标）· `资料库` · `技能`；修复项目行折叠态与会话行中 `project_busy` 掩盖 `project_attention` 的问题；次级工具（`创作` / `自动化` / `个性化`）与底栏账户/设置全部保留可达。
- [mod.rs](../../crates/flowy-app/src/app/mod.rs) 与 [skills.rs](../../crates/flowy-app/src/app/skills.rs)：新增 `Section::Board`（纳入前进/后退导航栈）、将新建弹层与流程图弹窗接入 `OverlayHost`、同步真实技能清单至 `StudioPanel`。
- `crates/flowy-i18n/locales/*.json`：23 套语言字典 100% 键位对称，所有插值占位符（如 `{count}`）与调用处严格一致。

---

## 4. 给隔壁内核 worktree 的只读接口需求单（后续协同零返工）

当前 UI v2 已完全基于 `f8fb6b491` 既有公开 API 跑通，零 mock。后续内核侧推进自治调度、交互审批与多 Work 协作时，只需按以下最小签名补充，UI 侧替换 `ports.rs` 适配器即可无缝升级（零界面重写）：

1. **交互式工具审批挂起端口**：
   - `AgentHandle::pending_tool_approvals(&self) -> Vec<PendingToolApproval>`
   - `AgentHandle::resolve_tool_approval(&self, approval_id: &str, decision: ApprovalDecision) -> Result<(), &'static str>`
   - `AgentUpdate::ApprovalsChanged`
2. **结构化 Work Trace 脱敏流**：
   - `AgentHandle::work_trace(&self, session_id: &str, limit: usize) -> Vec<WorkTraceRecord>`
3. **默认本机目录懒创建元数据**：
   - `AgentHandle::session_workspace_meta(&self, session_id: &str) -> Option<WorkspaceMeta>`（含展示根路径与 `materialized: bool`）
4. **轻量只读汇总查询（消除重复扫盘）**：
   - `AgentHandle::receipt_states_snapshot(&self) -> HashMap<String, ReceiptState>`
   - `AgentHandle::session_board_summaries(&self) -> Vec<SessionBoardSummary>`（后续补充 `parent_session_id` / `route_label` / `active_skill`）

---

## 5. 验证梯子

```powershell
# 1. 0 冲突文件边界审计（必须 0 命中 flowy-agent-core / flowy-agent / docs/architecture/flowy-v0）
git diff --name-only feat/flowy-v0-rc

# 2. UI 静态契约与 23 语言对称检查
node flowy18_wip/scripts/check-ui-contracts.mjs

# 3. 无头单测（flowy-i18n + flowy-ui）与严格静态检查
cargo test --manifest-path flowy18_wip/Cargo.toml -p flowy-i18n -p flowy-ui
cargo fmt --manifest-path flowy18_wip/Cargo.toml --check
cargo clippy --manifest-path flowy18_wip/Cargo.toml --tests -p flowy-i18n -p flowy-ui -p flowy-studio -p flowy-app -- -D warnings
cargo test --manifest-path flowy18_wip/Cargo.toml --no-run -p flowy-studio -p flowy-app
```

---

## 6. 基于 `feat/flowy-v0-rc`（`71cdb8e92`）的 6 接口架构升级

- **零冲突切齐基线**：`git rebase feat/flowy-v0-rc`（`71cdb8e92`）0 文件重叠，继续严守 UI 侧文件边界（0 触碰内核与 `flowy-shell`）。
- **自治队列与事件订阅**：生产构造 `StudioPanel::new_background` 开启 `agent.set_autonomous_drain(true)`，关闭 UI 侧 5s 轮询与手动 `finish_accepted`/`run_accepted_once`（仅在 `!autonomous_drain()` 保留兼容既有确定性单测）；`notify_cloud_ready` 防御性唤醒 `set_autonomous_drain(true)`；`process_agent_updates` 消费 `AgentUpdate::Receipt { id, state }` 实时同步 `Queued/Running/Retrying/Completed/Cancelled/Quarantined/Dead`。
- **显式取消与统一卡片网关**：全部取消路径迁移至 `agent.cancel_task(TaskTarget::Session | Receipt)` 并诚实承接 `CancelOutcome::FinalizingNotCancellable`；覆盖放行、交付重试与死信重投统一收口至 `agent.apply_card_action(CardAction::OverrideDeliver | RetryDelivery | RetryDeadReceipt)`（含 `retrying_receipt` 防重入与重投后清理旧死信 ID）。
- **契约进度投影与单一退出总闸**：`ports.rs` 通过 `agent.work_status(work_id)` 为持有真实不透明 `receipt.id` 的分支投影 `WorkContractProgress`（交付数 / 要求数 / 缺失交付物）；`FlowyApp::new` 通过 `cx.on_app_quit` 单一钩子异步调用 `agent.prepare_shutdown()` 覆盖全部退出路径。
- **报给内核侧的 4 项后续只读补齐（不阻塞本次合入）**：`apply_config()` 自动 `kick_autonomous_drain()`、`visible_acceptances()`（覆盖退避期与跨重启死信/隔离态）、`RetryDeadReceipt` 终结/取代旧死信防重启复现、`session_work_status(session_id)`（或 `SessionSummary.latest_work_id`）让非队列直提会话也可查 `WorkStatus`。

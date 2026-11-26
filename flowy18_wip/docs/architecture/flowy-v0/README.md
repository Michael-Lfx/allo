# Flowy 首版架构方案（v0）· 索引

[← 架构与决策](../README.md)

> 归属 `flowy18_wip/docs/architecture/flowy-v0/`（原根目录 `docs/architecture/flowy-v0/` 已整体迁回 `flowy18_wip/`）。
> 这里放的是**设计与文字实测证据**，不是代码。原临时 `evidence/` 下的桌面截图（PNG）与本机原始 JSONL 日志已移出 Git 仓库以控制仓库体积并避免包含本机路径，完整实测记录见 `证据.md`；实现进度见 `PROGRESS.md`，待裁决见 `BLOCKED.md`。

## 按你想知道的查

| 文件 | 读它做什么 |
| --- | --- |
| `一页纸.md` | **3 分钟了解全貌。** 它是什么、交什么、不做什么 |
| `实现对照.md` | **每个重要环节：我们选了什么 / 对比方案是什么 / 为什么不选 / 证据在哪。** 想挑刺先读这份 |
| `最终方案.md` | **结论稿（修订 9.10）。** 要动手只读这份：分层、work、能力包、执行、调度、核验、记忆、待人卡、边界、观测、§12.1–§12.8 演进与落地账本、**§12.9 PR-2A 会话级轻量 Router 与多 Work 并发内核落地、对抗加固与 PR-2B 接缝契约** |
| `证据.md` | **全部实测记录（§11.1–§11.37）。** 数字、可重跑目录、真机日志审计、独立内核审查反例闭环、真云端结构化 L3 实测、§11.32–§11.36 实测总账、**§11.37 PR-2A（PR #496）纵向验证、故障注入、深度审查加固与 475/475 门禁账本**及被推翻的结论 |
| `跨平台沙箱与进程回收预留方案.md` | **【跨平台预留蓝图】** Windows 优先通用层（`RunFence` 两阶段回收 + MIT 净室 `shell_policy` + 诚实 `SandboxAvailability`）与 macOS Seatbelt / Linux `bwrap` / Windows AppContainer 接入规格与验收矩阵 |
| `参照.md` | 五款工作台横评、AutoLoop 借鉴分级、sub agent 与 super agent 的区别、**§10.5 OpenAI Dot 四轮实测对照与老爷爷对抗审查分级** |
| `先记.md` | 六件暂时不做的事，每件写明「什么时候该回来看」 |
| `审查交接.md` | **【历史快照】** 2026-11-23 交给外部审查者的材料，含审查结论与修正清单 |
| `V0-执行计划.md` | **【现役执行计划】** 经 2026-11-25 需求方拍板与真机巡检重订的 Phase 1–Phase 4 收口计划（文末完整归档首轮底稿） |
| `交接给下一个agent.md` | **【现役交接主入口】接手先读 §19（PR-2A `feat/flowy-router-kernel` / PR #496）。** 唯一现役状态、模块速查、PR-2B 接线 API 契约、踩坑清单与实施指引 |

## 现状一句话

**方案修订至 9.10（保留 9.1–9.9 全部正文），实测覆盖 §11.1–§11.37。** 需求方已拍板确认目标锁定 `flowy18_wip/` 并采用显式豁免表方案 A；已删除历史冗余备份分支。

**代码侧（2026-11-25 · `feat/flowy-v0-rc@f47804a9d`）**：工作树 `.agents/worktrees/feat-flowy-local-receipt` 已合流 `feat/flowy-super-ui` 并完成 10 轮纯内核可靠性加固与路线 1 v2 收口（`f8fb6b491`..`f47804a9d`，0 碰 UI）：拆解 `handle/` 与 `verify/` 目录、补齐 `RunFence` 两阶段取消回收、MIT 净室跨平台 Shell 静态安全策略 + 诚实沙箱能力层、单次 HTTP 工具调用 `complete_structured`（真云端 L3 实测提速 3.26×）与 §6.8 `PROVENANCE_INVALID` 溯源拦截（`flowy-agent-core` 90 / `flowy-agent` 168 / 真云端 L3 1 全绿，`clippy -D warnings` 0 警告，`cargo check -p flowy-app` 通过）；仅剩 V6 外发通道按 `BLOCKED.md` §5 停等拍板。

**主干合流与外部对照定稿（2026-11-26 · `origin/main@526b2336f` / PR #490 · 修订 9.6）**：`feat/flowy-v0-rc` 已与 UI v2、ClawHub Skills（#487）及文档迁回整合并合入 `main`；同步在 `参照.md` §10.5、`证据.md` §11.32 与 `最终方案.md` §12.4–§12.5 落盘 OpenAI Dot 四轮实测对照、`contract.rs` 7 组真机实跑证伪与四处代码边界审计、以及校准后的三层对象模型 / 混合按需 `ContractResolver` / 持久化 `WaitingForUser` / `DeliveryStore` 版本门与 S1–S5 最终顺位。

**PR #492 落地与修订 9.8 Router＋多 Work 方案定稿（2026-11-26 · `origin/main@da4f6574a`）**：PR #492 已落地 S1–S3、极简助理 UI 与 600/600 单测全绿（详见 `最终方案.md` §12.6 与 `证据.md` §11.33）；基于真机连发双消息阻塞复现与三层并发能力审计（`证据.md` §11.34），同步在 `最终方案.md` §12.7 落盘「会话级轻量 Router + `root_session_id` 多后台 Work」架构方案与 PR-1（S4 并发底座）/ PR-2（Router 通道与 UI 解绑）精确文件影响范围账本；**PR-1（`feat/flowy-s4-concurrent-pool`）已在隔离 worktree 落地仅 `flowy-agent/src/handle/*`（0 碰 UI / core / i18n）并通过深度 Code Review + 老爷爷（`carl-idea-king`）6 项并发/嵌套目录加固与三 crate 445/445 全绿（`证据.md` §11.35）**。

**导航/全局快搜/进度看板 v2 体验收口（2026-11-26 · PR #495 `origin/main@feb010a56` · 修订 9.9）**：完成顶部栏 Windows 拖拽命中区修复、零依赖 `Ctrl+K` 全局快搜（带密钥前置脱敏）、左上角主助理品牌胶囊合并与 GPU 流动光带、侧栏「进度看板 / 资料库」更名与新建任务常驻、进度看板 v2（3 卡扩容可视化 + 近 7 日动态柱状图 + 5 类交付物占比 + 7 天前折叠 + 同名文件逐级路径消歧）、自动标题失败重试与 `pinned`/`archived` 双文件原子持久化、统一右键/操作菜单（置顶/重命名/归档/删除，兼容 S4 运行态归档保护）；全量通过独立对抗审查（6 P1 + 5 P2 全修复）、8-Crate `628 / 628` 单测、`clippy -D warnings` 0 警告及 `20 / 20` 实机桌面视觉 E2E 验收（详见 `最终方案.md` §12.8 与 `证据.md` §11.36）。

**PR-2A 会话级轻量 Router 与多 Work 并发内核（2026-11-26 · `feat/flowy-router-kernel` / PR #496 open，基于 `origin/main@feb010a56` · 修订 9.10）**：在零改动 UI/App/i18n 边界内完成 `SessionRole::{Standard, Router, Worker}`、`WorkAccessMode::{ReadOnly, ReadWrite}`、`PendingRouterFollowup` 持久化 FIFO、`AcceptanceQueue::accept_strict`（`AcceptedMessage` 装箱保 `240B`）、`PreToolDecision::SuspendWithCard`、4 亚秒控制工具（统一 `spawn_blocking`）、`RouterInbox` 合并回投、只读三工具白名单、可恢复级联删除及 PR #495 全部耦合护栏；经深度 `/code-review` + `carl-idea-king` + `/ponytail` 修复 5 项 `[Required]` 并清理死导出/重复代码，三 crate **475 / 475** 全绿（core 117 / agent 246 / studio 112）、`clippy --all-targets` 0 警告、`flowy-desktop` check 0 错误，待合流后启动 PR-2B UI 接线（详见 `最终方案.md` §12.9、`证据.md` §11.37 与 `交接给下一个agent.md` §19）。

## 三条最容易踩的

1. **核验只为「预先声明且重新核对成功的字段」背书** —— 不是「文件里的每个数都对」。未声明的文本框核验看不见。
2. **L3 语义验收首版只出提示、不阻断** —— 三次校准里前两次被自己的测试设计污染，第三次才干净（误判 0/10、漏判 1/10，样本小）。
3. **交付动作只能有一个出入口** —— 别处不持有发送器；`publish_file` 全类型均经 `DeliveryStore::deliver_with` 过门并向 `delivery/events.jsonl` 留痕（已知无核验器类型按方案 A 记 `EXEMPT_NO_VERIFIER_V0`，未知扩展名与未核验 `.pptx` 一律阻断）。

## 更新方式

方案有新结论 → 改 `最终方案.md` 并升修订号；有新实测 → 追加 `证据.md` 的 11.x；
被推翻的结论**不要删**，加一行说明「已被 11.x 推翻」——保留错误现场比擦干净更有用。

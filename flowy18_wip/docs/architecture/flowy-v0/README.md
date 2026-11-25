# Flowy 首版架构方案（v0）· 索引

[← 架构与决策](../README.md)

> 归属 `flowy18_wip/docs/architecture/flowy-v0/`（原根目录 `docs/architecture/flowy-v0/` 已整体迁回 `flowy18_wip/`）。
> 这里放的是**设计与文字实测证据**，不是代码。原临时 `evidence/` 下的桌面截图（PNG）与本机原始 JSONL 日志已移出 Git 仓库以控制仓库体积并避免包含本机路径，完整实测记录见 `证据.md`；实现进度见 `PROGRESS.md`，待裁决见 `BLOCKED.md`。

## 按你想知道的查

| 文件 | 读它做什么 |
| --- | --- |
| `一页纸.md` | **3 分钟了解全貌。** 它是什么、交什么、不做什么 |
| `实现对照.md` | **每个重要环节：我们选了什么 / 对比方案是什么 / 为什么不选 / 证据在哪。** 想挑刺先读这份 |
| `最终方案.md` | **结论稿（修订 9.6）。** 要动手只读这份：分层、work、能力包、执行、调度、核验、记忆、待人卡、边界、观测、§12.1 落地路线图、§12.2 Harness 四支柱评估、§12.3 路线 1 v2 拆解与结构化输出校准、§12.4 OpenAI Dot 四轮实测反哺、**§12.5 架构可行性实证校准与四项结构性增补** |
| `证据.md` | **全部实测记录（§11.1–§11.32）。** 数字、可重跑目录、真机日志审计、独立内核审查反例闭环、真云端结构化 L3 实测、**§11.32 `contract.rs` 实跑证伪与四处代码级可行性审计**及被推翻的结论 |
| `跨平台沙箱与进程回收预留方案.md` | **【跨平台预留蓝图】** Windows 优先通用层（`RunFence` 两阶段回收 + MIT 净室 `shell_policy` + 诚实 `SandboxAvailability`）与 macOS Seatbelt / Linux `bwrap` / Windows AppContainer 接入规格与验收矩阵 |
| `参照.md` | 五款工作台横评、AutoLoop 借鉴分级、sub agent 与 super agent 的区别、**§10.5 OpenAI Dot 四轮实测对照与老爷爷对抗审查分级** |
| `先记.md` | 六件暂时不做的事，每件写明「什么时候该回来看」 |
| `审查交接.md` | **【历史快照】** 2026-11-23 交给外部审查者的材料，含审查结论与修正清单 |
| `V0-执行计划.md` | **【现役执行计划】** 经 2026-11-25 需求方拍板与真机巡检重订的 Phase 1–Phase 4 收口计划（文末完整归档首轮底稿） |
| `交接给下一个agent.md` | **【现役交接主入口】接手先读 §17（`feat/flowy-v0-rc@f47804a9d`）。** 唯一现役状态、新模块目录布局、`6+4` UI 契约、踩坑清单与实施指引 |

## 现状一句话

**方案修订至 9.6（保留 9.1–9.5 全部正文），实测覆盖 §11.1–§11.32。** 需求方已拍板确认目标锁定 `flowy18_wip/` 并采用显式豁免表方案 A；已删除历史冗余备份分支。

**代码侧（2026-11-25 · `feat/flowy-v0-rc@f47804a9d`）**：工作树 `.agents/worktrees/feat-flowy-local-receipt` 已合流 `feat/flowy-super-ui` 并完成 10 轮纯内核可靠性加固与路线 1 v2 收口（`f8fb6b491`..`f47804a9d`，0 碰 UI）：拆解 `handle/` 与 `verify/` 目录、补齐 `RunFence` 两阶段取消回收、MIT 净室跨平台 Shell 静态安全策略 + 诚实沙箱能力层、单次 HTTP 工具调用 `complete_structured`（真云端 L3 实测提速 3.26×）与 §6.8 `PROVENANCE_INVALID` 溯源拦截（`flowy-agent-core` 90 / `flowy-agent` 168 / 真云端 L3 1 全绿，`clippy -D warnings` 0 警告，`cargo check -p flowy-app` 通过）；仅剩 V6 外发通道按 `BLOCKED.md` §5 停等拍板。

**主干合流与外部对照定稿（2026-11-26 · `origin/main@526b2336f` / PR #490 · 修订 9.6）**：`feat/flowy-v0-rc` 已与 UI v2、ClawHub Skills（#487）及文档迁回整合并合入 `main`；同步在 `参照.md` §10.5、`证据.md` §11.32 与 `最终方案.md` §12.4–§12.5 落盘 OpenAI Dot 四轮实测对照、`contract.rs` 7 组真机实跑证伪与四处代码边界审计、以及校准后的三层对象模型 / 混合按需 `ContractResolver` / 持久化 `WaitingForUser` / `DeliveryStore` 版本门与 S1–S5 最终顺位。

## 三条最容易踩的

1. **核验只为「预先声明且重新核对成功的字段」背书** —— 不是「文件里的每个数都对」。未声明的文本框核验看不见。
2. **L3 语义验收首版只出提示、不阻断** —— 三次校准里前两次被自己的测试设计污染，第三次才干净（误判 0/10、漏判 1/10，样本小）。
3. **交付动作只能有一个出入口** —— 别处不持有发送器；`publish_file` 全类型均经 `DeliveryStore::deliver_with` 过门并向 `delivery/events.jsonl` 留痕（已知无核验器类型按方案 A 记 `EXEMPT_NO_VERIFIER_V0`，未知扩展名与未核验 `.pptx` 一律阻断）。

## 更新方式

方案有新结论 → 改 `最终方案.md` 并升修订号；有新实测 → 追加 `证据.md` 的 11.x；
被推翻的结论**不要删**，加一行说明「已被 11.x 推翻」——保留错误现场比擦干净更有用。

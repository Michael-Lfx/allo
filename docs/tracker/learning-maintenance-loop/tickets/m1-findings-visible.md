---
id: m1
title: findings 可见——读接口 + 结构页签展示（零 LLM）
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

评审把看见的缺陷写进了 `learning_structure_findings`（本课已积 16 条），但
**没有任何读接口、没有任何界面**。用户看不到「评审看见了什么」，也看不到
「哪些还没修」。违反用户铁律：上下文里有 ≠ 用户能看到。

## 已核验事实

- 表 `learning_structure_findings`：`seq / source / node / field / issue /
  advice / course_id`（`070_learning_domain_rebuild.sql`）。现有 source 值：
  `concept-review`、`growth-budget`。
- 写侧：`append_structure_findings`（service，fail-open）；读侧只有两处内部
  用途——修补回合读水位之后的行、`maintenance_needs` 数未读行数。**无对外
  接口**。
- 水量语义：`applied` 与 `noop` 都推进（= 已读，不是已修复）。
- UI 现状：结构图页签已有「详情」折叠区（挂载观察逐行展示），是并列放置
  留痕的现成位置（`StructureGraphTab.tsx` 挂载明细折叠）。

## R

- **R-a 读接口**：`GET /api/learning/courses/{id}/structure/findings?limit=N`
  （默认 50，上限 200）——按 `seq DESC` 返回最近的留痕，字段照表返回，另加
  `read_by_repair: bool`（`seq <= 水位` = 修补已读过）。纯读、零 LLM、零写。
  课程不存在/软删 → 404（与 structure_view 同口径）。
- **R-b UI**：结构图页签的「详情」折叠区新增一段「评审留痕（最近 N 条）」：
  每条显示 `field`｜`issue`（原文）｜`advice`（原文，可为空）｜来源标记
  （concept-review / growth-budget）｜「修补已读」小标。默认折叠，不打扰。
  空态 = 整段不渲染。
- **R-c 双语**：新增文案进 zh-CN / en-US 两份词典并对称；`bun run gen:i18n`
  更新 `i18n-keys.d.ts`；`bun run check:i18n` 通过。
- **R-d 不做**：不提供编辑/删除/标记处置（留痕是只增审计面）；不推进水位；
  不触发任何回合。

## Acceptance

- 接口单测：按 seq 倒序、limit 生效、`read_by_repair` 与水位一致（构造
  `learning_maintenance_state` 行验证两侧）、课程不存在 404。
- UI 测试：有条目时渲染、空态不渲染、`read_by_repair` 标记正确。
- `cargo test -p nomifun-learning` 全量绿 + `bun run typecheck` +
  `bun run check:i18n`。
- **真机**：起服务后该课结构页签「详情」里能看到那 16 条（含「Option 类型」
  「容器/接口」「前置底座缺口」三条原文）。

## 回退条件

纯读面，无回退风险。若 UI 位置干扰结构图操作 → 挪进二级折叠即可。

## Resolution

**真机验证通过**（2026-11-25）。结构图页签里能读到本课全部 16 条留痕，
含票面点名的三条原文。

### 落地

- **提交** 2939a58db（读接口 + 结构图页签段）→ 0ed0bd485（字段标签补齐）。
- **R-a 读接口**：`GET /api/learning/courses/{id}/graph/structure/findings?limit=N`，
  service 函数 `structure_findings`（`service/learning_graph.rs`，紧跟
  `structure_view`，与私有的 `findings_watermark` 同文件）。响应
  `{ findings: [{ seq, ts, source, node, field, issue, advice, read_by_repair }],
  total, watermark, unread }`。纯读：零 LLM 零写，不推水位、不触发回合。
  `read_by_repair = seq <= 库里的水位`，**不套**
  `NOMI_LEARNING_FINDINGS_WATERMARK`（读面与进程级 env 解耦，也避开并行测试
  互扰）；limit 复用系统动作簿口径（缺省 50、钳 1..=200、坏值截断不拒绝）。
- **R-b UI**：`StructureFindingsSection.tsx` 三个组件——`StructureFindingsList`
  （行）、`StructureFindingsSection`（折叠壳，纯 props）、`StructureFindingsLoader`
  （自取数 + 听 WS 帧）；挂在 `LearningGraphWorkspace` 的结构图 `TabPane` 内、
  `<StructureGraphTab />` 之后。空态整段不渲染；读数失败时标题行留在场、展开
  见词典句。
- **R-c 双语**：22 个键两份词典同位；`structureFindingsLocales.test.ts` 另断言
  两份词典键序一致（`check:i18n` 只按 en-US 生成类型、不校验 zh/en 对称）。
- **R-d**：无编辑 / 删除 / 处置动作，不推水位。

### 偏离票面（读码取证后由执行方裁决）

| # | 票面 | 实际 | 理由 |
| --- | --- | --- | --- |
| 1 | 路径 `/courses/{id}/structure/findings` | `…/graph/structure/findings` | 现有结构路由全在 `graph/structure{,/regenerate,/mount,/repair}` 下，字面路径会脱离路由族 |
| 2 | 放进挂载卡的「详情」折叠区 | 结构图页签内独立自取数的折叠段 | 「详情」只在有挂载行时存在（`StructureGraphTab.tsx:151` 整张卡由 `mounts &&` 把门，`:183` 再加一道）；嵌进去，没跑过挂载的课程永远看不到留痕 |
| 3 | 未提交换 | 监听 `growth_*` / `structure_mount_*` / `structure_repair_*` 帧重取（与系统动作簿同组） | 结构视图本不随这三组帧重取；不监听则新留痕要刷新页面才出现 |
| 4 | 行 = field｜issue｜advice｜来源｜修补已读 | 另出 `ts` 与 `node`；字段 / 来源机器名走词典，未知值原样透出 | 仓库惯例「机器名的人面文案由前端 i18n 承担」；裸 `mints` / `teaches` 用户读不懂 |
| 5 | 响应只要求 `read_by_repair` | 另带 `total` / `watermark` / `unread` | 截断时如实说「最近 N 条 · 共 M 条」；标题行能报「修补未读 K」 |
| 6 | 隐含 field 是枚举（白名单 6 值） | 字段标签另覆盖 `intent` 与降级 / 归一通道 5 类 | `findings[].field` 是自由文本（无枚举校验），归档留痕里真出现过 `intent` |

### 检查

- 过滤测试：`cargo test -p nomifun-learning structure_findings` **5/5**
  （4 条 service + 1 条 oneshot 路由）。
- 全量 `cargo test -p nomifun-learning` **1081/0**（基线 1076 + 5）。
- `bun run typecheck` 通过；`bun run check:i18n` 通过（12,588 键 / 56 模块）。
- UI 测试全 0 失败：`StructureFindingsSection.render` 12、
  `structureFindingsLocales` 4、`LearningGraphWorkspace.findings` 5、
  `header-collapse` 19、`quiz-entry` 6、`soft-gate` 9、`StructureGraphTab.render` 7；
  另 `bun test --isolate src/renderer/pages/learning` 524/0、`check:dead-css` 干净。

### 真机读数（2026-11-25，冻结二进制 = 0ed0bd485 树，dev 库）

| 步骤 | 结果 |
| --- | --- |
| `GET …/graph/structure/findings`（缺省 limit） | 200；`total=16 watermark=0 unread=16`，返回 16 条，seq `16→1` 倒序，`read_by_repair` 为真的 0 行 |
| 票面三条原文 | #15「提名别名「Option 类型」…」、#14「…「接口」「容器」…」、#13「…前置底座存在缺口。」各命中 1 条 |
| `?limit=3` / `?limit=5000` / `?limit=0` | 3 条（16,15,14）/ 封顶仍 16 / 钳到 1 |
| `?limit=abc` | 400（axum 提取器纯文本拒绝，进不了 handler） |
| 课程不存在 | 404 `{"success":false,"error":"Not found: course …","code":"NOT_FOUND"}` |
| 页面 `/#/learn/{id}` 结构图页签 | 「评审留痕 · 最近 16 条 · 修补未读 16」默认折叠；展开 = 水位语义提示 + 16 行；字段标签 13 个（3 行字段为空不出标签）、来源标签 16 个 |
| 双主题 | 暗色截图正常；亮色下计数标签 `rgb(22,93,255)` / `rgb(232,243,255)`，与同页签现成的「入门检查」标签逐值相同 |
| 副作用 | 学习数据零变化（留痕 16、水位表 0 行、挂载 hash 与 updated_at 不变、动作簿无新增） |

### 边界

- `issue` / `advice` 是模型写的中文自由文本，en-US 界面下原文仍是中文——不可
  本地化，如实保留。
- 非 numeric `limit` 的 400 是纯文本而非 JSON 信封（axum `Query` 提取器行为，
  与系统动作簿接口相同）。
- WS 帧重取只做了源面核对（测试宿主的 WebSocket 是哑件）；真机没有靠触发生长 /
  修补来验证刷新。
- 带页面的真机要用 `cargo build -p nomifun-web --features static-webui`
  （须先 `bun run build:ui`）：`--api-only` 用法的二进制拒绝提供静态页
  （`no exact frontend build identity`）。

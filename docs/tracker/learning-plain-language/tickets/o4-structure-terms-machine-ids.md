---
id: o4
title: "结构区术语与机器直出——主干/门口/投影/端点口语化 + id/枚举不再上屏"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# o4: 结构图区术语口语化 + 机器 id/枚举直出收口

**What to build:** 两部分：

**A. 词典术语**（改写方向来自探查，允许微调）：

| key | 现状（病） | 改写方向 |
| --- | --- | --- |
| `learningGraphStructureSummary` | 「{{spines}} 主干 · {{blocks}} 块 · {{doors}} 门口」 | 「{{spines}} 条主线 · {{blocks}} 个知识组 · {{doors}} 个入门检查」 |
| `learningGraphStructRelation_assumes` | 「前置（门）」 | 「前置知识」 |
| `learningGraphStructDoorRefsLabel` | 「门口覆盖」 | 「对应的入门检查」 |
| `learningGraphStructRouteSpine` 及 Route/All/HideSpines/MatrixSpine 族 | 「主干：{{name}}（{{count}} 块 + 自动补前导）」 | 「主线：{{name}}（{{count}} 个知识组，自动补齐前置）」 |
| `learningGraphCreateHint` / `learningGraphStructureHint` / `learningGraphStructDetailNote` | 「主干与知识块」「参考地图」 | 地图隐喻保留，主干/知识块 → 「主线与章节」 |
| `learningGraphStructFooter` | 「投影非权威；层号 = 依赖深度，不是排期」 | 「此图仅供了解结构；层级表示先后依赖，不代表进度」 |
| `learningGraphStructUnregistered` | 「{{count}} 个未收录端点」 | 「{{count}} 个未归入结构的概念」 |
| `learningGraphStructLayer` | 「依赖层 L{{layer}}」 | 「第 {{layer}} 层（越靠前越基础）」 |
| `learningGraphConceptsEmpty` | 「教与假定的概念」 | 「课程生成内容后，这里会列出它教到的、以及默认你已会的概念」 |
| `learningGraphAuditFragmentsTooltip` / `SelfRefTooltip` / `MutualTooltip` | 「宁滥勿缺/切词噪声/ISO 704 禁止自指/循环定义的最短形态」 | 「定义里出现了这个词自己」「这个词和「{{name}}」互相解释，等于没解释」「定义中未被词表解释的词」 |
| `conceptRelationsHint` | 「（实时计算，不单独存储）」 | 「根据定义中的相互引用自动整理」 |
| `samplingTooltip` | 「连贯预算/章节主干」 | 「优先选与目标相关的文件并保持章节完整」 |
| `reviewManageHint` | 「剔除」 | 「移除」 |
| `learningGraphStructCounts`（Mindmap） | 同 Summary 族 | 随 Summary 口径 |

**B. 机器直出收口**（组件层）：

| 点 | 现状 | 改写方向 |
| --- | --- | --- |
| `GateCoverage.tsx:129` | `door.id` 直出 Tag | criterion 摘要句，id 只留 aria/Tooltip |
| `GateCoverage.tsx:190-196` | `basis.doors.join('、')` id 串 | 只报数量 |
| `CourseSourceCoverageCard.tsx:191,200` | `` `[${entry.role}] ${entry.path}` `` | role 映射词典（core/practice/…→中文角色名） |
| `utils.ts:33` | 库名解析失败裸 UUID | 「来源库已删除」 |
| `GrowthTaskPill.tsx:156-158` | topic 前缀失配回退整句后端串 | 回退「课程名 + 通用后台任务」（与 v3 `taskKindUnknown` 同水位） |
| `StructureGraphTab.tsx:152-159` | 「结构外：{{detail}}」后端观察串内插 | 确认 detail 形状，节点 id 换标题；无法映射则收进折叠 |

## 法证证据（2026-11-11 词典面 J2 + 组件面三类 18-23）

- 「门口/主干」是学习图核心自造词，用户无图例不可解；door.id 是英文机器
  key 常驻 Tag。en-US 同病。

## 实施要点

- [ ] A 部分 key 双语改写 + 断言；`rg "主干|门口|投影|端点" zh-CN/learning.json`
      本域清零（o2 已清声明区，此处收结构区剩余；保留隐喻清单除外）。
- [ ] B 部分六个直出点收口（role/id 映射词典新增）。
- [ ] 渲染测试更新（Summary 行/RouteMap/BlockDetail/GateCoverage 相关既有
      断言同步）。
- [ ] `gen:i18n` + `check:i18n` + typecheck + learning 测试。

## 验收

- zh/en 两语本域目标词归零；机器 id/枚举/裸 UUID 上屏点 grep 归零。
- 真机目验：结构图 Tab + 门口卡 + RouteMap 读一遍（两语）。

## 非目标

- 声明区（o2 已覆盖）、布局重构与双挂载去重（o6）、图例/教学引导新功能
  （若口语化后仍需解释，先记录后置，不本波加功能）。

## Resolution（2026-11-11 施工收口）

**代码面验收全过，已闭票；真机目验（结构图 Tab + 门口卡 + RouteMap 两语）与欠账合并执行，FAIL 按票重开。**

- 施工 = `40ef60a43` + `ea61ba86f`（20 文件 +943/−107）：A 面 14 组术语双语改写（主线/知识组/入门检查/前置知识/仅供了解结构/第 N 层…）+ B 面 6 直出点收口（门口 Tag→criterionSummary 判据摘要码点安全截断、已声明行只报数量、role 八档词典映射 SOURCE_ROLE_KEYS 收 model.ts、knowledgeBaseNames 裸 UUID→「来源库已删除」、GrowthTaskPill topic 失配回退→「后台生长任务」词典、后端观察句确认后收 MountObservationSummary 折叠）+ **票外发现 1 处**（BasisDeclareDone toast join 门口 id+块 slug 双串→只报数量）。
- 与 o2 的合并裁定（已落在集成树）：结构区措辞归 o4、声明区措辞归 o2；占位契约归 o4（`{{doorsCount}}/{{blocksCount}}`，o2 原 `{{doors}}` id 串值与其组件不兼容）；o2 两处测试期望同步到合并值。
- 会话裁决修正落地：**「知识块」移出保留隐喻清单、统一「知识组」**（评审发现 o4 票面方向与 map 裁决 3 冲突致词典三词并存）——残留 5 处由修复 F1 清零（`302ecc3c8`），`rg 知识块` 全词典归零。
- 病词 sweep：主干/门口/投影/端点/宁滥勿缺/ISO 704/剔除/连贯预算/幂等/铸出/登记表 zh learning.json **零命中**。297 绿（施工时）→ 316（全波终态）绿。

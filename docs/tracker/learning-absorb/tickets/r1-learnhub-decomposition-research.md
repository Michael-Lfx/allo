---
id: r1
title: "learnhub 知识结构生成链路深挖"
type: research
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

learnhub 的 decomposition 链路具体是什么：用户输入、LLM 轮次、契约 schema、提示词资产形态、
JSON wire-only 落盘、修复重试；配套 ADR 裁决链（0010→0012→0013→0015）各锁定了什么、否决了
哪些替代方案？哪些可直接移植到 axum + SQLite 环境？

## Resolution

**速览**：用户只给四项目标声明（goal 原句 / goal_type 二选一 / proficiency_statement /
learner_basis 可缺省 = 纯零基础），**没有任何终点或起点选择输入**。「多终点选择」不是被做成
选项，而是根本不存在终点概念——结构只是参考地图，学习顺序权威归账本 + 教练。

- **流程**（`src-tauri/src/decomposition/facade.rs`）：每轮恰好 2 次串行 LLM 调用——
  ① GoalGenerator 生成 structure 模型面 JSON，② TerrainGenerator 生成前置分解（门口
  G1..Gn 判据）；随后门口对账（structure 的 prerequisite_doors 必须能解析到地形门口）；
  任一产物失败则两份**整批重试 ≤3 次且带逐条款违规反馈**；全部成功才原子落盘。
- **契约 v2.4**（`docs/contracts/decomposition-artifacts.md`）：structure = 8 标量键 +
  domain（core_definition + includes/excludes ≥3 + 门口引用 + audience + scenarios）+
  spines 6–12（可 thin_spine 豁免）+ blocks 40–80（五键 name/slug/spine/stage/note，
  一物一名）+ edges 三型（assumes/composes/contrasts，tier know/use/teach，端点解析域 =
  块名集）+ gaps ≥3 + quality ≥5；全文禁排期语词。常量单源 `structure.rs`。
- **JSON 契约**（ADR-0014，`json_contract.rs`）：provider json_object 保证合法 JSON + 本地
  JSON Schema 单源（schema↔validator 互钉测试）+ 语义 validator 永不退役 + wire-only 落盘
  （模型 JSON 代码序列化为 YAML，「解析风险归零」）。五级修复链仅剩 content 长文在用
  （KaTeX 长文不走 JSON）。
- **提示词资产**：仓库 md 文件 `include_str!` 编译期嵌入 + 头注剥离 + 具名占位符单遍替换 +
  围栏词表机械拦截开发面词汇 + 金样快照；温度/thinking 住注册表不进文本。
- **ADR 裁决链**：0010 概念词典与课程结构分离 → 0012 普查废止（概念唯一出生路径 = 生长批
  铸名门）→ 0013 起点判定归概念侧（结构不供内容/不做判定/不与概念连接，assume 只引用已铸
  概念）→ 0015 生长求解器（结构 = 地图不是队列；FIFO 链队列、交付账、selectable 过滤、
  NoFrontier 全部退役；横跳是教学正解，病只在不跳步；防跳步铁底 = 概念侧两层门）。
- **可视化**：dagre 算坐标 + 自绘 SVG 连线 + HTML button 节点（xyflow 已弃用，echarts 仅
  力导图按需加载）；五图 = 思维导图/矩阵/关系图谱/学习路线/线路图 + 详情面板；「投影非权威」
  标注收敛到详情层；差异化路线 = 读侧投影（子图 + 传递闭包自动补前导），不落盘。
- **投影铁律**（ADR-0002）：structure 文档快照是唯一落盘真相；导出走纯函数
  `build_structured_view`（零 IO 零 LLM）运行时重算；不建关系表存块间边。
- **SQLite 移植要点**：结构源整篇单字段快照存储；契约文档先行、条款化 validator（gd.*/ng.*
  编号即稳定机器标识）、整批重试带反馈、LlmExecutor 接缝、观测存档可直接平移。

完整报告（含 schema 片段、提示词原文要点、模块移植相关性清单）：
[assets/r1-learnhub-findings.md](../assets/r1-learnhub-findings.md)

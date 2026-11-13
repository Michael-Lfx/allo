---
id: r2
title: "allo 学习模块现状盘点"
type: research
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

allo 学习模块现在长什么样：建课流程分几步、哪一步是多终点选择、终点锚体系分布在哪里；
学习图数据模型与生长治理现状；可视化现状与缺口；工作区未提交改动涉及哪些面？

## Resolution

**速览**：学习图建课是两步向导，**第 2 步就是多终点选择**（AI 提议终点锚 → 用户增删改确认，
数量不封顶）；后端以 `learning_course_endpoints` 落库并驱动罗盘分节、终点完成裁决、停摆臂。
可视化完全缺失（无图渲染、无布局算法、无边数据接口，罗盘只是 `<pre>` 纯文本）。

- **向导**（`ui/src/renderer/pages/learning/components/CreateCourseDialog.tsx`）：
  `graphStep: 'goal' | 'endpoints'`；第 2 步每条终点含标题（≤40 字）+ 程度声明两个输入框。
  `useCourseCreation.ts` 里 `proposeGraphEndpoints` 失败降级为空列表不阻塞建课。
- **后端**（`crates/backend/nomifun-learning/src/service/learning_graph.rs`）：
  `POST /graph/propose-endpoints`（analyze_scope + propose_endpoints 各 1 次调用）；
  `POST /courses/generate` 按 `course_kind` 分流——确认过的终点优先、为空才现场提议；单事务
  落库课程 + 隐含模块 + 每终点一行 + 零正文标记课时；`tokio::spawn` 后台罗盘初画 ∥ 首生长。
- **首生长管线**：入口域盘点（首批专享）→ 教练起草（32768 tokens）→ 确定性结构门 →
  修复循环 ≤4 稿 → AI 概念评审 → 事务落库（铸名 → 节点 → 概念网 → 终点裁决 → 批次档案）。
- **数据模型**（067/068 迁移 + 未提交 069 软删、070 难度列）：`learning_concept_registry`
  （canonical + aliases + definition，跨课程唯一）、`learning_lesson_concepts`
  （teaches/assumes × know/apply/teach）、`learning_course_endpoints`（lesson_id UNIQUE）、
  `learning_growth_batches`（状态机）、`compass_md`。
- **结构门**：`validate_batch`（就绪即发布——assumes 必须被全局已教账覆盖；铸名不撞）+
  `validate_batch_depth` 深度水位门；生长触发 `maybe_kick_growth`（就绪 <3 才动）+ 停摆两臂
  （空图 / 全部终点完成）+ R 软闸（预测回忆率 <0.85 剔出推荐）+ 行为摘要（7 日/10 节点窗口）。
- **可视化现状**：`LearningGraphWorkspace.tsx`（815 行）是 4 类 Arco 组件信息架构——终点
  chips + 就绪集导航 + 批次时间线 + 概念表；`compass_md` 是收起展开的 `<pre>`；后端注释明说
  「DAG 画布已不存在」；**没有任何接口输出边给前端**。概念网关系数据足以支撑节点-边渲染，
  图投影 API 属净新增。
- **工作区**：15 文件未提交（+912/−205），集中在 `learning_graph/mod.rs` 与
  `service/learning_graph.rs`——吸收工作前需先落库。

完整报告（含 i18n 键反推的交互步骤、提示词原文、token 预算表）：
[assets/r2-allo-findings.md](../assets/r2-allo-findings.md)

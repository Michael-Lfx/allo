# Map: 生长状态作用域与术语精准波（b1–b2）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-17）。来源 = 用户真机反馈：仅在另一门课建课运行
      时，本课（01a1e364，无任何任务）知识结构卡即标「生长中…」+ 补充节
      点钮禁用/加载——「似乎任意模型占用都会被标示」；且「知识结构生长」
      与「补充节点」并非一回事，提示应精准。 -->

## 背景

f1–f3 关波后用户发现处理态呈现的两个缺陷：跨课程串扰（后端全局旗）与
动作术语混用（徽章「生长中」vs 按钮「补充节点」）。另有 a3（头部队列）
配套：后台运行不应禁用折叠钮。

## 已核验事实（2026-11-17 对码）

- **串扰根因**：`learning_graph.rs:4972` `generation_running()` =
  `generation_tasks.snapshot().iter().any(RUNNING)`——**不筛课程、不筛
  kind**（七类任务 KIND_BASIS_DECLARE/GROWTH/LESSON/OUTLINE/STRUCTURE/
  MOUNT/REPAIR 皆命中）；`:4058` 把它注入**每一门课**的 graph 视图
  `growth_running`。前端消费：徽章「生长中…」（LearningGraphWorkspace
  :714）+ 次要行（:690）+ 补充节点钮 loading/disabled（:871/:876）。
  复现：A 课建课跑 KIND_STRUCTURE → B 课 growth_running=true。
- **前端其它源无串扰**：WS 订阅有 course 过滤（:322）；注册表快照
  `hasActiveStructureRow` 正确按 course_id+kind 过滤（structureReadout.ts
  :79-89）；structureRunning 徽章面干净——串扰专属 growth_running。
- **`generation_running()` 全仓唯一调用点 = :4058**——修法收口面小。
- **术语现状**：growth 徽章「生长中…」（learningGraphGrowing，zh:458）vs
  同一动作的按钮「补充节点（就绪 x/y）」（learningGraphGrow，zh:457）；
  auto 派发窗徽章「自动准备中…」语义不同（另词）；结构生成徽章走
  structureBadge 族（「结构生成中 · 第 x/y 次」）。

## 票表

| id | 票 | 严重度 | 类别 |
| --- | --- | --- | --- |
| b1 | growth_running 按课程+kind 收窄（跨课程串扰） | 高（处理态系统性失真） | 后端口径 |
| b2 | 徽章动作名与按钮对齐（生长中→补充节点中） | 中（术语误导） | i18n/术语 |

## 裁决待定

- b1 修法口径（票内荐：按 course_id + KIND_GROWTH 过滤，快照行已带
  course_id，零 schema 变更）。
- b2 键名是否随值更名（learningGraphGrowing → learningGraphAddingNodes，
  荐改名求名实一致；gen:i18n 承担键面更新）。

---
id: c8
title: "概念详情页 + 全局总览页（路由 / 两页 / 概念表行点击与页头入口 / 其他课程可点）"
type: task
status: open
assignee:
blocked_by: [c7]
part_of: ../map.md
---

# c8: 概念详情页 + 全局总览页（前端）

**What to build:**

- 路由：`/learn/concepts`（总览）与 `/learn/concepts/:conceptId`（详情），
  lazy 加载；静态段优先于 `/learn/:id`。
- **总览页**：表格列 = 概念（canonical + 定义两行 ellipsis）/ 别名数 / 在教
  课程数 / 学习节点数 / 已声明课程数 / 审计标记（自指、互指 ×N）；行点击进
  详情；页头说明「概念跨课程共享」；返回学习列表。
- **详情页**：标题 canonical + 来源 tag（已会声明来源等）+ deprecated 提示；
  定义全文；别名表（名称/定义/状态）；关系区（互指 / 提及 / 被提及，chip
  可点跳对方详情；自指警示）；使用区（按课程分组：课程名可点跳 `/learn/{id}`，
  组内节点行 = 标题 + 教/假定·层级 + 引用名 + 状态，节点纯文字）；声明区（哪
  些课程声明过它）；返回上一页。
- **课程内入口**：ConceptTable 行点击 → 详情；表头上方「全部概念」按钮 →
  总览；其他课程也在教列的课程名可点跳课程。

## 法证证据（2026-11-09 实码核实）

- 无任何概念页面/路由：全仓无 `ConceptDetail` / `conceptDetail`；
  `Router.tsx:414-415` 仅 `/learn` 与 `/learn/:id`。
- 先例：`/knowledge` + `/knowledge/:id`（Router.tsx:412-413）独立列表/详情
  组件；learning 页面 `index.tsx:233` 用 `navigate(\`/learn/${courseId}\`)`。
- 节点无 URL：选中态本地 `selectedLessonId`（LearningGraphWorkspace.tsx:107）
  ——详情页节点引用 MVP 纯文字（后置清单）。
- 历史设计参照（未建）：`docs/tracker/learning-absorb-3/tickets/r1-structure-maintenance.md:37`
  的 ConceptView（定义 + 别名 + 按别名分组跨课引用）。

## 实施要点

- [ ] Router.tsx 两条 lazy 路由（照现有 learning/knowledge 懒加载写法）。
- [ ] 总览页 / 详情页组件（`pages/learning/` 下，按现有目录习惯定名）；加载 /
      空 / 错误三态；语义 token 配色，两主题可用。
- [ ] ConceptTable：`onRow` 点击跳详情；页头「全部概念」按钮；other_courses
      渲染为可点链接。
- [ ] 全部新文案 zh-CN/en-US 对称（`gen:i18n` → `check:i18n`）。

## 验收

- `bun run typecheck` + `check:i18n` + `check:theme` 绿。
- 真机手验：概念表行点击 → 详情页定义/别名/关系/使用/声明齐全；关系 chip 互跳；
  课程名跳课程；页头入口 → 总览 → 行点击回详情；未知 id 显示错误态不白屏。

## 非目标

- 节点直达（需节点 URL 化）；概念编辑；总览分页（后置清单）。

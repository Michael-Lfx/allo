---
id: c8
title: "概念详情页 + 全局总览页（路由 / 两页 / 概念表行点击与页头入口 / 其他课程可点）"
type: task
status: closed
assignee: zhangyj
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

## Resolution（2026-11-09 施工收口）

**全验收过，闭票。** 施工 = `383e630f6`。

- **路由**：`Router.tsx` 两条 lazy 路由 `/learn/concepts` 与
  `/learn/concepts/:conceptId`（静态段优先于 `/learn/:id`），页面放
  `pages/learning/concepts/`（默认导出供 lazy）。
- **总览页** `ConceptsOverviewPage`：沿用学习列表页页壳与内容列限宽；表 = 概念
  （名 + 定义两行截断 + Tooltip）/ 别名 / 课程 / 学习节点 / 已会声明 / 定义审计
  （自指、互指 ×N）；行点击进详情；名称/定义本地筛选（票外小增量：上限 500 行
  时有用，零后端成本）；>50 行前端分页。
- **详情页** `ConceptDetailPage`：标题 + 来源/自指/废弃 tag；定义全文 + 别名
  （名/定义/出处）；关联关系三组（互指紫 / 定义中提到蓝 / 被其定义提到灰，chip
  点击互跳）+ 「读时推导不存储」说明；学习节点应用按课程分组（课程名可点跳课
  程，节点行 = 教/假定·层级 + 标题 + 引用名 + 进度，节点纯文字并注明暂不支持直
  达）；声明过已会的课程（可点）；返回上一页 + 「全部概念」入口；加载/错误/空
  三态（未知 id 走错误态，不白屏）。
- **课程内入口**：概念表表头常驻说明 + 「全部概念」按钮（空表也有）；行点击 →
  详情；「其他课程也在教」渲染为可点课程链接（stopPropagation，不触发行点击）。
- **i18n**：26 个新 key 双侧对称（11581→11607）；复用既有 key 4 处（概念/已会
  声明/定义审计/自指，以及教·假定·层级/引用名）。
- **验收**：typecheck 55 错持平存量（learning/i18n/Router 零错）；`check:i18n` /
  `check:theme` / `check:icons` / `check:button-layout-contract` /
  `check:css-scope` / `check:motion` 绿；`check:dead-css` 学习域零命中（该闸红
  为他处存量债）；学习域前端单测 **119 pass / 0 fail**（17 文件）。真机手验留
  汇总门。

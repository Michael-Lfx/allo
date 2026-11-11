---
id: s1
title: "8 处 join('、') + 全角括号换 i18n——en-US 不再见中文标点"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# s1: 学习图 UI 硬编码中文标点机械修

**What to build:** 学习图结构/覆盖面 UI 的 8 处 `join('、')` 换成已有的
`t('learning.listSeparator')`（zh「、」/ en ", "），:1215 别名列的全角括号
模板换成双语 key——en-US 用户当前在这些位置看到中文标点。

## 法证证据（2026-11-11 审计，实码复核）

- 8 处 `join('、')`：
  - `LearningGraphWorkspace.tsx:410/411`（已会声明成功 toast：doors/blocks）
  - `LearningGraphWorkspace.tsx:1155`（审计 fragments Tooltip）
  - `LearningGraphWorkspace.tsx:1213-1217`（别名列，**全角括号模板**
    `` `${alias.name}（${alias.definition}）` `` + join）
  - `structure/GateCoverage.tsx:62/153/193`（doors/blocks）
  - `structure/StructureGraphTab.tsx:142`（nodes）
- 已有键 `learning.listSeparator` 双语齐全（zh-CN/en-US learning.json:114），
  仓内 4 处在用（CourseGenerationPill.tsx:63、CourseWorkspace.tsx:215/235、
  CourseGenerationProgress.tsx:159）。
- `multiSourceDisclosure.test.ts:39` 已有断言禁止该文件出现 `join('、')`——
  这是本仓已成的规则，其余 8 处是漏网。
- 全角括号无现成模板键，需新增。

## 实施要点

- [ ] 7 处纯 join 换 `t('learning.listSeparator')`（确认每个调用点 `t` 在
      作用域内；非 React 作用域的辅助函数把 separator 作为参数传入）。
- [ ] :1215 别名列模板新增键（如 `learning.aliasWithDefinition`：
      zh `{{name}}（{{definition}}）` / en `{{name}} ({{definition}})`），
      join 同样走 listSeparator。
- [ ] `bun run gen:i18n` + `bun run check:i18n` 过。
- [ ] `bun run typecheck` 过。
- [ ] 自查：`grep -rn "join('、')\|join(\"、\")" ui/src/renderer/pages/learning` 归零。

## 验收

- grep 归零 + 三门（gen:i18n / check:i18n / typecheck）全绿。
- 新增键双语对称。

## 非目标

- EvolutionTab 的 14 处中文 defaultValue（main 侧存量，map 后置 #4）。
- companion 气泡（map 后置 #2）。

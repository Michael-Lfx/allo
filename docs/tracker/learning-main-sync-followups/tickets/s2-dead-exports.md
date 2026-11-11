---
id: s2
title: "5 个零引用导出清理"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# s2: 学习模块死导出清理

**What to build:** 删除 5 个全仓零引用的导出（全仓唯一出现即定义处本身，
2026-11-11 计数核实），连带清理文件内遗留的孤儿 import。

## 法证证据（2026-11-11 审计，实码复核）

- `components/structure/BlockDetail.tsx:142` — `StructureEmpty`，导出组件
  从未被使用。
- `model.ts:31` — `sectionStatusLabel`，与 `utils.ts` 的 `statusLabel`
  疑似旧实现重复。
- `phaseMeta.ts:38` — `COURSE_PHASES`；`:58` — `LESSON_PHASES`；
  `:112` — `isKnownPhase`。同文件的 `PET_PHASE_ALIAS/PHASE` 被 companion
  合法使用（`companion/messages/project.ts:13`），这三者无引用。

## 实施要点

- [ ] 五处导出整体删除（含函数体/常量体）。
- [ ] 删除后遗留的孤儿 import 一并清掉（typecheck/死代码告警兜底）。
- [ ] `bun run typecheck` 过；学习相关前端测试无引用断裂。

## 验收

- `grep -rn "StructureEmpty\|sectionStatusLabel\|COURSE_PHASES\|LESSON_PHASES\|isKnownPhase" ui/src`
  归零。
- typecheck 绿。

## 非目标

- 仅文件内自用的多余 export（palette/layout/growthTaskPill/mindmapLayout
  6 处，非死代码，不动）。

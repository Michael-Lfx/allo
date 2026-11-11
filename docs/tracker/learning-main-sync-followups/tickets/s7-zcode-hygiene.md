---
id: s7
title: ".zcode 会话文件出库 + gitignore 补规则"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# s7: 仓库卫生——AI 工具会话残留出库

**What to build:** 把分支上已提交的 `.zcode/plans/` 会话计划文件移出 git
跟踪（工作树副本保留），`.gitignore` 补 `.zcode/` 规则，防止后续
`git add -A` 把工具会话文件卷进提交。

## 法证证据（2026-11-11 审计，实码复核）

- 已跟踪文件：`.zcode/plans/plan-sess_73a4b952-8a68-4725-8f47-6be52755ed37.md`
  （10KB，「学习图重构：跨课程概念图（生长式）实施计划」，
  `git log --diff-filter=A` 溯源 = 70410f5e9 引入）。
- origin/main 无任何 `.zcode` 路径——不清理就会随学习分支首次把该目录
  带进 main。
- `.gitignore:125` 只有 `.zcodeignore`，无 `.zcode/` 规则；工作树另有
  未跟踪的 `plan-sess_b71fc9ef….md`，处于被 `git add -A` 卷入的风险面。

## 实施要点

- [ ] `git rm --cached` 该文件（工作树副本不删——内容是历史计划文档，
      留在本地）。
- [ ] `.gitignore` 补 `.zcode/`（紧邻既有 `.zcodeignore` 行）。
- [ ] `git status` 确认 `.zcode/` 下所有文件转为被忽略。

## 验收

- `git ls-files .zcode` 为空；`git check-ignore .zcode/plans/...` 命中。
- 提交署名人类（本波惯例）。

## 非目标

- 清理工作树里的本地 plan 文件内容（工具自己管理）。
- docs/superpowers、docs/research 等有意入库的产物（不动）。

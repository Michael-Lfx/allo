---
id: g2
title: "已会声明前端补传 modelChoice——后端已支持 provider_id+model"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# g2: 已会声明补传 modelChoice

**What to build:** `api.ts:166` `declareBasis(courseId, doors)` 补传
modelChoice；调用点 `LearningGraphWorkspace.tsx:273/277` 取
`useLearningAutogenModel` 接线。后端 `DeclareBasisRequest.provider_id/model`
（models.rs:2505）现成，零后端改动。

**验收：**
- 声明请求体带 provider_id+model，SSE 记录 model=用户所选
- `bun run typecheck` 绿

**病历依据：** 2026-11-09 真机 declare 单调用 113.3s 用的平台默认档。

## Resolution

已关（2026-11-10）：commit `23abddc68`，并回 feat/learn0928。declareBasis 请求体带 provider_id+model（null 双不带），请求体两条用例红绿样；stash 对照证明零新增 typecheck。

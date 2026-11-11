---
id: o3
title: "建课与进度流程口语化——分诊/审计/资源槽/幂等/快照/挂载族清零"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# o3: 建课与进度流程口语化（词典 J1 分诊族 + 快照族 + J4 句式）

**What to build:** 建课流程（对话框 + 进度视图 + 生成完成视图）与生长 Pill
的用户文案口语化。改写建议来自词典面探查，施工时允许微调：

| key | 现状（病） | 改写方向 |
| --- | --- | --- |
| `genStepTriage` / `genTriageDegraded` / `growthPillStepTriage` | 「来源分诊」「规则分诊，未做 LLM 判定」「清点与分诊」 | 「整理资料来源」「按固定规则整理，未经 AI 判断」「正在清点来源文件…」 |
| `genStepAudit` / `genAuditLabel` / `lessonGenAudit` / `learningGraphConceptAudit` | 「审计」作步骤名 | 「质量检查」（「定义审计」→「定义检查」） |
| `genQueuedNotice` | 「排队等待空闲资源槽」 | 「正在排队等待空闲算力」 |
| `genRetryResumeHint` | 「生长是幂等的小批次操作，失败不残留半成品」 | 「重新生成会接着上次进度继续，不会留下做一半的内容」 |
| `lessonGenSourceDegraded` / `sourceCoverageUnavailable` / `courseRenameHint` | 「快照兜底/快照版本无法识别/结构快照…一个不动」 | 「知识库暂时连不上，将基于本课已生成内容继续」「覆盖情况暂时看不了，不影响学习」「只改名，内容与进度不受影响」 |
| `growthPillStepRepair` / `growthPillStepMount` | 「按留痕与观察修补」「挂载节点到结构块」 | 「根据生成记录微调课程结构」「对齐内容与目录」 |
| `growthPillRedraftGates` | 「结构门打回，正在重新起草」 | 「AI 检查未通过，正在重写（第 {{count}} 次）」 |
| `selectKnowledgeBaseHint` | 「参与采样与课时 grounding」 | 「所有选中的库都会用于出题和课时内容」 |
| `genTriageLine` | 一行五段箭头斜线 | 「已整理 {{total}} 个来源文件：核心 {{core}}、跳过 {{skipped}}」，其余进明细 |
| `genCoverageReady` | 「覆盖读数：…显式跳过…兜底课时」 | 「已覆盖 {{covered}} 个主题，跳过 {{skipped}} 个，{{fallback}} 节为基础课时」 |
| `courseGoalHint` | 「整体文本即目标…主索引」备忘录腔 | 「这段文字会用作课程标题，也决定 AI 挑选哪些知识库内容」 |
| `kbSelectionTooMany` | 「不会被静默丢弃，也不会提交」 | 「最多选 {{count}} 个，请先取消一个」 |
| `sectionDegradedWarning` | 「可视化承诺未能兑现，已降级为纯文字保底」 | 「这一节的图表没能生成，目前只有文字；可试试 AI 重写」 |

**注意**：`growthPillStep*` 八档（v3 扩展）里 triage/repair/mount 三档在本
票改写——改的是词典值，step→key 机器面不动；`structureBadgeRunning`/
`genBgStructureRunning*`（v2/v3 新 key）已是口语化标准，不回改。

## 法证证据（2026-11-11 词典面探查 J1 #7/#8/#9/#10/#12/#13/#14/#15、J4 #1/#3/#9/#8、P1 #4/#5）

- 生成进度条是用户必经面：分诊/审计/资源槽三词每门课都出现。
- en-US 同病（triage/audit/free slot/idempotent/grounding/snapshot）。

## 实施要点

- [ ] 上表 key 双语改写 + 相关既有测试更新 + 真实词典 JSON 断言遍历。
- [ ] `rg "分诊|审计|资源槽|幂等|留痕|挂载|grounding" zh-CN/learning.json`
      本域清零（保留隐喻清单内的用法除外；settings 页的留痕/补挂归 o5）。
- [ ] `gen:i18n` + `check:i18n` + typecheck + learning 测试。

## 验收

- 本域目标词归零；渲染测试（进度条/排队/失败重试提示）全绿。
- 真机目验：建课全程提示读一遍（两语）。

## 非目标

- 工作区结构区术语（o4）、设置页（o5）、呈现密度（o6）。
- 「生长/教练/块/XP」保留隐喻（map 裁决 3）。

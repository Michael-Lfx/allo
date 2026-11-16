---
id: o5
title: "设置页与遗留文案口语化——留痕/补挂/回合/环境开关清零"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# o5: 设置页与遗留文案口语化（词典 J1/J3/J4 余量）

**What to build:** 设置页（LearningSettings）与其余散点文案口语化：

| key | 现状（病） | 改写方向 |
| --- | --- | --- |
| `settings.maintenanceAutoEnabledHint` | 「定时消费结构评审留痕并补挂节点（先修补后挂载；零工作不跑模型）」 | 「自动定期检查并补齐课程结构；无事可做时不调用 AI」 |
| `settings.maintenanceIntervalMinutesHint` | 「自动维护回合的间隔（30–10080，默认 360）」 | 「自动维护多久运行一次（30–10080 分钟，默认 360）」 |
| `settings.autoRefillEnabledHint` | 「（手动补充永远可用；环境开关可整体关闭）」 | 「打开课程时内容不足会自动补一批」 |
| `settings.fsrsParametersHint` | 「保存时自动附加算法标签（fsrs6）」 | 「保存时自动标记算法版本」 |
| `settings.wideContentHint` | 90+ 字含 3 处像素值 | 像素值删除，压缩为一句 + tooltip 分行 |
| `capacityFeedbackIntro` | 「自报…超出时长预算。你的选择只作留痕」 | 「AI 生成这一节时觉得内容偏多；你的选择当前不会自动改动课程」 |
| `capacityFeedbackHint` | 三重否定链 | 「这只是记录你的偏好，课程安排不受影响」 |
| `deleteCourseConsequenceRecords` | 「此后各安排面（复习队列/就绪推荐等）不再出现本课」 | 「进度和 XP 保留，但本课不再出现在复习队列和推荐里」 |
| `learningGraphRepairStaleBanner` | 「结构已修补，既有挂载待重评估——运行一次挂载即可全量重评」 | 「结构已更新，点击「重新检查」把现有内容与新结构对齐」 |
| `questionQueueLegend` | 一行 4 状态 =号分号链 | 逐状态分行或 tooltip |
| `learningGraphMountCounts` | 「已挂 N · 结构外 M · 未评估 K」 | 「已对应 {{mounted}} 个，待检查 {{rest}} 个」 |
| `sourceCoverageFallback` | 「兜底课时（按标题/文件名生成，非正常设计）」 | 「{{count}} 节简化课时（依据标题生成）」 |
| `sourceCoverageAnchorUncertain` | 「主题锚点无法精确定位（按文件级兜底）」 | 「{{count}} 个主题只能定位到文件，未到章节」 |
| `questionManager` 其余内部账目 key | 施工时同域核对 | 同水位口语化 |

## 法证证据（2026-11-11 词典面 J1 #2/#6/#11、J3 #3/#6/#7、J4 #2/#12、P1 #1/#2/#3/#5/#6）

- 设置页是 J1「留痕/补挂/回合」密度最高面；「环境开关」是部署配置披露
  （J3 典型）。
- en-US 同病同步改。

## 实施要点

- [ ] 上表 key 双语改写 + 断言；`rg "留痕|补挂|回合|环境开关|安排面" zh-CN/
      learning.json` 清零。
- [ ] LearningSettings / QuestionManager / CourseSourceCoverageCard 相关
      渲染测试更新。
- [ ] `gen:i18n` + `check:i18n` + typecheck + learning 测试。
- [ ] 顺路核查（非强制）：`sourcePlanLocales.test.ts` 的 2 例存量红测
      （`costGate*` 词典缺失，v2 施工时发现留痕）——若触及 sourcePlan
      文案顺带补词典归零，否则留独立小票。

## 验收

- 本域目标词归零；真机目验设置页读一遍（两语）。
- 闸门：typecheck / check:i18n / learning 测试全绿。

## 非目标

- 呈现密度重构（o6）、错误直泄（o1）。
- 设置项行为与后端语义零变化（纯词典 + 呈现措辞）。

## Resolution（2026-11-11 施工收口）

**代码面验收全过，已闭票；真机目验（设置页两语读一遍）与欠账合并执行，FAIL 按票重开。**

- 施工 = `6f99bf2a5` + `07b410416`（10 文件 +217/−60）：票面 14 行全覆盖（zh+en）+ o3 移交 4 挂载键（`MountRun`→「重新检查」顺带修复 banner 所指按钮不存在的问题、`MountTitle`→「内容与目录对应」、`MountStale`、`SystemActionKindMount`→「结构对齐」与 o3 taskKind 同词）+ 同域顺手 `courseArchiveDone`（安排面→复习队列/推荐）。
- **costGate 存量红测归零**（与票面建议的偏差，已留痕）：删 `sourcePlanLocales.test.ts` 的 4 个 `costGate*` 陈旧断言而非补词典——d5 已删计划卡 UI 与词典键且全库无消费点，补词典 = 复活死文案。
- **共享面声明**：`pages/settings/.../PreferenceRow.tsx` description 放宽为 `ReactNode`（additive，14 处消费点零行为变化）——学习模块之外的共享组件，本波唯一一处（map 非目标为 common 层；此为 pages/settings，评审判轻越界、保留并记录）。
- 后置建议（进 map）：「修补」词族（`learningGraphRepairRun/RepairNoop/SystemActionKindRepair`「结构修补」）不在必改清单、与 o3 修补域相邻——留独立小票。
- 263 绿（施工时）→ 316（全波终态）绿；`sourcePlanLocales.test.ts` 2 红 → 0。

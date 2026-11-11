# u4: 完成结算页（逐节点对账 + 分级 + 校准裁决）

## Status

施工中（2026-11-11 开票）。

## Terms

- **R1（结算呈现）**：u3 会话末题后 `quizComplete(courseId, roundId,
  answers)` → `QuizRoundReport` 呈现页（会话容器内的终态，非独立页面）：
  - 总净入账 `earned_xp`（全卷 Σ，折叠不计薪已扣）；
  - 逐节点对账行 `covered[]`：`node_name` / `answered` / `correct` /
    `earned_xp`——**折叠节点 `earned_xp = null` 呈现「同日已结算 · 不计薪」**
    （learnhub quiz_settled_on_day 语义；不得渲染成 0 或空）；
  - 逐题校准点 `questions[]` 本票不单独呈现（数据已落盘喂校准面，见 R3）。
- **R2（分级文案）**：准确率 = Σcorrect / Σanswered，镜像 settle.rs 分级
  阈值——`≥0.9` excellent / `≥0.6` pass / `≥0.3` half（阈值**注释标注
  settle.rs 常量单源**，前端不自定数值）；三档文案进 u1 词典，呈现带
  简短激励/建议语境句（口语化，遵 o 波口语化基调）。
- **R3（校准裁决——票面裁决项）**：核对 quiz 校准点落盘是否已进
  MemoryHealthPanel 消费的 stats 源 bins（数据源：stats/calibration 聚合
  面 vs quiz 回合落盘）：
  - 已含 → **不接** `GET /api/learning/quiz/calibration`，MemoryHealthPanel
    零改动（本票只留核对记录）；
  - 缺 → 校准 bins 并入 MemoryHealthPanel（呈现形态沿现有 bins，不新造
    面板），端点封装补进 api.ts。
  裁决与证据（查询结果）写进 Resolution。
- **R4（收口链路）**：结算页提供「完成」回到课程详情页；u2 的
  `onQuizFinished` 回调触发 progress 刷新——N/K 归零读数立即可见
  （「距上次检测 0/K」重新积累态）。

## Acceptance

- 渲染测试：报告呈现（总入账 + 逐节点行）/ 折叠态 null 呈现 /
  三档分级文案断言（边界值 0.9/0.6/0.3 恰好命中档）。
- 收口链路断言：完成后 progress 刷新被调用。
- `bun run typecheck` 绿；文案全走 i18n。

## Resolution

（施工后收口）

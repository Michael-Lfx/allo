# u2: 课程详情页测验入口 + 进度读数

## Status

施工中（2026-11-11 开票）。

## Terms

- **R1（入口区块）**：`CourseWorkspace`（`ui/src/renderer/pages/learning/
  components/CourseWorkspace.tsx`）内新增测验区块，挂点随课程详情页现有
  布局（票面裁决：进度卡内嵌按钮 vs 页面区块，参照 ReviewBanner 与现有
  卡片层次取其一，不新造第三种形态）。区块呈现：
  - 进度读数：`settled_count/trigger_count`（K 单源后端，前端不自定 6），
    learnhub #90 形态——「距上次检测 N/K 次学新结算」；
  - due 态（`due=true`）：「开始测验」按钮可点（接 u3 会话容器，本票先
    留 `onStartQuiz` 回调位，u3 接线）；
  - 未 due 态：按钮禁用呈现、**不隐藏**（与 ReviewBanner 空队列保留入口
    同理——按钮凭空消失违背可预期性）。
- **R2（刷新时机）**：进入课程详情页时 + 测验会话结束回调后刷新
  `quizProgress`；**不轮询**（读数面非门禁，结算后由会话结束回调驱动
  归零呈现；轮询徒增请求面）。
- **R3（409 竞态防御）**：进度刷新竞态（读数非门禁，点开测瞬间可能被
  后端 409 拒）——本票只负责入口区块对 409 的兜底呈现（toast + 刷新
  进度），会话容器内的完整边界态归 u5。
- **R4（课程存在性）**：课程归档（enabled=0）或删除（墓碑）时不渲染区块
  （跟随课程详情页的存在性语义；后端 due_reviews 对归档课程收窄同源）。

## Acceptance

- 渲染测试两态：due（按钮可点 + N/K 读数）/ not-due（按钮禁用 + 读数）；
  归档课程不渲染断言。
- `bun run typecheck` 绿；文案全走 i18n（u1 词典）。
- 真机目验（launch-sim 造数库 8/6 due 态可点）随 u5 走查一并收口。

## Resolution

（施工后收口）

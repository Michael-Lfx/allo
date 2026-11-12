# u9: 软闸行动闭环——due_count 修复 + 薄弱集复习会话

## Status

施工中（2026-11-12 开票；真机走查暴露 + 用户两轮设计拷问后的修正案）。

## 已核验事实（2026-11-12 逐条对码，非推测）

- **R1 bug 根因**：`learning_graph.rs` 软闸到期数查询把 `user_id` 推进 IN 列表
  （`separated.push_bind(user_id)` 首位）——占位符 N+3 vs 绑定 N+2，尾部
  `ri.due_at <= ?` 落到无绑定态，SQLite 按未绑定 = NULL → `due_at <= NULL`
  恒假 → **due_count 恒 0、无任何报错**。真机实证：seed 软闸臂供给节点卡
  `is_due=1` 而建议条显示「0 道到期题」。邻位的卡 R 查询块（占位/绑定平衡）
  不受影响——r 读数正常、due_count 恒 0 的组合与截图一致。
- **早练语义**（非死态依据）：当日尚未推进过的卡允许提前练一次（CONTEXT.md
  复习队列词条；due-gate = 重复作答只记流水不推进）——`due_only=false` 的
  复习会话可出示未到期卡并真实推进，due_count=0 态有活路。
- **数据面**：`GraphBlockedView` 含 `supplier_lesson_id` ✓；`due_reviews`
  多值 course_id/tag 手工收集但**无节点过滤**；`DueReview` DTO 无
  `lesson_id`（只有 lesson_title）。
- **报告面**：`QuizRoundNodeReport.node_id` 在场——薄弱集可从报告聚合。
- **机制结论（用户拷问定案）**：r 回升唯一途径 = 复习队列真实推进；重读
  正文与课时内重做均不推卡（in-course attempts never reschedule）。遗忘态
  只做题大概率再错——行动必须是「重学 + 推卡」双步，不能只有做题。

## Terms

- **R1（后端 bug 修复）**：due_count 查询重排绑定——IN 列表只放 lesson ids，
  user_id 挪 WHERE 段正常绑定，与 `ri.due_at <= ?` 数量对齐。**回归单测**：
  构造「blocked + 供给节点有到期卡」路径断言 due_count ≥ 1（原缺陷漏测点，
  红测：先断言现状恒 0 再修）。
- **R2（节点过滤数据源，实现路径票面裁决）**：`due_reviews` 支持节点集合
  过滤——方案 a：新多值 query 参数 `node_id`（镜像 course_id 手工收集模式）；
  方案 b：`DueReview` DTO 透出 `lesson_id`，前端按薄弱集过滤。推荐 a
  （scoped 会话不下发全量列表）。
- **R3（软闸行动闭环，重学优先双步）**：
  - 主行动「复习该节点」：开**供给节点（或薄弱集）范围的复习会话**
    （`due_only=false` 早练语义，出示含未到期的该节点卡）——唯一能改 r 的
    动作；due_count=0 态非死（早练可推）。
  - 次行动「重读正文」：u8 跳转保留降为次按钮（真忘了先重学，学完再推卡）。
  - 多节点聚合：软闸多条 blocked 合并为一条（列节点名按最差 r 排序，≤3
    截断保留），行动开薄弱集 scoped 会话；**检测报告尾部**聚合 CTA 同语义
    （薄弱节点 = 对账行分级 ≤ half 的 node_id 集）。
- **R4（文案事实分支 + 降噪）**：due_count > 0 → 「N 道到期卡」；= 0 →
  「卡未到期，可提前复习巩固」；当前选中节点 = 供给节点时建议条收起
  （u8 既有选中机制可判）。
- **R5（明确不做）**：浮动坞 Pill、通知、今日安排面、g4 反哺、ready_count
  软闸前口径字段（维持 map 后置）。

## Acceptance

- Rust：due_count 修复回归单测（先红后绿）；`cargo test -p nomifun-learning`
  相关簇绿；fmt 过。
- 前端：typecheck / check:i18n 绿；软闸聚合条 + 双行动渲染测试；报告尾部
  CTA 渲染测试（薄弱集提取）；learning 回归绿；theme/dead-css 干净。
- 真机（seed v3 软闸臂）：软闸条显示真实 due_count ≥ 1 → 主行动开复习
  会话（含未到期卡）→ 推卡 → 重新进课程页 r 回升条消失；次行动跳正文；
  报告尾部 CTA 开薄弱集会话。

## Resolution

已施工收口（2026-11-12，2bd4eeeea + 86f777414 + 1cd198f38 + 评审修复
cc951de47 + 9113cfcce，交付合并 ab4d11139 已推）。

- **R1**：红绿实证——红相 `GraphBlockedView { r: 0.76, due_count: 0 }`
  （与真机截图组合一致），修后绿；全量 `cargo test -p nomifun-learning
  --lib` 966 绿。**实弹探针**：同一份 seed 数据，修复前真机截图「0 道到
  期题」→ 修后 course detail 返回 `due_count: 1`（r=0.809）。
- **R2**：方案 a 落地——`DueReviewQuery` 不动，routes 手工收集多值
  `node_id`（照 course_id 模式），service 末位加参（18 处测试调用点补
  `&[]`）；**实弹探针**：`node_id=<节点6>&due_only=false` 精确返回该节点
  1 张卡。注意：node 过滤隐含排除自建题（无节点归属，结构性不匹配）。
- **R3**：软闸多条聚合为一（节点名按最差 r 升序、≤3 截断保留），主行动
  「复习薄弱节点」开 scoped 早练会话（**裁决：只传 nodeIds 不带课程过滤**
  ——ready_partition 跨课程取教师，课程过滤会把跨课程供给卡静默滤空；
  Spec 轴越权核查通过：due_reviews 恒 JOIN 用户注册队列，未报名课节点
  匹配零行 + answer_review 二次校验 404）；次行动「重读正文」保留；
  检测报告尾部薄弱集 CTA（对账行分级 ≤ half 的 node 集）。
- **R4**：文案事实分支（`WeakAdviceDue`「N 道到期卡」/`WeakAdviceNotDue`
  「卡未到期，可提前复习巩固」）；选中节点 ∈ 薄弱集 → 整条收起（聚合语义
  下唯一自洽读法）。
- 评审轮：Standards（红线全过、3 smell）+ Spec（越权核查通过；揪出
  **必修硬伤** learning_smoke.rs 6 参调用 E0061 → 修复 + `cargo check
  --workspace` 绿 8m06s）→ 修复代理 F1–F3 收口。
- 闸门：`bun run check` exit 0；learning 回归 408 绿；Rust quiz 簇 +
  learning_graph 966 绿；fmt 干净；词典 7 新键 2 退役全对称。
- **桌面走查欠账**（IAB 宿主不可用，清单交用户）：`--graph --fresh` 灌库 →
  学习首页感知横幅（图课 6/6）→ 图课程软闸条显示「1 道到期卡」（不再
  是 0）→「复习薄弱节点」开 scoped 会话（含未到期卡）→ 推卡后 r 回升
  重新进页条消失 → 检测全链 → 报告尾部「开始薄弱复习」CTA。

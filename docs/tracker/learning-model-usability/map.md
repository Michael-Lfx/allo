# Map: 模型可用性修复波（u01–u11）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-06）。来源 = 全生成流程五路「模型可用性」审计（教练/评审/
      内容 agent/结构维护/出题判分），读实码 + 对照真机档案（absorb8-smoke、
      absorb7-smoke），清单见记忆 generation-pipeline-model-usability-audit。
      用户指示：立票并修，**要保证修复都是正优化而不是负优化**——本波总约束
      第一条即为此设的验收协议。 -->
<!-- 基线：feat/learn0928 尖端 7478dc2de。 -->

## 根因（两类）

1. **让模型搬运本该由代码持有的状态**：点修要模型整批重发 JSON 还要逐字不改；
   内容 agent 每轮重发全部历史；结构双产物连坐重掷。模型多抄一遍就多一次漂移
   或多一份冗余。
2. **失败信号到不了模型，或到了也看不懂**：修补应用面失败直接终局；math_verify
   失败在发布门后才追加；超时重试套「修正违约处」话术；守卫文案被读反。

## 总约束：正优化验收协议（本波最高优先）

1. **每票三件套**：①主指标（基线值取自现有真机档案，写进票面）；②不劣化护栏
   （既有单测全绿 + 相关 smoke 判据全过 + 票面列出的质量指标不降）；③开关
   （见下条）。缺一不收票。
2. **开关规则**：凡改变**模型可见内容或模型被要求的行为**的改动（提示词条款、
   上下文装配、工具语义、循环节奏），一律挂 `NOMI_LEARNING_*` 开关——off 臂与
   基线逐字节一致（单测钉住），on 臂为新行为。**默认值由真机 A/B 决定**：主指标
   改善方向成立且护栏不劣化 → 默认 on；不成立或不确定 → 默认 off 出货并记录。
   开发期缺省一律 off，真机 A/B 达标后在收尾提交统一翻 on（含断言同步）；
   集成分支在 A/B 定论前不并回 learn0928。
   机械必然项（bug 修复、静态门补检、契约同源、纯增量留痕）不挂开关，单测证明 +
   smoke 回归即可。
3. **不新增硬阻断**：新检查一律走「反馈重试」或「warning 留痕」，不新增终局失败
   路径——防止为了更严而把可用产出挡掉（负优化的典型形态）。
4. **A/B 规程**：同一二进制、同一目标、全新库（搬同一 provider 行）、两臂串行；
   模型行为项每臂至少 1 跑，结论不清的加跑 1 次；指标表落票尾 Resolution。
5. **不确定项写推翻条件**：票面写明「出现什么读数就回退默认 off」。
6. 构建纪律沿用：worktree 双 env 前缀共享缓存 + 首次 cargo 自检；**测试前强制重链
   本票改动包**（共享缓存同名二进制劫持教训）；最终验证一律主仓串行跑。

## 票表与车道（车道内串行、车道间并行）

| 车道 | 票 | 内容 | 类别 | 开关 |
| --- | --- | --- | --- | --- |
| A 生长 | u01 | 重裁同标题未点名节点回填 + 守卫文案改写 | 模型行为 | `NOMI_LEARNING_REDRAFT_SPLICE` |
| A | u02 | 评审重试按失败原因分账 + 超时重试话术 + 降级保全 problems | 机械/增量 | 无 |
| A | u03 | 评审输入补全：挂号表逐字候选词、一条 problem 可并列多词、默认方向厘清 | 模型行为 | `NOMI_LEARNING_REVIEW_CONTEXT_V2` |
| A | u04 | 首批课程语境词豁免 + 评审翻案锁 + intent/scope 边界去重 | 模型行为 | `NOMI_LEARNING_ANTI_LOOP` |
| B 内容 | u05 | verify_math 批量（claims 数组，向后兼容） | 工具 | `NOMI_LEARNING_VERIFY_BATCH` |
| B | u06 | 历史压缩实验（已落库正文换摘要 + ls_inspect 取回原文） | 高风险 | `NOMI_LEARNING_LESSON_HISTORY_COMPACT`（**缺省 off**） |
| B | u07 | 档位优先条款 + 节长上限 warning + 发布前数学失败可见 + 修复轮带上下文 | 模型行为+机械 | `NOMI_LEARNING_TIER_PRIORITY`（仅档位/上限） |
| C 结构 | u08 | 修补回合：应用模拟+复核并入静态门走反馈重试；快照补主干名单 | 机械 | 无 |
| C | u09 | 结构提示词「和」自检句对齐门；goal 字段豁免排期扫描 | 机械 | 无 |
| D 出题 | u10 | 变式判底正文/缺席跳过 + 证据逐字核 + 按题型装配契约 + tol 透传 + 指令独立段 + 语言跟随 | 机械+模型行为 | `NOMI_LEARNING_VARIANT_V2`（判底/契约/指令段） |
| D | u11 | smoke 夹具：「贝塔系数」别名换成正确同义名 | 卫生 | 无 |

## 文件冲突面

- 车道 A：`learning_graph/mod.rs`（教练/评审提示词、concept_review）、
  `service/learning_graph.rs`（growth_body）、`growth_retry.rs`、`fragment_verdict.rs`
  ——四票串行同一实现者。
- 车道 B：`nomifun-ai-agent/src/lesson_content_loop.rs`、`loop_core.rs`、
  `learning_loop.rs`、`nomifun-learning/src/lesson_draft.rs`、`service/lesson_draft.rs`、
  `verify_math.rs`——三票串行同一实现者。
- 车道 C：`structure_ops.rs`、`mounting.rs`、`structure_contract.rs`、结构提示词资产、
  `service/learning_graph.rs` 的修补快照/回合区（~:938-1087，与车道 A 的 growth_body
  ~:1938+ 异区）。
- 车道 D：`service/quiz.rs`、`generation/activities.rs`、`generation/mod.rs`（出题
  提示词）、`nomifun-app/.../learning_smoke.rs`（夹具，与其它车道的 smoke 读数行
  异区——合并时注意）。

## 推迟项（不在本波，附理由）

- 评审拆成两次调用（语义面/机械表分离）：大改动，先用 u02 分账重试与 u03 减负看
  超时率，有数据再议。
- 开放题生成期 rubric：生成+判分两侧改动，独立立项。
- 删除 search_concepts：absorb-7 立项功能，无伤害证据，不删。
- 结构双产物按产物认领重试、挂号表关系条款渲染、判分判对/缺失互斥（仅推断）、
  内容 warning 行动压力（回归风险）：低优先或证据不足，记观察。

## 验证总纲

1. 车道内每票：`cargo test -p <改动包>` 全绿 + 新测（含开关 off 臂逐字节回归测试）。
2. 合并后主仓串行：learning / ai-agent / app / db（单线程）全量 + workspace check。
3. 真机 A/B：A 车道 grow 臂、B 车道 content 臂、C 车道 structure 臂（修补无真机
   基线→单测为主）、D 车道 quiz 臂（先补基线跑）。

## A/B 规程补充（2026-11-06 开工后）

- **两臂同一冻结二进制、仅开关不同**：全部车道合并后在主仓构建一次 nomicore，立即复制到
  `C:/Users/test/Desktop/work/absorb8-smoke/bin-ab/` 冻结使用，所有 A/B 臂都跑这份副本。
  原因：并行车道的 `cargo test -p nomifun-app` 会经集成测试重链共享目标目录里的
  `target/debug/nomicore.exe`（17:27 实测被改写），开工前「改动前二进制」不可信。
- off 臂由单测保证与基线逐字节一致（开关项），机械项两臂同享——两臂差异只来自开关。
- 既有真机档案（r01 两跑、pctx content 干净跑）作历史参照，不作对照臂。

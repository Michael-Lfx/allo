# Map: 模型可用性收尾修复波（w01–w04）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-08）。来源 = 第二轮五路复审（对照第一轮逐条核验 u01–u11
      修复效果 + 找回归与新问题）。用户指示：再次修复，不要过度优化，做真实
      可用的优化，确实没有意义就不做——本 map 的「不做清单」与票面取舍均按
      此裁决。 -->
<!-- 基线：feat/learn0928 尖端 765bce772（含可用性波与开关缺省收口）。 -->

## 取舍原则（承正优化协议）

修「有真机/实码证据、修了模型立刻受益」的问题；**不做**证据不足、修了没人
受益、或属设计裁决的项——每一条都写明理由，允许推翻。

## 不做清单（明确不做，附理由）

| 项 | 不做理由 | 重启条件 |
| --- | --- | --- |
| 评审拆两次调用（语义面/机械面） | off 臂评审稳态 316s/15.1KB，未破 480s；r01 的超时实例是 SSE 传输拖累非稳态 | 稳态超时再现（评审调用 >480s 或降级率上升） |
| COMPACT 取回占位（ls_inspect ToolResult 永驻历史） | COMPACT 已 off，重测前修复无受益方 | 决定重启 COMPACT 实验 |
| 保守放行丢弃合法 reject 的语义 | 设计裁决（评审器故障 ≠ 批缺陷，ADR-0026/m3），r01 起已可见；改语义需用户拍板 | 用户裁决 |
| 挂号行引用条款零执行、名词修法纸面冲突、双产物连坐、s2 超上限 warning 无牙 | 低价值或设计使然（warning 本就非门禁；模型对纸面冲突已自化解） | 出现实际损害 |
| A/B 归档勘误 | 流程项：第二轮已确认正确对照对——quiz off=`run-1793977442245`（retries=2）/ on=`run-1793977484697`（retries=0, misses=0）；本 map 即记录 | 无 |

## 票表（四票并行，文件面互斥）

| id | 票 | 严重度 | 类别 |
| --- | --- | --- | --- |
| w01 | 点修基准污染修复——守卫原地重试不得替换上一稿基准 | 中高（真机坐实越权落库） | 机制 bug 修复 |
| w02 | 切片质量——语境词/候选词的二元垃圾过滤 + 教练全批语境词 | 中（两路独立审计同指） | 模型可见内容（ANTI_LOOP on 行为精化） |
| w03 | 提示词对齐——结构 goal 回执例外句 + 出题形状示例去字母答案 | 中/低（均有真机/读数证据） | 提示词资产修正 |
| w04 | 评审翻案条款补全 + 卫生批（开关注释/测试消息同步、app 测试 serial、回合同干题面池） | 中/低 | 提示词条款 + 卫生 |

## 文件冲突面

- w01：`service/learning_graph.rs`（growth_body ~:2140-2280 守卫/重裁区）+ `growth_retry.rs`——独占。
- w02：`learning_graph/mod.rs`（course_context_words_block ~:650 / derive_naming_list ~:3350 / candidates 渲染）+ `generation/terms.rs`（切片器）——独占；`ANTI_LOOP` on 行为精化不新增开关。
- w03：`prompts/structure/goal-decomposition.md` + 金样 + `generation/mod.rs`（形状示例）+ `generation/tests.rs`（golden）——独占。
- w04：`learning_graph/mod.rs`（评审判据条款区 ~:976-981/214 字条款附近）+ `switches.rs`（注释/测试消息）+ `service/quiz.rs`（回合同干池）+ `nomifun-app/src/app_server_catalog.rs`（1 行 serial）——与 w02 在 mod.rs 异区，可并行。

## 验证总纲

1. 每票 `cargo test -p <改动包>` 全绿 + 新测；四票并回后主仓串行全量
   （learning/ai-agent/app + workspace check）。
2. 真机回归（合并后冻结二进制，on 臂）：grow ×1（w01/w02/w04——守卫重试
   不再污染基准、语境词块无垃圾串）、content ×1（w03 形状示例——首答拒绝
   下降）、structure ×1（w03 goal 句）、quiz ×1（w04 同干题面）。
3. A/B 不重跑：本波全部是已验证机制的 bug 修复/精化/文案对齐（map 总约束
   第 2 条的机械项），真机回归只验不劣化。

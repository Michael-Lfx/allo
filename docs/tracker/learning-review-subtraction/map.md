# Map: 评审域减法波（rs1–rs5）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-20）。触发=用户真机问「补充节点 4:58 为什么
      这么慢」→ 全链路法证（38 次教练调用史 + smoke 档案 token 级读数）
      → 初版减法案（删挂号表/机械预裁噪声）被 learnhub 对照推翻大半，
      修订为「只吸收上游已验证的机械减法」。用户裁决：试试吧，如果负
      优化就换方法；先立票不开工。 -->

## 已核验事实（2026-11-20 对码 + 档案取证）

### 时间解剖（token 级硬证据，smoke run-1794819652922 档案 + 11-20 live 日志）

- **教练调用是思考 dominated，不是输出 dominated**（修正初诊）：
  growth_draft 70s = output_tokens 10,774 其中 **reasoning_tokens 9,561
  （88%）**，可见回包仅 2419 字符；growth_review 162s =
  output_tokens 31,290 其中 **reasoning_tokens 28,579（91%）**，可见回包
  7571 字符。评审的 28.6k 思考 token = 逐词落行程序真正在执行——这就是
  挂号表作为「强制注意力」的机制形态（ADR-0033 #169 病历的对偶面）。
- live 11-20 干净轮（attempts=1）：draft 215s + review 348s + 机械面
  ~4s = 565s；同类 draft 历史方差 20s→215s（同形同模型）= 云端方差。
- max_tokens 与 reasoning 的份额关系：档案里 review output_tokens 31,290
  > CONCEPT_REVIEW_MAX_TOKENS=16384——本 provider 思考 token 不占（或不
  只占）completion 预算，与 completer.rs:45 注释假设不符（观察项，不在
  本波修）。

### 病态尾部（11-16 单日实锤，全部有留痕）

- **返工率 6/16**：16 次 growth 里 6 次调用数 >2（打回环），最长
  1146s/6 调用（19 分钟病历）。
- **59,617 字符呓语**（11-17 01:16）：评审输出契约六份工作，模型把思
  考写成意识流回包，烧 278s 后 parse-fail → 死因重试恰一次（72s）救回。
  属 ADR-0033 §5 定价的「残余风险：可观测可数，不改 Degraded 语义」。
- **静默降级放行 ×2**（11-16 09:34 / 10:03）：SSE 掉线（failed-provider-sse
  留痕）→ 重试 **0 秒瞬时双败**（无退避）→ 未经任何评审 PASS，仅
  findings+note 留痕。
- **契约死因重掷成本**：+303s/次（手冲滤杯循环自证案，p01 留痕）；上游
  同族病 2026-11-17 病历=同一行重掷 3 次、每次 57–137s（ADR-0033 #252
  修订的驱动）。

### 上游对照（learnhub /c/Users/test/Desktop/my/learnhub，2026-11-20 现行）

- **挂号表删除 = 已被上游病历否决的负优化**：#127（有理数）+#169（三角
  形）两次原则形态静默放行 → #169 程序化后回放 5 跑 4 拒 1 放。「完备性
  机械扫描替代」在 ADR-0033 备选节明文拒绝过（误报消耗评审注意力）。
- **上游自己走的减法路 = 机器可证出口机械化**：
  - **#252（2026-11-17）常识行撞名池机械归一**：整份作废重掷 → 机械改
    判+findings 留痕不挡门；其余行伪维持 Err。实码
    `src-tauri/src/engine/node_growth/reviewer.rs:535`（归一）+ `:2103`/
    `:2178`（Scripted 测试）。
  - **#213（2026-11-05）正向快路**：term 逐字命中本批教出名集 = 机械放
    行出口，「出处机械可证，快路只收紧」；同票清单合并渲染+单字过滤。
  - **#218（2026-11-06）未来提及边界**：路线图提及判噪声不作范围外。
- **allo 版本差**：#252/#213 快路/#218 均未吸收。allo 现状=
  `provenance.rs:15` 仍是否决语义 + `mod.rs:979` 同形词盲区注记（提示词
  伤疤让模型自己绕）+ `mod.rs:1013` p01 噪声出口（散文自证，run-1793828585963
  滥用面）。单字过滤与合并渲染 allo 已有。

### 观测面现状（修正初诊）

- **smoke 臂的 coach 调用已被 ArchivingCompleter 全量留痕**（含
  elapsed_ms + usage.reasoning_tokens；`learning_smoke.rs:231-232`
  `set_generation_dependencies(knowledge, pipeline)`）——本波 A/B 读数
  基建已在，不需新建。
- **live 桌面端零调用留痕**：本次法证只能靠聚合日志行反推。生产接线缝
  = `nomifun-app/src/services.rs:2922`。lesson agent 引擎在 smoke 里走
  OneShotDeps 自解析 provider、不经管线 completer（首轮观测帧除外）。

## 用户裁决（2026-11-20）

- **条件批准**：试试吧，如果负优化就换方法——每票按验收读数判负优化，
  命中各自回退条件即回退或改法（见各票 R 节）。
- **先立票不开工**。
- **范围外（本波不做）**：挂号表/碎片表删除（上游病历否决）；评审
  thinking 开关与默认模型替换（质量门槛，ADR-0026/0033 语义）；draft
  契约与常规路径提速（思考方差=云端，头部下拉用户可自选模型）。

## 票表

| id | 票 | 严重度 | 类别 |
| --- | --- | --- | --- |
| rs1 | live 调用档案：ArchivingCompleter 接生产缝，开关控制 | 中（观测缺口=每次法证多付一小时税） | 观测面 |
| rs2 | 吸收 #252：常识行撞名池机械归一+留痕，同形词注记条款退役 | 高（重掷环 2-5 分钟/次，静默质量税） | 评审契约 |
| rs3 | 吸收 #213/#218：正向快路机械出口 + p01 散文出口退役 | 高（打回好批翻案环 + 话术滥用面） | 评审契约 |
| rs4 | 评审传输重试退避（0s 瞬时双败 → 1.5s） | 中（两次静默降级放行的直接形态） | 评审韧性 |
| rs5 | 病历回放夹具：四病历 Scripted 钉死 + live 回放臂 | 中（负优化判定的读数前提） | 测试基建 |

## 施工顺序与负优化判定

1. 顺序：rs1 → rs2 → rs3 → rs4 → rs5（rs5 的 Scripted 部分随 rs2/rs3
   同票落地，live 回放臂殿后）。无硬 blocked_by。
2. 每票负优化判定 = 命中该票回退条件（票面 R 节），或双臂 smoke 对照
   出现 reject 方向翻转/该打不打的放行。命中即回退该票、换法再立票。
3. 契约改动（rs2/rs3）验收必含：`cargo test -p nomifun-learning` 全量绿
   + 双臂 smoke 对照读数（archiving usage 读数可查 reasoning/elapsed）。

## 修法落点（对应各票 R 节）

- rs1：`nomifun-app/src/services.rs:2922` 生产缝包
  `ArchivingCompleter`（`nomifun-ai-agent`），开关控制默认关；
  run 目录按日（observation/learning-live/），脱敏复用
  `register_redaction`。
- rs2：`provenance.rs` 校验器否决臂改归一臂（三前提：逐字命中名池 ∧
  已过定义原文逐字核 ∧ ref 为空）；`mod.rs:979` 条款退役；
  `ProvenanceReadout` 增 `normalized` 计数（GrowthRunStats 跟进）；
  findings 走既有 `record_review_findings` 通道（node="批"）。
- rs3：`fragment_verdict.rs` `check_out_of_scope_row` 三条件前置快路
  （term 逐字命中教出名集即放行）；`mod.rs:1013` p01 出口退役、正向提
  及回归范围外(b)+快路；`mod.rs:4059` 条款断言同步。
- rs4：`mod.rs` 传输重试分支（~1795）sleep 1.5s；cfg(test) 常量置 0
  （11 个重试族测试不减速）。
- rs5：四病历 Scripted 夹具（滤杯循环自证应重试 / 次数同形应归一留痕 /
  三角形折叠应打回 / 59k 呓语 parse-fail 恰一次重试应救回）+ smoke 增
  review-replay 臂（录制批次过真实模型 N 跑断言打回方向）。

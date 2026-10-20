# Map: 学习冒烟加固——首次带凭据真机暴露缺陷的收尾波

<!-- wayfinder:map -->
<!-- 状态：收尾（t1–t6 全关，2026-10-20 结案波；剩余项见 Notes 与 Not yet
      specified）。源起：2026-10-16/19 首次带凭据跑 `nomicore learning-smoke`
      全套（八档 EXIT=0）暴露的真机缺陷与产出质量差距，本 effort 收尾。 -->

## Destination

真机冒烟暴露的缺陷与差距全部收口：冒烟管线九档在 flash 上稳定全绿，产出质
量与 learnhub 的已知差距要么追平、要么票面裁决放弃。

## Notes

- **前情**：一轮/二轮/quiz 波全关闭（feat/learn0928）；首次带凭据真机记录见
  j10 增补勘误与 2026-10-16/19 审计记录。
- **已修（本波之前落地）**：结构生成 16384 token 帽截断（目录上限死管道改
  消费，ADR 语境见 0013 前身审计）；goal-decomposition 门口编号 `[G1]` 括
  号残留；生长门近形归一（ADR-0013）+ 同批依赖剪枝（ADR-0013）。
- **模型约束（用户明令 2026-10-19）**：不换模型——pro 现在反而更弱，flash
  是当前唯一正确选择；任何"升级模型"路线不做。
- **当前天花板**：深批次 round 3+ 评审多发现点修循环（flash 点修越权）；
  nearform/剪枝落地后死线已从 round 2 推到 round 3。

## Decisions so far

- （待裁）深批次批目标是否 7→5（learnhub 同为 1–5 档）：真机 round 3 复测
  后裁决，不预裁。

## Decisions so far

- （待裁）深批次批目标是否 7→5（learnhub 同为 1–5 档）：真机 round 3 复测
  后裁决，不预裁。

## 实施票已切（tickets/，依赖序见各票 blocked_by）

- [t1](tickets/t1-repair-smoke-seed.md) repair 冒烟种子结构性不可赢（两修
  法待票面裁决）；
- [t2](tickets/t2-richness-parity.md) 产出丰富度追平 learnhub（正文篇幅 /
  题目 evidence / 铸名出处，三条款独立可拆）；
- [t1](tickets/t1-repair-smoke-seed.md) repair 冒烟种子结构性不可赢（两修
  法待票面裁决）——**已关**（修法 1，`931932de4`，真机首次 EXIT=0）；
- [t2](tickets/t2-richness-parity.md) 产出丰富度追平 learnhub（正文篇幅 /
  题目 evidence / 铸名出处，三条款独立可拆）——**已关**（条款 1+2 落地
  `f0f1a07cc`+`679da6cfb`，条款 3 缓行裁决见票面；正文 3218 字 ≈ 3×，evidence
  6/6 落库）；
- [t3](tickets/t3-reuse-ruling-registry-validation.md) reuse 裁决校验放宽
  到登记表全量（小改，首选快赢）——**已关**（`17e1814a3`）；
- [t4](tickets/t4-review-advice-mechanical-alias.md) 评审建议机械转化
  （advice 指向别名的问题由代码执行，不折返模型）——**已关**
  （`2d8859436`，含撞池机械复用扩展）；
- [t5](tickets/t5-quiz-smoke-provider-tier.md) quiz 冒烟 provider 档（带凭
  据的全链变体验证；DoD 含域文档补 quiz 档记载）——**已关**
  （`931932de4`，双环境 EXIT=0）；
- [t6](tickets/t6-structure-stage-consistency.md) 结构 stage 标注一致性
  （提示词条款 + warn 级 validator，或接受生长补偿——票面裁决）——**已关**
  （方案 1，`7eec8be49`）。

## Notes（结案波 2026-10-20）

- **八次 grow 真机的事件链复盘**（预修复六跑 + 结案波两跑，run-1792401388594
  …run-1792464413123）：深批次（round 2+）死因三类——① 复用类打回（近义
  铸名 advice/撞池名），② 起草形状违约（未知键/中文档位「会用」
  ng.refs.tier/est 0），③ 形状或内容打回后的重裁点修越权
  （ng.redraft.scope）。t3+t4 把 ① 机械消化（结案波两跑 dropped=0、无一次
  复用类重裁）；②③ 成为剩余天花板——结案波两跑均 3/4 轮绿、round 4
  确定性死于「中文档位形状违约 → 重裁整批重写 → 点修越权」同一循环。
- **机械消化的下一候选**（未开票）：中文档位（会用/知道/能教）在
  ng.refs.tier 处机械映射到 know/apply/teach 而非打回——与 t4 同律
  （可机械判定的不送模型），能把 ② 类的最后一支也消掉。
- **七档全绿矩阵**（flash，dev 库）：xp（历史）/ json（历史）/ structure
  （历史）/ repair（本波首次 EXIT=0）/ quiz 双环境（本波）/ content（本波，
  读数达标）/ grow 4 批（3/4 轮，见上）；loop 档未单独复跑。

## Not yet specified

- 深批次批目标：7 vs learnhub 1–5 档——**已裁（2026-10-20 grill 轮）**：
  `MAX_BATCH_NODES` 与 `READY_TARGET` 双 7→5，归 t7 落地（用户明裁，推荐
  队列深度产品行为变更用户认可，整体测试兜底）。
- 铸名 source 出处——**已裁（2026-10-20）**：本波落地（t8），attestation
  口径，迁移 082。

## 增补票（2026-10-20 第二轮裁决）

- [t7](tickets/t7-tier-normalization-and-batch-five.md) 中文档位/est 字符串
  解析归一 + 教练面英文词表 + 批容量与就绪线双 5——grow 收官票；
- [t8](tickets/t8-mint-source-attestation.md) 铸名 source 落地（t2 条款 3
  补完，attestation 口径）。

收官后开草稿 PR（单分支单大 PR，四波导览描述）。

## Out of scope

- 学习图产品行为变更（水位契约、就绪触发线）——那是产品波的事，本 effort
  只管冒烟与产出质量。

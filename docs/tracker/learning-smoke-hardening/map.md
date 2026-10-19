# Map: 学习冒烟加固——首次带凭据真机暴露缺陷的收尾波

<!-- wayfinder:map -->
<!-- 状态：进行中。源起：2026-10-16/19 首次带凭据跑 `nomicore learning-smoke`
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
- [t3](tickets/t3-reuse-ruling-registry-validation.md) reuse 裁决校验放宽
  到登记表全量（小改，首选快赢）；
- [t4](tickets/t4-review-advice-mechanical-alias.md) 评审建议机械转化
  （advice 指向别名的问题由代码执行，不折返模型）；
- [t5](tickets/t5-quiz-smoke-provider-tier.md) quiz 冒烟 provider 档（带凭
  据的全链变体验证；DoD 含域文档补 quiz 档记载）；
- [t6](tickets/t6-structure-stage-consistency.md) 结构 stage 标注一致性
  （提示词条款 + warn 级 validator，或接受生长补偿——票面裁决）。

## Not yet specified

- 深批次批目标：7 vs learnhub 1–5 档——round 3+ 再失败 2–3 次后按数据
  裁决，不预裁（2026-10-19 用户裁决选 b 时搁置；已记于 Decisions）。

## Out of scope

- 学习图产品行为变更（水位契约、就绪触发线）——那是产品波的事，本 effort
  只管冒烟与产出质量。

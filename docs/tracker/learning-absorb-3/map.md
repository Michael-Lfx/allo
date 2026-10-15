# Map: 学习模块三轮吸收 learnhub——结构维护战役 / 会话账本线 / #99 产品波（114 提交）

<!-- wayfinder:map -->

## Destination

三轮吸收规格：learnhub 在二轮窗口（fb72157..7f57fc0，114 提交）的成果——结构转活文档
（挂载 + 修补 + 维护回合）、概念别名结构化升级（含 new_aliases 通道）、XP 结算参考实现、
长任务真取消、「现存且启用」课程管理口径、会话体系、产品面小件——哪些进 allo、以什么形态
进（含对二轮已裁未实施项的修订），全部裁决完毕，与二轮规格合并成可切票的实施底座。

## Notes

- **前情叠加**：一轮（已实施）+ goal-kb（已合并）+ **二轮（已裁决未实施**：
  `../learning-absorb-2/`——est/XP 统一账、内容成色纪律、别名闭环、装箱教义、重试机制）。
  三轮票的落点多数是二轮决议的**实现参考或修订**，切票时两轮合并排程。
- **重大交叉**：learnhub 的 concepts 域重构（别名携带定义/来源 + 批内 new_aliases +
  登记单点）是二轮 g3「别名闭环」的**升级版参考实现**——二轮裁的「字符串落池」应考虑
  升级为结构化别名记录；RoundRetry（同错指纹早停）与二轮 t1 同源，有现成代码可抄。
- **实锤缺陷**：allo 现有生长取消是**假取消**——`growth_body` 循环零检查点，取消后继续
  跑完并落库（仅 Err 分支改标 cancelled）。learnhub #103 长任务注册表 + 协作取消是修复
  蓝本（P0，行为缺陷非新功能）。
- **铁律沿用**：课程边界（结构课程内隔离、概念表跨课程、零关联）不变——learnhub 的
  挂载/修补同样严守「挂载无判定身份、结构↔概念经节点两跳」，同源无冲突。est 不进门禁
  不进调度；XP 记努力不记掌握；投影不落盘。
- **报告来源**：assets/ 三份深挖报告（r1 结构维护、r2 会话账本、r3 产品波），含逐提交
  机制、schema 原文、条款清单与 allo 对照。
- **工作方式**：g1–g4 HITL grilling 一次一票；t 票为设计定案（可 AFK）。

## Decisions so far

- [结构转活文档：挂载 + 修补 + 维护回合进多少？](tickets/g1-living-structure.md)
  （**推荐项默认裁决，可复议**）：**全量进**，实施两步走（先挂载后修补）；载体 = 挂载
  单行 JSON 表 + 修补审计表；findings 采纳评审 warn 通道（第一轮生长即留痕）；连带定案 =
  门位口径随二轮提示词波、RoundRetry 与二轮 t1 合并、ops 空 = 合法/skipped 零 LLM、
  手写 FNV-1a、显式命令触发、smoke 判据裁剪（渲染一致删除）。
- [learnhub 结构维护体系深挖](tickets/r1-structure-maintenance.md)：挂载 facts（FNV-1a
  hash 锚定 + 三态失效降级 + merge_mounts 幂等）、七型修补 ops（ops=意图引擎管自洽 +
  st.ops.apply 全量重过 + no_slug_churn）、维护回合（显式触发/RoundRetry/ops 空=合法）、
  findings 留痕（评审 warn 通道生产）、概念域重构（层收缩/别名携带定义/批内 new_aliases/
  登记单点四保证）、门位口径 v2.10、v2.8 术语纠偏；A/B/C 移植分级。
- [learnhub 会话与账本体系深挖](tickets/r2-sessions-ledger.md)：XP 三线结算纯函数包
  （settle.rs 340 行）+ 写侧条款 validate_settlement、xp.rs 读数投影（两把尺/ETA/84 格
  日历含零桶）、题级卡 allo 已有（learnhub 向 allo 对齐，ADR-0022 明引迁移 039）、缺
  推进前快照四字段/同日折叠/幂等键、会话体系全套（sessions 落盘首写为终/五命令/脱敏
  投影/统一会话页）、渲染管线（fail-visible + 模型产物徽标）。
- [learnhub #99 产品波深挖](tickets/r3-product-wave.md)：「现存且启用」口径（安排面收窄
  历史面不收/注册表滞后补立）、课程管理写面（改名不动宪章/归档=enabled 翻转/删除先目录
  后注册表 + 输入显示名强确认）、**allo 假取消实锤**（growth_body 零检查点）、长任务
  注册表全套（状态机/终态幂等/中性终态 envelope/协作取消 LLM 前后双检查/注册序判据/
  步骤叙事映射）、coach_view 五卡 + 预览式解析 + 禁语围栏（6 词零机制）、重出替换语义、
  R9 叙述声部 + 跨课预告废除。

## Not yet specified

- 三轮 + 二轮合并切票的粒度与顺序（等 g2–g4 裁决后切）。
- AI 批改轨题型是否入 XP 账、九题型当量表取值——随 t1。
- 会话体系若缓，复习中心聚合读数/检测记录读面的挂点——随 g4。

## Out of scope

- learnhub Rust 模块四层重组（app/engine/domain/infra）——allo 已有成熟 crate 布局。
- CI/pre-push 门禁、worktree 纪律等仓库基建。
- learnhub 空间 Git/文件空间载体件（mounting.yaml 文件形态、Git diff 即出处）——allo 用
  DB 表 + 审计表等价承载，形态重裁（见各票约束）。
- 联赛/streak 激励（learnhub 亦明确不抄）。

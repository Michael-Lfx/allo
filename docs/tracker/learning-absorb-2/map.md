# Map: 学习模块二轮吸收 learnhub——2026-10-12 以来的 22 提交

<!-- wayfinder:map -->

## Destination

一份锁定实施决策的二轮吸收规格：learnhub 在一轮实施期间新增的 22 个提交（#85 装箱教义与
提示词波、#88 量纲宣言、#86 内容成色纪律、#79+#60 命名门与重试机制）里，哪些进 allo、以
什么形态进（CONTEXT.md 词条、教练/内容提示词条款、门与机制迁移、量纲宣言范围），全部裁决
完毕，可直接拆实施票。

## Notes

- **前情**：一轮吸收（`docs/tracker/learning-absorb/`，已到达目的地）的实施已落地——
  i01–i08 + 评审轮共 16 提交在 `feat/learn0928`（**未推送**；`origin/main` 已前进）。本轮
  只吸收 learnhub 新提交的增量，不重开一轮已裁事项。
- **已覆盖免吸收**（r1/r2 对照结论）：铸名门名字面兜底（#79 同效已由 `ng.mints.collision`
  覆盖）、slug 批内互撞（allo 概念无 slug，批内唯一已覆盖）、评审「批内兄弟不算供给」
  （allo 判据②已对齐）。
- **铁律沿用**（一轮地图 Notes 与 CONTEXT.md 词条继续生效）：结构课程内隔离、概念表跨
  课程、两层零关联；投影不落盘；顺序权威 = 教练 + 账本；xyflow 不用（dagre + 自绘 SVG）。
- **常设偏好沿用**：每条生成管线配无头真机冒烟入口（learning-smoke 四子命令已在位）。
- **来源**：learnhub `C:\Users\test\Desktop\my\learnhub`（HEAD fb72157）；完整调研在
  assets/（r1 = learnhub 22 提交深挖，r2 = allo 实施后对应面盘点）。
- **工作方式**：g1–g4 为 HITL grilling（一次会话一票），t1 为 task（机制定案）；决议逐票
  写入票面 Resolution 并追加到下方索引。

## Decisions so far

- [learnhub 二轮增量深挖（22 提交）](tickets/r1-learnhub-round2-commits.md)：五组增量——
  P0 早停+容量守卫（allo 两循环都缺）、P1 别名闭环（三件套缺二）、P1 内容成色纪律（缺
  序列面）、P2 量纲宣言（三套无桥）；三条方法论（放宽配收窄/删预设少加规定/判据只看
  可见层）。
- [allo 实施后对应面盘点](tickets/r2-allo-post-impl-surfaces.md)：教练批已迁 json 接缝+
  ng.* 契约，缺早停与容量守卫；铸名门精确名撞已覆盖，缺别名闭环与 deprecated 口径；
  内容生成缺序列纪律；量纲三套无桥；冒烟四子命令已落地。

## Not yet specified

- 二轮实施切票粒度与顺序（等 g1–g4 全部裁决后，按依赖关系切成实施票——预计切票沿用
  `docs/tracker/learning-absorb/impl/` 的样式另开目录）。
- 内容成色纪律若采纳，是否需要内容侧冒烟读数面（g2 裁决后看）。
- est 字段更名与 070 迁移列的关系（g1 若采纳 (a)/(b) 时定）。
- 二轮实施完成后的推送/PR 时机（与一轮 16 提交一起处理——`feat/learn0928`）。

## Out of scope

- 一轮已裁事项的重开（课程边界铁律、罗盘/终点退役、结构化输出基建范围、教练批同期全迁）。
- learnhub 单机架构专属件（文件空间/keyring/MCP server）与内部清理（qb.formula.quoting、
  coach.rs 重命名等）。
- defnear/nearname 完整近邻裁决链（learnhub 既有机制非本窗口）——g3 若选引入则只做轻量
  代理，完整链不移植。
- 教练多 agent 化、传统课程接入结构管线（与一轮范围外一致）。

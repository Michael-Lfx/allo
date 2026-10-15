# Map: 学习模块二轮吸收 learnhub——2026-10-12 以来的 22 提交

<!-- wayfinder:map -->
<!-- 状态：已到达目的地（2026-10-13）——全部票关闭，二轮实施规格齐备，可拆实施票开工。
      注意：g4 为推荐项默认裁决（问题发出未获回答），票面已标注可复议。 -->
<!-- 后继：三轮吸收（learnhub #52 结构维护 / #87 会话账本 / #99 产品波，114 提交）见
      ../learning-absorb-3/map.md——其 g2 修订本 effort g3 的别名落池形态（字符串 →
      结构化记录），t1 是本 effort g1 的实现参考，t2 修复相关重试循环的取消语义。
      本 effort 决议（g1–g4/t1）未实施落码，与三轮合并切票。 -->
<!-- 实施波已切：impl/（j01–j10，二轮 + 三轮决议合并实施，依赖序见 impl/README.md）。 -->

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

- [重试循环机制定案](tickets/t1-retry-mechanics.md)：指纹 = 失败报告规范化 JSON，两循环
  （growth_body / structure_gen）同款早停（连败两次即停、不烧预算）；顺带引入「未点名
  被改」机械守卫（guard 豁免早停，配对「点修不是重写」）；重裁出口容量守卫（专用码
  `ng.batch.redraft_capacity` + 定向指令，guard=false）；越权原地重试恰一次不占轮预算；
  smoke 增 attempts_used/stop_reason 读数。
- [装箱教义与提示词波（#85）](tickets/g4-packing-doctrine.md)（**推荐项默认裁决，可复议**）：
  教义三条全采（道与术/概念不可学完/概念投影 → CONTEXT.md 词条 + COACH_SYSTEM 三条款）；
  提示词波全采域中立（弧线跨批投影/对照限同轴/动词多样谱/首批特则/同批不重教/悬空技巧
  与真实需要三问/定义文本前提闭包/评审禁则句）；COACH_SYSTEM 保持内联（一轮裁决不变）；
  三条方法论落 learning.zh.md 新节。
- [铸名门与命名面：别名闭环、deprecated 口径、近邻判定](tickets/g3-naming-face.md)：
  **复用通道全式**——近邻候选集装配（名字精确/近形/定义归一化+trigram/词面补位）+
  概念评审门扩权裁决（复用须指候选集内 ID，无新 LLM 调用，降级退化现状）；复用即提名名
  落池为活跃别名（字符串）；名字面唯一性不放松（撞名仍拒铸）；**deprecated 落库同口径
  过滤**（apply 两处查询补 deprecated=0）；池行 + 选名裁量条款随 g2 移交落地。
- [内容成色纪律与教学上下文投影（#86）](tickets/g2-content-discipline.md)：八条全采（域
  中立改写，零新增机械条款，R3 词表随 g3）；读侧投影 = 首现（祖先闭包、零新存储）+
  引用名开讲（`ref_name` 列小迁移），词表随 g3；通用纪律（R1/R4/R6/R7/R8）进两条管线、
  概念面条款与投影只进图节点路径；learning-smoke 增 **content 子命令**（读数面非门禁）。
- [量纲宣言采纳范围：est / XP / 分钟](tickets/g1-dimension-declaration.md)：**完整采纳**——
  立宣言 + 建账；**统一账**（学习结算入 companion XP，Lv 升级总努力级）；**三线一次到位**
  （教新按 est / 复习按工作量当量 / 题目按准确率分级）；**全式含惩罚**（分级授予 + 乱猜
  负分，需补检测）；**打卡并入 XP**（退役计数口径，日目标缺省 40；**不额外奖励 XP——
  只统一口径**，XP 全来自三线计价）；**代码/契约统一 est**（DB 列不动）。边界：est 不进门禁
  不进调度、XP 记努力不记掌握、读数纯投影。
- [learnhub 二轮增量深挖（22 提交）](tickets/r1-learnhub-round2-commits.md)：五组增量——
  P0 早停+容量守卫（allo 两循环都缺）、P1 别名闭环（三件套缺二）、P1 内容成色纪律（缺
  序列面）、P2 量纲宣言（三套无桥）；三条方法论（放宽配收窄/删预设少加规定/判据只看
  可见层）。
- [allo 实施后对应面盘点](tickets/r2-allo-post-impl-surfaces.md)：教练批已迁 json 接缝+
  ng.* 契约，缺早停与容量守卫；铸名门精确名撞已覆盖，缺别名闭环与 deprecated 口径；
  内容生成缺序列纪律；量纲三套无桥；冒烟四子命令已落地。

## Not yet specified

- 复习线/题目线的题型当量映射与作答去重口径（g1 已定方向，实施票定细节）。
- 乱猜检测的判据常量与报警阈值（g1 已裁采纳，实施票定）。
- 二轮实施切票粒度与顺序（等 g4/t1 裁决后切）。
- 二轮实施完成后的推送/PR 时机（与一轮 16 提交一起处理——`feat/learn0928`）。

## Out of scope

- 一轮已裁事项的重开（课程边界铁律、罗盘/终点退役、结构化输出基建范围、教练批同期全迁）。
- learnhub 单机架构专属件（文件空间/keyring/MCP server）与内部清理（qb.formula.quoting、
  coach.rs 重命名等）。
- defnear/nearname 完整近邻裁决链（learnhub 既有机制非本窗口）——g3 若选引入则只做轻量
  代理，完整链不移植。
- 教练多 agent 化、传统课程接入结构管线（与一轮范围外一致）。

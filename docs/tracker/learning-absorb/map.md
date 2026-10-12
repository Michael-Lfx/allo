# Map: 学习模块吸收 learnhub——先生成知识结构并可视化

<!-- wayfinder:map -->

## Destination

一份锁定全部决策的实施规格：allo 学习模块（`nomifun-learning` + `ui/renderer/pages/learning`）
把建课流程从「目标 → AI 提议终点 → 用户多终点选择 → 生长」改造为「目标 → 一次生成知识结构
（结构 + 前置地形）→ 可视化投影 → 生长」，终点锚体系命运、结构契约与存储、可视化方案、
结构化输出基建全部裁决完毕，可直接拆实施票开工。

## Notes

- **来源项目**：`C:\Users\test\Desktop\my\learnhub`（Tauri + 本地文件）。allo 的概念网生长模型
  （ADR-0009，commit fdd337072）本身就是 learnhub 的同源移植；本次吸收的是它**更新的成果**：
  普查废止、起点归概念侧、结构仅参考、生长求解器（ADR 0010→0012→0013→0015 裁决链）、
  JSON wire-only 契约（ADR-0014）、dagre+自绘 SVG 可视化。
- **铁律（learnhub 真机代价换来的，移植时不得重新引入）**：结构对生长不供内容、不做判定、
  不与概念建立任何连接；投影永不落盘；顺序权威 = 教练 + 账本；普查 / 交付账 / selectable
  过滤 / NoFrontier 终态均已退役。xyflow 已被 learnhub 验证弃用（边渲染缺陷），可视化用
  dagre 算坐标 + 自绘 SVG。
- **课程边界（用户 2026-10-12 强调，最高优先）**：知识结构**仅限课程内部生效、跨课程
  隔离**；概念登记表**跨课程**；两层**永不直接关联**——投影不 join 登记表、不做块→概念
  名字匹配、可视化不互渲染。已固化进 CONTEXT.md（Structure 词条）。
- **环境差异**：allo 是 axum + SQLite 多用户（web/桌面双宿主），learnhub 是单机文件为真相。
  SQLite 替代文件时：结构源仍整篇单字段快照存储，投影运行时重算。
- **工作区状态**：分支 `feat/learn0928`（HEAD bab7a4a8e）有 15 个文件未提交改动（growth
  governance 迭代：入口域盘点、行为摘要、R 软闸、深度水位）。动建课流程前先落库这批改动。
- **主要事实来源**：已结案 research 票「learnhub 知识结构生成链路深挖」与「allo 学习模块现状
  盘点」的 assets 报告。
- **工作方式**：本 effort 的裁决票多为 HITL grilling（涉及产品形态取舍），留给能代表用户的
  会话逐票解决；一次会话只解决一张票。
- **常设偏好（用户 2026-10-12 指示）**：每条生成管线必须附带 learnhub `smoke-run` 式的
  无头真机冒烟入口——与应用同接线、观测面存档 LLM 调用、机械判据自检（不过非零退出）。
  设计归票「真机冒烟入口设计」。

## Decisions so far

- [图投影只读 API](tickets/t1-graph-projection-api.md)（design facts 定案）：路由
  `GET /courses/{id}/graph/structure`（软删 404 守卫）；DTO 随学习模块惯例住
  `models.rs` + TS 镜像，`LearningStructureView` 200 恒答、None = 未生成；纯函数
  `build_structure_view` 住新模块 `structure_view.rs`（零 LLM 零时钟）；WS 事件
  `structure_started/completed/failed`；v1 无缓存；严格课程内、不 join 登记表。
- [知识结构可视化：图选型与落地形态](tickets/p1-visualization-prototype.md)（原型取消，
  直接裁决）：首版三图——思维导图 / 关系图谱（dagre LR，边方向反转 assumes=门）/ 学习
  路线（主干子图 + 传递闭包补前导），矩阵顺带、地铁图砍；数据源 = structure_json 投影
  结构块图，概念网图形化留后续；宿主 = 工作区新增「结构图」Tab；v1 不引 echarts；详情层
  非权威标注等 learnhub 形态机械采纳。
- [结构化输出基建：按 ADR-0014 建多少？](tickets/g4-structured-output-infra.md)：新基建
  覆盖全部生成面（structure/地形/教练批/概念评审）——**教练批同期全迁**（反推荐，用户
  明示；容错 deserializer 退役、重试对齐 ≤3 带条款报告、全量重验）；桥层新增 json 接缝
  （解析留 learning 侧）；provider 不支持 json_object 即报错（fail loud，不降级）；仅新
  管线提示词资产化（教练批契约迁移与提示词内联解耦）；analyze_scope 随之退役。
- [结构与生长的边界](tickets/g3-structure-vs-growth-boundary.md)：教练上下文重排为 9 块
  （结构参考材料全量块摊开取代范围参考、地形门口首批重点、覆盖读数改纯展示）；深度水位门
  退役、反虚标职能移概念评审门（difficulty 字段保留）；首级判据 learnhub 式重写；就绪即
  发布门与 R 软闸保留。
- [真机冒烟入口设计](tickets/s1-smoke-harness-design.md)（常设偏好，待设计）：参考
  learnhub 三冒烟 bin（smoke-run/smoke-json/smoke-loop），每条生成管线配无头冒烟入口 +
  观测存档 + 机械判据自检；设计待解。
- [结构生成契约与存储落点](tickets/g2-structure-contract-and-storage.md)：structure 契约
  全量引入 + 向导输入最小化（目标/程度声明/基础声明，后两项可选，goal_type/domain_type 由
  LLM 自判）；前置地形独立产物 + 门口对账，入口域盘点退役；双调用 + 整批重试 ≤3 带反馈；
  `learning_courses.structure_json` 单字段快照、重跑覆盖；不产人读文档（读侧投影承担）；
  新建条款化契约文档 `docs/contracts/learning-structure-artifacts.md`。
- [终点锚体系：删除、降级还是改造？](tickets/g1-endpoint-anchors-fate.md)：完全删除终点
  体系（表/提议端点/向导第 2 步/完成裁决/停摆臂第二臂/保护条款/UI/i18n）；存量破坏性
  DROP；生长无自动终态（只留空图臂，覆盖读数纯展示）；罗盘跟随终点退役。人读概览交给
  结构读侧投影，教练参考改结构全量块摊开。ADR-0010 已立，ADR-0009 决策 6 与 Amendment 1
  第 3、5 条废止。
- [learnhub 知识结构生成链路深挖](tickets/r1-learnhub-decomposition-research.md)：输入只有
  四项目标声明（无终点选择）；每轮 2 次 LLM 调用生成 structure + 前置地形，门口对账 +
  整批重试 ≤3 带反馈；契约 v2.4（8 标量 + spines 6–12 + blocks 40–80 + 三型边）；
  JSON Schema 单源 + wire-only 落盘；提示词为 md 文件资产 + 金样 + 围栏测试。
- [allo 学习模块现状盘点](tickets/r2-allo-learning-status-research.md)：学习图向导第 2 步
  就是多终点选择（`propose-endpoints` + `learning_course_endpoints` + 终点裁决/停摆臂）；
  可视化完全缺失（无图渲染、无边数据接口，罗盘只是 `<pre>` markdown）；概念网数据模型
  （registry + teaches/assumes×tier）足以支撑图投影；工作区有未提交改动。

## Not yet specified

- 建课向导的生成进度与结构可视化衔接形态——可视化宿主已定（结构图 Tab），剩进度事件
  与生成态展示的细节，实施时定。
- 概念网图形化投影（learning_lesson_concepts → 节点-边）——概念表 Tab 已覆盖跨课程
  概念面，结构块图上线后有需要再议。
- 传统课程（`course_kind='traditional'`）是否同享结构生成管线。
- blocks 40–80 规模对前端渲染性能与 WS 进度事件的影响。

## Out of scope

- learnhub 独立模块的整体移植：FSRS 复习调度（`review.rs`）、mastery 聚合、contentgen 课件、
  questions 出题、ledger 行为账本、MCP server、web_research——与「结构生成 + 可视化」无直接
  耦合，allo 已有对应或不需要。
- 概念身份体系重构为 ULID：allo 的 `learning_concept_registry`（canonical + aliases +
  definition）+ 铸名门已与 learnhub 概念模型语义等价。
- 教练多 agent 化（learnhub 自身亦后置为 v2 路线）。

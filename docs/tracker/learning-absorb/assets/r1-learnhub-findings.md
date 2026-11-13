# learnhub「知识结构生成 + 可视化 + 放弃多终点选择」链路调研报告

> 调研日期 2026-10-12；仓库 `C:\Users\test\Desktop\my\learnhub`（Rust+Tauri 后端 `src-tauri/`
> + React 前端 `src/`，FSD 分层）。由 Explore 子代理产出，为票
> [learnhub 知识结构生成链路深挖](../tickets/r1-learnhub-decomposition-research.md) 的结案资产。

## 1. 知识结构生成（decomposition）流程

### 1.1 输入：用户给什么、起点怎么定

输入是 `CourseInput` 四项（`src-tauri\src\decomposition\facade.rs:34-43`）：
- `goal` 目标原句（不得改写）
- `goal_type`：capability（能力）| coverage（覆盖）
- `proficiency_statement` 程度声明（达成后能独立做成什么）
- `learner_basis: Option<String>` 学习者基础声明（缺省 = 按纯零基础画地形）

**关键：没有任何"起点/终点选择"输入**。用户只给一句目标，起点与终点完全交由 LLM 一次性
生成整个结构（主干 6–12、块 40–80/硬上限 100），零基础衔接交给独立产物"前置分解"
（门口 G1..Gn 判据）。"多终点选择"不是被做成选项，而是**根本不存在终点概念**——结构只是
参考地图，学习顺序权威归账本+教练（ADR-0002）。

### 1.2 是否已放弃"多终点/多入口选择"

演进路径非常清晰，移植时应直接取终态：
- **普查（census）已废止**（ADR-0012）：概念唯一出生路径 = 节点生长批铸名，分解管线
  "不铸造、不记账"，普查条目与登记表字段级同形是违例根。
- **结构仅参考**（ADR-0013）："块表、知识结构网、地形、及其一切读侧投影对生长**不供内容、
  不做判定、不与概念建立任何连接**"；任何「结构 → 概念」连接（中文名匹配、候选行网边段）
  即违例。
- **多入口反而是正解**：ADR-0015 最终修正（2026-10-12）——"横跳（多主题交错并行）本身是
  教学正解……病只在不跳步"；FIFO 链队列、交付账、`NoFrontier` 终态、selectable 过滤
  **全部退役**，结构源是"地图不是队列"。
- 快速路线 = 读侧投影："速成/系统等差异化路线 = 读侧按需投影（从结构源过滤 + 自动补前导），
  不落盘、非权威"（契约 §1.4；`src\pages\course-detail\ui\structure\route-map.tsx` 即此实现）。

### 1.3 LLM 交互轮次与门面

`Decomposer::run`（`src-tauri\src\decomposition\facade.rs:113-172`），整批语义：
1. **每轮恰好 2 次 LLM 调用，串行**：① `GoalGenerator`（生成 structure 模型面 JSON）→
   ② `TerrainGenerator`（生成前置分解模型面 JSON）。
2. **门口对账** `check_door_references`（facade.rs:245-263）：structure 的
   `domain.prerequisite_doors` 必须都能在前置分解的门口定义里解析，不得自定义。
3. **整批重试带反馈**：`MAX_ATTEMPTS = 3`；任一产物失败，两份**一起**重新生成，且把逐条款
   违规报告注入重试装配（`feedback = "上一轮产物违反契约,逐条修正后重新生成:\n{reason}"`，
   facade.rs:160-163）。契约 v2.1 §0 明言：无反馈盲目重试零收敛（#53 实证：三次重试 33 项
   同型违规）。
4. 全部成功才落盘（暂存目录整体 rename，重跑同目标 = 新课并列）；耗尽则磁盘无半成品。
5. 每次调用（成功/失败）经 `ArchivingExecutor` 存档到观测目录 `<obs>/llm-calls/`
   （prompt/schema/response 三件）。

### 1.4 提示词：文件资产 + 零模板引擎（不是 minijinja）

- 模板是仓库内 md 文件，`include_str!` 编译期嵌入：`src-tauri\prompts\decomposition\`
  goal-decomposition.md、prerequisite-terrain.md、mint-verdict.md（金样快照在 `golden/`
  子目录）。
- 装配是**头注剥离 + `{{具名占位符}}` 单遍纯文本替换**（`src-tauri\src\decomposition\prompts.rs:1-13`），
  占位符与注册表双向互锁（测试+运行时双闸）；温度/thinking 档位住 `PromptSpec` 注册表
  不进文本。goal-decomposition：temperature 0.2、thinking=true；mint-verdict：
  temperature 0.0、thinking=false。
- 模板头注五字段（id/source/scope/notes/review）承载开发面知识；"围栏词表"（prompts.rs:48-120）
  机械拦截开发面词汇（如 `gd.`、`ADR-`、`LlmExecutor`）出现在模型面正文——ADR-0004 读者面纪律。
- 提示词要点（goal-decomposition.md 原文）：
  - "八个标量键全必填……domain_type（你在四种领域型里自判，只允许填 **理论 / 工程 / 历史 /
    技能** 四字之一）"
  - "blocks……每条只允许 name、slug、spine、stage、note 五个键——definition、operation、
    example、pitfall、source 等判据形字段一律禁止（块不是概念，不进概念表）"
  - "assumes 的 source = 需求方（要学的一方）、target = 前置（先有的那一方）……不要写反"
  - "入门档从零基础可直接上手的具体对象起步……抽象语言与记号体系不得占据入门档第一梯队"
  - "门口编号只能引用前置分解产出的门口，在下方输入里没有给门口编号时写 `[G1]` 占位并靠
    系统对账"
  - "全文不得出现排期语词"

### 1.5 输出 artifact schema（structure.yaml）

契约 `docs\contracts\decomposition-artifacts.md`（v2.4），模型面 JSON、落盘 YAML 由代码
序列化。核心：
- 8 个标量键：goal / goal_type / proficiency_statement / domain_type / form_reference /
  source_tier / generated_by / disclaimer
- `domain`：core_definition + includes/excludes 各 ≥3 + prerequisite_doors（引用 G 编号）+
  audience + scenarios 2–3 + granularity
- `spines` 6–12 条：name/definition/key_points≥1/instance（可验证实例）；可 `thin_spine: true`
  + reason 豁免每干≥5 块配额
- `blocks` 40–80（80<n≤100 告警，>100 红）：五键 name/slug/spine/stage（入门|进阶|高级）/
  note；一物一名（禁并列连词，`「async/await」` 括注豁免）
- `edges` 三型：assumes（source=需求方，target=前置）/ composes / contrasts；
  tier ∈ know/use/teach；端点解析域 = 块表名集；同三元组唯一；weight 废
- `gaps` ≥3、`quality` ≥5（unmet 必带 reason）
- 硬禁：全文不得出现排期语词（gd.no_schedule）

代码侧常量单源：`src-tauri\src\decomposition\structure.rs:26-72`（TOP_KEYS/BLOCK_KEYS/
STAGES/EDGE_TYPES/TIERS/DOMAIN_TYPES/量级档常量）；读侧派生（名集/主干分组/矩阵格/依赖分层
L0=无前置）也在本模块，"零 LLM、零 IO、零时钟"。

### 1.6 JSON 契约与修复轮（ADR-0014 架构，移植重点）

- `src-tauri\src\json_contract.rs`：三件公共底座——① **schema 单源**（`schema_goal_structure()`
  等，词表/键集直接引用 validator 常量，schema↔校验器由互钉测试锁定）；② validate/normalize
  分离（`normalize_latex_fields`：LaTeX 反双重转义启发式，只对非 JSON 转义字母
  `\\alpha`→`\alpha`）；③ **wire-only 落盘**：`yaml_value()` 把模型 JSON 无损折成 YAML 值 →
  校验 → `serde_norway::to_string` 代码落盘——落盘 YAML 从此是代码产物，"解析风险归零"。
- **为什么迁 JSON**（ADR-0014）：YAML 提示词纪律 → parse_repair 修复链 → validator 的老路
  在引号/块标量/块序列三类真机失败上持续膨胀；现厂商（DeepSeek/GLM）只有 json_object 无
  json_schema，故"合法 JSON 由解码器保证 + 结构合规本地 JSON Schema 单源执法"；语义
  validator（条数下限/端点解析/覆盖门）永不退役。**KaTeX 长文不走 JSON**（content-write
  保持受限 Markdown 分面）：`\frac` 的 `\f` 是合法 JSON 转义，"产物合法但内容已坏"。
- `src-tauri\src\parse_repair.rs`：OpenMAIC 五级修复链（原样→剥 `<think>` 前缀→剥代码围栏→
  双组合→失败带行号+120 字符窗口日志），**现役仅 content-write 长文消费**，结构化面已全部
  退役此链——移植 axum 项目时结构化面可直接走 json_object+本地 schema，不必带修复链。

## 2. 起点与结构的关系：核心 ADR 摘要

| ADR | 决策 | 理由 | 否决的替代 |
|---|---|---|---|
| **0013** 起点判定归概念侧 | 结构仅参考：不供内容/不做判定/不与概念连接；铸名唯一触发=节点 `teaches`；assumes 只引用已铸概念；"当时可学"整体住概念侧（空表⇒首批 assumes 机械必空）；基线不落判定面；候选行只留结构字段+「无前置」结构布尔 | 真机首测把结构条目按中文名匹配到已教概念集，是"结构→概念"连接的残留；选料器读反边表方向 | 结构入选料池/结构下延零基础/地形块入池；基线进宪章概念清单；保留 `ng.assumes.net_faithful` |
| **0012** 普查废止 | 概念唯一出生路径=生长批铸名门，登记表空表起步；分解只立课+两产物；前置地形明示保留 | 普查六件套与登记表字段级同形="结构长得像概念表"的违例根；普查没实际用过 | 方案 B（普查降草稿/激活态）：同形违例存续；"改名换字段保留"中间态 |
| **0003** 结构即原料 | 结构产物对教练可取用（拉取式优先）但无权威；顺序权威唯一=账本+教练；教练职责=局部最优下一步；非权威标注收敛到**详情层** | "动态性不住在『无结构』，住在『权威归属』"；五仓库调研全部向教练注入背景，硬隔离是孤例 | 维持硬隔离（上下文变薄）；推注式注入（锚定/懒惰风险）；把原料读成排期；教练多 agent 化 |
| **0010** 概念词典与课程结构分离 | 结构（宏观、概念无关）与概念表（原子微观、跨课程）无关联；概念需求驱动创建，兼作建课防跳步结构件（引用不可解析=Broken，fail loud）；概念依赖=挂接网读侧投影永不落盘；别名禁作深度/粒度/掌握度轴 | 开发 agent 曾把课程内结构当概念候选源；挂接是课程内结构非节点依赖 | 收敛率混算；登记表存关系字段；别名当粒度轴；投机预铸 |
| **0002** 两层面铁律 | 存储面只存：概念登记表+结构文档快照；**永不存储**概念间关系/树/矩阵/分层/路线/scenarios；投影重算即得；顺序权威=账本 | KSB-YAML 先例"单源数据+解析期重算"可行优雅；显影剂可整体退役是"帮忙不殖民"的结构保证 | 存储式树层级/路线/scenarios；概念网关系入登记表；自动退役/重画 |
| **0015** 生长求解器 | 结构源是地图不是队列；solver 三次收束：链式确定性→安全横跳→**参考材料全量摊开**；selectable 过滤切除、NoFrontier 废除、交付账退役；防跳步铁底=概念侧两层门；原子化修正：节点必填 est（10–60 分钟）、多回合管线 | 用户底线原话："项目中不要有什么前置的规定结构约定，来限制教练接下来的操作……这些只能是参考信息"；插件实证横跳是正解 | 结构键排序缓解方案；一步到位 LLM 教练回合（v2 后置） |
| **0001** 概念身份 | 概念=ULID，别名聚合；概念层（id/definition 唯一主判据/def_src/operation/example/pitfall/deprecated/superseded_by）与别名层（name/lang/kind/attestation/display_priority/status） | ULID 消解改名=全库 rekey；判据单一性 | definition 下放别名层；"名字即身份"残余 |
| **0006** 一物一名（已被 0012 取代，判据由铸名门承接） | 一条=一个可独立教学概念；名字禁并列连词；逃生门=官方术语「」包裹 | 捆扎条目"教会了一半"无法记账；金样 58 条七成是捆扎 | 机械禁字符不留逃生门；校验器执法语义原子性 |
| **0009** 生长式 | 每批 1–5 就绪节点；普查=初始地图非覆盖义务；外包批处理管线；零人审；失败=整批原子 | "普查全量≠够用，没有盲区是不可能的"；生长把赌注单元缩到批，错得起 | 一次成型全量节点；教练逐批亲产+门禁机器 |
| **0011** 挂接对齐 | tier-blind（档位不进门禁）；覆盖来源=批前当前图；引用别名（展示面）；**语义评审门**（确定性门后唯一语义兜底，审计失败不杀人） | 档位是无法从结构验证的声明，做门禁是精度剧场 | 档位比较门禁；批内互覆；评审器故障即批失败 |

## 3. 前端可视化

**重要事实纠正：`@xyflow/react` 在 package.json 中但 src 下零 import**。唯一出现处是注释：
`src\pages\course-detail\ui\structure\terrain-dag.tsx:20`——"地形能力块 DAG（自绘 SVG +
dagre LR；**xyflow/echarts 边渲染缺陷见票 #27 findings**）"。即项目实际弃用了 xyflow，采用
**dagre 算坐标 + 自绘 SVG 画边 + HTML button 画节点**；力导图用 echarts（按需动态 import）。

五图容器 `structure-graphs.tsx`（tab 切换，页脚硬标注"投影非权威；层号=依赖深度，不是排期；
学习顺序权威=账本+教练"），全部在 `src\pages\course-detail\ui\structure\`：

| 图 | 文件 | 实现 |
|---|---|---|
| 思维导图 | `mindmap.tsx` | 纯自绘 SVG，主题居中、主干按块数降序分左右两翼配平；主干分组配色 `palette.ts` 与图谱/路线共用一源 |
| 矩阵 | `matrix-view.tsx` | 主干×阶段二维阵，读侧重算 |
| 关系图谱 | `concept-net.tsx` | 双布局：force=echarts graph（repulsion 200/edgeLength 70-140/roam/draggable/click 选详情）；layered=dagre `rankdir:"LR", nodesep:12, ranksep:60` + SVG line + 绝对定位按钮节点；**边方向反转**展示（assumes=门）；枢纽 Top10（assumes+composes 出入度）琥珀描边；按主干隐藏、按关系过滤；指标条（节点/边/平均度/孤岛）；未收录端点渲染为虚线菱形占位节点（`◇` 前缀） |
| 学习路线 | `route-map.tsx` | dagre LR，列=assumes 前置链深度；选主干=子图+**传递闭包自动补前导**（虚线框+"前导"角标，已访问集防环）——"放弃多终点选择"后参考路线的读侧投影形态 |
| 线路图（地铁） | `metro-map.tsx` | "站序是参考，不是行驶时刻表" |
| 详情面板 | `concept-detail.tsx:104` | 点击节点弹出：五键结构位+关联关系+固定文案"**由课程内容自动整理，仅供参考；学习顺序以教练实际安排为准**"（ADR-0003 后记：非权威标注收敛至详情层） |
| 地形 DAG | `terrain-dag.tsx` | 前置分解能力块 DAG，同款自绘方案 |

**投影与存储分离在前端的体现**：前端消费 `export_course_view` Tauri 命令 →
`build_structured_view`（`src-tauri\src\decomposition\export.rs:188`，纯函数零 IO 零 LLM）→
`StructuredView{course, structure, terrain}` JSON（specta 导出 TS 绑定）。export.rs 头注：
"structure.yaml 是唯一落盘真相，本模块只是其读侧投影……永不写第二份真相文件"；BlockView
预计算 `no_prereq` 布尔（不做任何 assumes 边的源）。

## 4. 节点生长（node_growth）

契约 `docs\contracts\node-growth-artifacts.md`（v1.9）+ ADR-0009/0011/0015。

**生长批**：一次调用 = 1–5 个就绪学习节点（`LearningNode{slug,node,est,teaches,assumes,opt}`）
+ 批内新概念提名（`new_concepts` 七字段）；批文档是模型面过渡物**永不落盘**，落盘事实只有
`graph.yaml` 追加节点+铸名门新概念。

**管线**（`src-tauri\src\node_growth\facade.rs` 头注）：
1. **结构参考材料组装**（`reference.rs`，ADR-0015 底线修正）：**全量块摊开**，呈现序
   （难度档，前置链深，主干序，块序），行格式 `- 块名(slug)|主题一句|难度档|主干|无前置`
   或 `|前置:A、B`——纯结构事实，不裁剪候选面、不限批范围、无交付账；教练可围绕参考装箱
   也可自拟。
2. LLM 装箱（`prompts\node-growth\node-growth.md`：参考非限制/不跳步唯一硬线/est 15-30
   目标 60 硬顶/禁双事合装/覆盖律=assumes 只准引用批前已教/零基础判据"要么本节点教它要么
   是常识，简单≠常识"）。
3. 契约校验门（ng.* 全条款，`validator.rs`；ng.parse 已退役=JSON 前置保证）。
4. **铸名门**：definition 归一化撞名=复用改挂；每条提名恒送 stage 2 语义裁决（近邻集=名字
   精确占用+近形+判据近邻+词面 top-k=3 补位，零分亦入）；裁决不可用=整批可重试失败不降级
   直铸；**新铸 slug 撞名=拒铸整批重试**（v2.4，真机事故：两条真概念共用 slug 铸出双记录）。
5. 挂接解析成 ULID（引用别名 `ref` 记录展示用习惯名）。
6. **语义评审门**（`reviewer.rs`）：判据四面=前提完备性/名实相符/定义质量/档位合理性
   （v1.9 加 atomicity/est 拆分建议）；问题四件套（node/field/issue/advice）注入重试；
   评审器自身失败重试一次后保守放行+warn。
7. 整批原子落位（`placement.rs` 读改写整文件原子替换）。

**教练边界**（ADR-0009/0015）：教练只消费不生产；批量=就绪存量缺口（`coach::READY_TARGET`）；
`coach.rs` 是"零 LLM 决策"的确定性编排面（候选排序/约束过滤/下一步解析全部代码），"任何
『问模型下一步』都违反评估公理"。

**多终点结论**：生长永远可尝试（无前沿耗尽终态），多入口并行开链、跨主干横跳合法，唯一
铁线=不跳步，铁底在概念侧（装箱覆盖律+批内自足+语义评审+铸名门），结构侧零安全裁量。

## 5. 概念身份体系（concepts）

`src-tauri\src\concepts\`（model/store/minter/defnear/nearname/verdict）：
- **模型**（`model.rs:57-79`）：`ConceptRecord{id(ULID), definition(唯一主判据), def_src(ai|human),
  operation, example, pitfall, deprecated, superseded_by, display, aliases}`；
  `AliasRecord{name, lang, kind(canonical|translation|abbreviation|former|colloquial),
  attestation, display_priority, status}`；展示名=display 指针，缺席=首别名。
- **存储**（`store.rs`）：文件为真相，每概念一个 `<slug>-<ulid>.md`（frontmatter YAML）；
  登记表不存概念间关系。
- **铸名门判据件**：`defnear.rs`（定义归一化+字符 trigram Jaccard 近邻）、`nearname.rs`
  （名字近形互包含）、`verdict.rs`（裁决上下文装配+结构化裁决 JSON 核验：复用 id 必须来自
  近邻 id 集，禁编造）、`minter.rs`（机械落位段）。

## 6. 其他模块清单（移植相关性）

| 模块 | 职责一句话 | 与"结构生成+可视化"耦合度 |
|---|---|---|
| `courses.rs`/`course_domain.rs` | 课程域存储与命令面单一收口 | **强耦合**（结构产物落位/读回） |
| `coach.rs`/`coach_log.rs` | 教练最小内核：零 LLM 确定性编排+propose 审计流水 | 中（消费结构参考） |
| `ledger/` | 学习者行为流水账（JSONL 追加只增） | 独立 |
| `review.rs` | FSRS 间隔复习调度 | 独立 |
| `mastery/` | 掌握度聚合（账本回放投影） | 独立 |
| `contentgen/` | 按载体契约为学习点生成分步课件 | 弱（消费节点不消费结构） |
| `questions/` | 出题+判分+XP 结算 | 独立 |
| `readside.rs` | 只读命令：课程图投影/跨课程就绪聚合/登记表单条读 | 中（挂接网投影） |
| `node_ctx.rs` | 学习点教学上下文读侧 | 中 |
| `llm.rs` | LlmExecutor 接缝 trait（DeepSeek/FakeLlm/ArchivingExecutor） | **移植必备** |
| `fsx.rs` | 原子写纪律 | 移植必备 |
| `learner_state.rs` | 两态自报 | 独立 |
| `web_research.rs` | bigmodel 双 MCP 适配 | 独立 |
| `mcp_server.rs`/`bin/` | stdio MCP server + 冒烟 bin | 独立 |
| `settings.rs` | 应用设置 | 独立 |

## 7. 移植到 Rust+axum+SQLite+React 的要点

1. 可直接平移：契约文档先行 → schema 单源（json_contract）+ 条款化 validator（gd.*/ng.*
   编号即稳定机器标识）+ 整批重试带反馈（MAX_ATTEMPTS=3）+ wire-only 落盘 + prompt 文件
   资产/金样/围栏测试矩阵 + LlmExecutor 接缝 + 观测面存档。SQLite 替代"文件为真相"时注意：
   结构源仍应整篇单字段存储（快照语义），投影永远运行时重算，勿建关系表存概念间边。
2. 可视化直接抄方案：dagre 布局 + SVG 连线 + 详情层非权威标注；不必引入 xyflow。
3. 最核心的架构裁决链条（ADR-0010→0012→0013→0015）是"放弃多终点选择"的完整推理：结构=
   宏观参考地图（概念无关、永不判定）、概念=微观身份（唯一出生路径=teaches 铸名）、顺序
   权威=账本+教练、防跳步铁底=概念侧两层门。移植时不要重新引入普查、交付账、selectable
   过滤——它们都是该仓已用真机代价否决过的形态。

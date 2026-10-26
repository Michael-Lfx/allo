# Map: 五轮吸收——B 组产品线 + learnhub 定义接地新线（ab908e0）

<!-- wayfinder:map -->
<!-- 状态：grill-with-docs 两轮定案完成（2026-10-26）。m4/m6/m8 三张 grilling
      票全部关闭并切实施票；前沿 = m1/m2/m3/m5/m7/m9/m10/m11。三张 grilling
      的裁决细节在各票 Resolution；术语正名（已会声明/定义用语表）已进
      CONTEXT.md。 -->
<!-- 前情：四轮 k01–k07 全部关闭（feat/learn-absorb-4，tip e3106eec6，迁移 070 已落，
      未推送）。本波吸收 learnhub B 组五线 + 1037086..ab908e0 新 18 提交（定义接地线
      #169/#171/#173/#176 + #175 收敛梯子三断层 + smoke-basis-flow）。 -->
<!-- 来源：learnhub C:\Users\test\Desktop\my\learnhub（tip ab908e0），参考实码逐票引用。
      两仓现状深挖（2026-10-26 双代理对账）结论已织入各票「allo 现状」。 -->

## Destination

B 组产品线域中立移植：基础声明通道（勾选即已会、只铸概念不落节点）、门口梯队
door_refs 注解、评审挂号表 term_provenance 程序化、定义接地二阶（词表/审计）、
生长收敛梯子加固、自动补批、学习任务资源闸、铸名门外部证据。每票带双臂真机
冒烟与前后比对（总纲同四轮）。

## Notes

### 两仓对账关键事实（2026-10-26 深挖，票面锚点以此为准）

- **ADR-0031 门口物化已退役**：learnhub e06b3cb 删除物化模块，ADR-0032（基础声明
  =只铸概念不落节点）取代。本波**不吸收物化线**，只吸收其终态继任者。
- **allo 地形门口已是梯队形态**：`TerrainView.doors` 4-8 条 G-id（criterion +
  kind hard|self_check，`models.rs:1769-1782`）——#165 对 allo 的增量 =
  door_refs 块注解面 + 过门自检消费，不是梯队本身。
- **allo 生长 = 评审先行 + 单事务原子落库**（growth_body :1783 评审 → :2107
  apply_batch，:2723-2932 单事务）：#128 的「铸名轮级暂记回滚」痛点 allo 结构上
  不存在；#128/#129 残差 + #175 三断层（m5）才是真缺口。
- **allo 三个结构性差异**（决定 m2/m6/m8 的裁决空间）：
  ① 无 mint-log 铸造账（声明铸造出处由声明锚表本身承载）；
  ② `LearningCompleter` 纯文本补全（`completer.rs:17-37`），无工具轮缝——
  #168 按需查询是基建级缺口，入雾区；
  ③ 无 start_session 教练会话面——#121 的第二触发点需域中立重设计（m6 盘问）。
- **k 波已落接口**（新票避碰）：七个 `NOMI_LEARNING_*` 开关（switches.rs 集中
  解析）；smoke 档位 xp/quiz/content/course/mount/repair/maintenance/structure/
  json/grow/loop；`stem_similarity`（defnear.rs:147，0.7）；k07 判据① V2 子查
  （CRITERION_ONE_V2，mod.rs:720-727，NOMI_LEARNING_REVIEW_V2 开关）。

### 验证总纲（沿用四轮 k 波纪律）

1. 开关臂：新行为面配 `NOMI_LEARNING_<FACE>=off`（switches.rs 先例），同构建
   同凭据同 fixture 双跑；纯资产面用版本臂（旧 tip vs 新 tip，独立构建目录）。
2. 硬判据进 smoke EXIT；效果判据产出**对照卡**（两臂读数并排 + 人读清单）——
   **两臂必须都跑，只跑一臂不算关票**。
3. 既有审计计数器与既有 smoke 判据 on 臂不得劣化；观测存档
   `observation/learning-smoke/run-*/calls.jsonl`。
4. learnhub 参考实码先读铁律（每票 Reference 表）；迁移从 **071** 起（070 已占）；
   DB 迁移高危区纪律（append-only + 测试 + v3 契约注册同步）。
5. 真机纪律：NOMI_CHANNEL=dev 显式带、跑前核 nomicore.exe mtime、--data-dir
   放子命令前。

### 雾区（Not yet specified）

- **#168 装箱面按需查询（lookup_concepts/concept_status 工具轮）**：需要
  `complete_json_with_tools` 基建（allo completer 无工具缝，learnhub
  MAX_TOOL_TURNS=8 + rig agent）。触发条件 = m2 落地后概念面注入（现 coach
  上下文块 9 登记表节选，已封顶）被证实不够用，或声明集需要按需查询时开票。
- **ConceptsDoc 回放比较器（1037086）**：依赖 C 组回放套件，维持不吸收。
- **mint-log 铸造账全量移植**：m2 只用声明锚表承载出处；生长铸名的全量铸造账
  另议（现无消费方）。
- **自动维护回合 GUI 按钮**：k02 雾区维持。
- **提案者代理（m4 裁掉）**：缺口名词自动提名提案。触发 = 审计（m9）标记数
  大且人工提名负担重；learnhub 自评零弃权过度自信，只配「提案 + 人工确认」。
- **根类别集（m4 默认裁决，可复议）**：B 路线（定义闭包）终局件，learnhub
  亦未落地于三层用语表。触发 = 审计显示缺口碎片大量因缺属词误报。
- **分解侧课程参照搜索复用（m8 雾区）**：结构生成的搜索增强。触发 = m11
  「变好才许收」通过后再议。
- **FragmentVerdict enforce 阻断（m4/m9）**：缺口碎片阻断生长。触发 = 定义
  用语表经真机审计运行成熟后开开关（`NOMI_LEARNING_LEXICON_ENFORCE`）。

## Tickets

| 票 | 标题 | 依赖 |
|---|---|---|
| m1 | 门口梯队 door_refs 块注解 + 过门自检读侧 | 无 |
| m2 | 已会声明通道——勾选即已会、只铸概念不落节点（ADR-0032 移植） | m1 |
| m3 | 评审挂号表 term_provenance 程序化 + 定义原子性条款（#169/#176） | 无 |
| m4 | 定义接地二阶定案——定义用语表/只读定义审计/提案者（#171/#173 盘问） | 无（建议 m3 后） |
| m5 | 生长收敛梯子对账加固——有状态复审 + #175 三断层 | 无 |
| m6 | 自动补批定案——生长作为调度动作（#121/ADR-0024 域中立） | 无 |
| m7 | 学习任务资源闸——并发上限 + 排队 + 取消窗口（#144/#145 缩编） | 无 |
| m8 | 铸名门外部通行名证据定案（#151/ADR-0030 盘问） | 无 |
| m9 | 定义用语表与只读审计落地——base 语料移植 + 审计纯函数 + 端点与标记 | m4（定案）；碎片面在 m3 后接 |
| m10 | 自动补批落地——verdict 显式化 + 工作区加载触发 + 开关与重入集 | m6（定案）、m7 |
| m11 | 铸名门外部证据落地——flowy-web 直依赖 + 扩邻 + 证据块（fail-open） | m8（定案） |

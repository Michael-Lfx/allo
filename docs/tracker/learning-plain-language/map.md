# Map: 学习模块提示口语化——去开发用语、收错误直泄、降呈现密度（o1–o6）

<!-- wayfinder:map -->
<!-- 状态：立图（2026-11-11）。来源 = 用户判定「学习模块页面暴露提示太多：
      展示方式不一定合适 + 大量开发用语普通用户看不懂，需要口语化」。两路
      只读探查（词典面 J1–J4/P1 共 55 条 + 组件面五类 30 条）证据齐备后
      分组立票。**施工开动条件 = learning-structure-progress-face 波（v1–v4）
      关票并回 feat/learn0928 之后**（审计时 v 波未并回，条目 26/5 等呈现
      现状已被 v2/v3 改写，施工时先按并回后现状复核行号与文案面）。 -->
<!-- 基线：feat/learn0928 尖端 06091fd45（审计时点）；施工基线 = v 波并回后
      的 learn0928 新尖端。 -->

## 用户裁决（2026-11-11）

1. **两个问题定案**：展示方式不一定合适（呈现密度/位置/方式）+ 开发用语
   需口语化。范围 = 学习模块页面（词典 + 组件层 + 后端文本的消费面）。
2. **施工排程**：本波立票即开工前置条件为 v 波关票并回；真机目验与全模块
   欠账合并。
3. **保留产品隐喻清单**（探查判定 + 用户默认）：「生长」「教练」「知识块
   （块）」「XP」「复习队列」「记忆编程」「参考地图」——这些已成长期语汇，
   口语化不碰；**必改清单**：「门口/主干/铸出/投影/留痕/幂等/补挂/快照/
   分诊/审计（作步骤名时）/挂载（用户侧）/资源槽/读侧/端点/锚点/兜底」。

## 缺口 → 票映射（证据：两路探查报告，2026-11-11）

| 缺口 | 规模 | 归票 |
| --- | --- | --- |
| E1 错误直泄根因：`BackendHttpError.message` 构造成 `` Backend POST {path} failed (409): {JSON.stringify(body)} ``，`errorMessage(t, …)` 对 Error 原样返回——学习模块约 **25 个消费点**（toast/Modal/面板）把英文+JSON 原文弹给用户；5 个 `Modal.error` 强制弹窗直泄；声明窗口首屏拼 `structureError`（契约门报文 `[gd.terrain.keys]…` 可整句上屏） | 1 根因 + ~25 点 | **o1** |
| E2 已会声明区开发用语重灾区：「铸出/登记表/幂等增量/门口判据/读侧参考/通道未开启」成对出现（词典 J1/J2/J3 集中区） | ~12 key | **o2** |
| E3 建课与进度流程用语：「分诊/审计（步骤名）/资源槽/幂等/快照兜底/挂载/修补留痕/grounding」+ `genTriageLine` 一行五段箭头式 | ~15 key | **o3** |
| E4 结构图术语与机器直出：「主干/门口/前置（门）/投影非权威/未收录端点/依赖层 L{n}」+ `door.id` Tag 直出、`basis.doors` id 串、role 枚举 `[${role}]`、裸 UUID 回退、topic 前缀回退 | ~14 key + 6 直出点 | **o4** |
| E5 设置页与遗留文案：「留痕/补挂/回合/环境开关」三连、wideContentHint 像素值行、fsrs 算法标签、deleteCourse「安排面」、capacityFeedback 双重否定链、reviewManage「剔除」 | ~10 key | **o5** |
| E6 呈现密度与信息架构：工作区顶条三 processing Tag 并排、首页顶区 error Alert + 永久 `packContract` 架构词 Alert + Banner 堆叠、GateCoverage 卡双挂载（学习 Tab + 结构图 Tab）、StructureGraphTab 观察列表平铺、建课完成视图 5 组 Collapse | 5 重灾区 | **o6** |

## 影响面核查关键事实（2026-11-11 探查核实）

- **根因向量形状**：`ui/src/common/adapter/httpBridge.ts:252-256` 构造
  `BackendHttpError.message`；`pages/learning/utils.ts:123-125`
  `errorMessage(t, error)` 对 Error 返回 `error.message` 原文。409 生成冲突
  已有友好词典（d7 `learningGraphStructureConflict`/`inflightTaskToast`
  措辞达标，**零改动保留**）；其余 status/code 无映射。
- **o1 改造面**：`utils.ts errorMessage` 单源改造（status/code → 词典、
  未知 → 通用句、原文仅入 console）即可覆盖全部走 `errorMessage` 的消费点；
  另有 ~8 处**绕过点**需收编（`Modal.error({ content: errorMessage(…) })`
  属强弹窗需降级或映射、LessonStudy/QuestionDialogs 的 `error.message`
  直用、GrowthTaskPill failed detail 直出、useCourseCreation 两处、系统
  动作簿 `row.reason` 直出）。**前置核查**：`BackendHttpError` 是否暴露
  结构化 `status/body` 字段（httpBridge 构造处必有原始值）；若需给 common
  层加只读字段（additive、低冲突）允许，**除此之外 common 层零改动**；
  施工前先读 `scripts/check-error-surface.mjs` 契约（改错误面可能撞闸）。
- **词典面规模**：zh/en learning.json 各 660 行，en-US 与 zh 同病（直译
  保留 dev 语：idempotent/triage/gates/snapshot/projection…）——所有词典
  票双语同步改 + 断言（红线 2）。
- **与 v 波的交界**：审计时点早于 v 波并回——`structureBadgeRunning`
  （v2 新徽章文案「结构生成中 · 第 {attempt}/3 次 · 已 mm:ss」）与
  `genBgStructureRunning[Elapsed|Attempt]`（v3 Pill 文案）已按口语化标准
  书写，本波不回改；GrowthTaskPill 的 `growthPillStep*`（v3 扩到八档）在
  o3 票内口语化；`growthPillRedraftGates` 在 o3。
- **双挂载事实**：GateCoverage 卡在 `LearningGraphWorkspace.tsx:661-677`
  与 `StructureGraphTab.tsx:167-177` 双挂载——o6 去重时以「单一挂载点 +
  Tab 间复用」为方向，不做两份改写。
- **存量债留痕（本波外）**：`services/i18n/sourcePlanLocales.test.ts` 2 例
  在 HEAD 即红（`costGate*` 词典缺失）——v2 施工时发现并留痕，与本波无关，
  顺路可修（o3/o5 施工时若触碰 sourcePlan 文案可一并补词典，否则留待
  独立小票）。

## 施工顺序建议

o1（根因 + 最高杠杆）最先；o2–o5 相互独立可并行（都是词典 + 局部组件）；
o6 最后（呈现重构需对照 v 波后的现状，且最好在 o2–o5 文案定稿后做，避免
二次改写）。真机目验与全模块欠账合并执行。

## 非目标（全波）

- common 层除 `BackendHttpError` 暴露只读字段外的任何改动（裁决：避免跨
  模块冲突）。
- 后端文案生产侧（WS text 帧保留现状 = c3 裁决 2；后端错误 message 构造
  不改——消费侧映射兜底）。
- companion 宠物气泡（消费 learning 文案，归 companion 后置，先例同 c3）。
- 保留隐喻清单内的词（裁决 3）。
- error-surface contract 的契约本身变更（o1 只改消费侧映射，闸门约定不变）。

## 收图记录（2026-11-11）

- 施工：o1 = `f5aff2084`；o2 = `9ab13389a`；o3 = `88c59582e`；o4 = `40ef60a43`；
  o5 = `6f99bf2a5`；o6 = `175b8ec9b`+`59f3151da`。o1 先行（根因票），o2–o5
  四代理并行，o6 收尾；worktree 各自分支，merger 依次并回；tracker 随
  `ecbdf73a6` 入 git。
- 双轴评审（Standards/Spec 并行子代理）：Standards **零硬违规**（common 层
  diff 为空、packContract 删键零残留、诊断信息 Tooltip/折叠收纳不丢、双语
  断言与署名达标）；Spec **非目标全守住**（后端/companion/v 波真话面零回改，
  保留隐喻除下述裁决修正外未碰）。
- 评审修复 = `302ecc3c8`（F1 术语统一）+ `276ca3b78`（F2 动作标题恢复/F3
  语境句复活/F4 triage 计数进明细/F5 reason 原文收纳/F6 映射语义修正/
  F7 组件去重两处）。
- **裁决修正（收图时）**：「知识块」移出 map 裁决 3 保留隐喻清单——o4 票面
  方向（知识组）与裁决冲突致词典三词并存，会话裁决统一「知识组」（更口语，
  且为已施工方向）；全词典 `rg 知识块` 归零（F1）。
- 验证（集成树 3716976e1 会话独立复验）：前端 learning **316 绿**、
  typecheck **0 错**、i18n **12401 keys**、check:theme 过；病词 sweep
  （主干/门口/投影/端点/幂等/铸出/登记表/留痕/补挂/回合/环境开关/安排面）
  zh 词典零命中；`sourcePlanLocales.test.ts` 存量 2 红被 o5 顺路归零。
- 留痕不修/后置：PreferenceRow 共享面 Tooltip 改动（pages/settings，additive
  已申报保留）；「修补」词族独立小票；`row.basis` 裸渲染下票核对；英文复数
  `_one/_other`（非违规）；reasonKey string 沿旧例；check:error-surface
  浏览器探针本机环境受阻（Edge 管道 stdout，非代码问题，环境正常补跑）。
- 真机欠账合并批：o 波六票目验 + v 波四票 + c1–c3 + s1/s4。

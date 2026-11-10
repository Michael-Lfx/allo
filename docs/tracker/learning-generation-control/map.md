# Map: 生成任务控制波——模型选择接线、冲突显性化、悬浮进度与性能三小票（g1–g4 · p1–p3）

<!-- wayfinder:map -->
<!-- 状态：**立票（2026-11-09）**。来源=桌面端真机（bun run dev → Flowy.exe，
      数据目录 %LOCALAPPDATA%\Flowy\Nomi-dev）建课→生长→课时全链取证 + 用户
      三项裁决。**票据全部本地**（tickets/ 7 张票面）——不使用 GitHub issue
      （用户裁决 2026-11-10；此前误建的 #422–#428 已关闭作废待管理员删除）。
      文档未提交（用户指示「建成本地md文档」）。 -->
<!-- 基线：feat/learn0928 尖端 bffb22b0d（2026-11-09 16:13 +0800）。 -->
<!-- 真机 run：2026-11-09 16:53–17:31（课程 01a1bf85-3de5-7022-8968-77587b1d2a4c，
      provider=Flowy Cloud server.flowyaipc.com/claw/v1）。 -->

## 来源：真机病历（36.0 分钟墙钟 = 有效 549s + 浪费 804s + 人操作间隔 808s）

| 任务 | 墙钟 | LLM 调用 | 入/出字符 | 有效/浪费 |
| --- | --- | --- | --- | --- |
| structure | 235.7s | 4 | 12,333 / 38,646 | 122.6s / **113.0s（48%）** |
| growth | 948.1s | 2 | 33,156 / 22,252 | 257s / **691s（73%）** |
| lesson | 55.8s | 14 | 229,503 / 11,253 | 全有效 |

关键读数：growth 浪费 = 评审 attempt1 480s 超时 + attempt2 断流 210s（断前
256KB 全是 `reasoning_content`，正文零字节）+ 网关 502 连环；同输入成功流仅
93s ⇒ provider 方差极大，非引擎问题。课时输入 229,503 vs 审计基线 68.8 万 =
**-66.7%**，u 系列 A/B 读数真机复现。留痕路径：`observation/learning-tasks.jsonl`
（任务级计时）、`diagnostics/failed-provider-sse/*.sse`（断流 256KB 捕获）。

## 已证实事实（全部 file:line 实证）

1. **模型路由分裂**：用户始终选同一模型（v4.1-flash），但只有建课
   （`GenerateCourseRequest.provider_id/model`，models.rs:49）与课时
   （`LessonStudy.tsx:752`）生效；生长链 grow 路由（routes.rs:313）硬编码
   `kick_growth_with_trigger(..., None, trigger)`——服务层签名本有
   `model_override`（learning_graph.rs:1646），只差路由+前端最后一米；
   已会声明后端支持（models.rs:2505）但前端 `declareBasis` 只传 `{ doors }`。
   None 兜底 `resolve_default_model`（knowledge_completer.rs:152）= Flowy
   Cloud 目录滤 auto 后第一个启用行 = v4-flash。
2. **冲突暴露不对称**：growth kick 冲突 → 409 → 前端 Modal.error ✓；
   structure regenerate 冲突 → spawn 后仅 WARN（learning_graph.rs:420），
   HTTP 200 kicked=true，UI 零反馈 ❌（用户实点一次）。
3. **取消面干净**：唯一取消入口=显式路由（generate/cancel +
   cancel_generation_task + cancel_learning_graph_generation）；前端仅
   index.tsx:449/466 两个显式 onCancel；`useGraphGenerationEvents` 无卸载
   副作用——**切页面不杀任务**（设计成立；失联只是事件不重放）。
4. **队列基建成熟**：m7 资源闸（TASK_GATE_LIMIT=5、queued 状态、排队取消=
   释放位）护课程生成；registry 槽串行（structure/growth/lesson 三类任务键）
   + cancel token + `set_step` 逐步文案 + `emit_graph_event` 事件流。
5. **思考模式现状**：学习补全器全量发 `thinking=Disabled`（侧任务 JSON
   约定，learning_completer.rs 头注），但 AIPC 网关对 deepseek-v4 系**无视
   该开关**（真机 256KB reasoning_content 断流留痕为证）。

## 用户裁决（2026-11-09）

1. **思考模式该开还是要开**——不作为提速手段关闭；评审/教练的判分思考保留
   （现状网关无视 disable = 实际在开）。提速走 g1 接线 + p1/p3 瘦身，不开
   新的关闭手段。开放问题：若后续换 honoring 网关，按任务面重估 thinking
   策略（记录在案，不立票）。
2. **生成后台执行 + 悬浮折叠进度**——建课链已验证的 Pill 模式推广到生长
   （g4）；用户原话动因：切页面之后生成进度失联，只能干等。
3. **队列控制核查**——结论见上「已证实事实 3/4/2」：取消面干净、冲突面
   不对称（g3 收口）、lesson 分桶成熟。

## 票列表

| id | 票面文件 | 类型 | 一句话 |
| --- | --- | --- | --- |
| g1 | [tickets/g1-growth-model-wiring.md](tickets/g1-growth-model-wiring.md) | bug | grow 路由+前端接线 modelChoice（服务层参数现成） |
| g2 | [tickets/g2-declare-model-choice.md](tickets/g2-declare-model-choice.md) | bug | 纯前端 2 处，后端现成 |
| g3 | [tickets/g3-conflict-surfacing.md](tickets/g3-conflict-surfacing.md) | bug | regenerate 冲突同步暴露 + lesson/auto_refill 顺带核查 |
| g4 | [tickets/g4-growth-floating-pill.md](tickets/g4-growth-floating-pill.md) | enhancement | 推广 CourseGenerationPill 模式；轮询为准+事件增强 |
| p1 | [tickets/p1-review-batch-json-compact.md](tickets/p1-review-batch-json-compact.md) | enhancement | pretty→紧凑，入参省 20-30%，单点零语义差 |
| p2 | [tickets/p2-structure-unknown-key-lenient.md](tickets/p2-structure-unknown-key-lenient.md) | enhancement | 删键+findings 留痕，不再整份两产物重生成 |
| p3 | [tickets/p3-mint-evidence-parallel.md](tickets/p3-mint-evidence-parallel.md) | enhancement | 5 次串行 3.1s → fan-out，fail-open 不变 |

## 影响面速查

- g1/g2：无新增用户可见文案（无 i18n）；g3/g4：新文案必须 zh-CN/en-US
  双语对称（`gen:i18n` + `check:i18n`），g4 另过 `check:theme`/`check:dead-css`。
- 后端单点票（p1/p2/p3）：`cargo test -p nomifun-learning` 基线；
  p2 注意与 attempt 循环 Rejected 语义边界（只放宽可机械修复类违规）。
- 前端票：`bun run typecheck`；g4 有组件骨架可复用（CourseGenerationPill）。

## 开放问题（记录在案，不立票）

1. lesson 任务是否并入 g4 Pill（二期）。
2. thinking 策略按任务面分化（评审开、机械 JSON 关）——依赖网关 honoring
   行为，先观察。
3. auto_refill 自动触发的冲突静默降级边界（g3 验收内顺带）。
4. 建课 Pill 取消按钮仍调旧全量取消端点，会连带取消正在跑的 growth
   （g4 施工记录遗留，票外现存）——修复候选。
5. code-review 裁量不动项：双 Pill 壳提取（GrowthTaskPill/CourseGenerationPill
   同形）、`(provider_id, model)` 元组的后端小类型化、前端解析后端中文主题串
   当协议（`GROWTH_TOPIC_PREFIX`，有防御回退）——均有哨兵测试/回退兜底，
   留观不施工。

## code-review 结果（2026-11-10）

双轴评审（b29c34a9a...c43dae34e，27 文件 +1475/-91）：Standards 零硬违规
（i18n/主题/mock/署名四红线全过），6 项 smell judgement call——其中 4 项进
修复轮（成对校验 4 份重复收口 `paired_model_choice`、spawn 样板提取、
`RegenerateStructureRequest`→`ModelChoiceOverrideRequest` 改名、p2 生成级
findings 断言补盲），2 项留观（见开放问题 5）。Spec 零实质缺口，p2 盲区入
修复轮，两处票外裁量已补记 g4 票面。

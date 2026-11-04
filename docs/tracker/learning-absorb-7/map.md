# Map: 第七轮吸收波——learnhub 22d016d 机制对照（y01–y07）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-04）。来源 = learnhub main@22d016d 相对上次参照点 b6a8c4e
      的 16 个提交复核（七组并行深读 + allo 修改面实码核对）。
      用户裁决（2026-11-04）：立票范围 = 第一 + 第二优先级；y02 先只改学习模块，
      公共代码尽量少动、确有必要才动；实现时先读 learnhub 参考实码，减少重复研究。 -->
<!-- 基线：feat/learn0928 尖端 50a9d2fae（本波立票提交）。 -->
<!-- 参照点：learnhub main 22d016d（2026-11-03 21:02 +0800）；工作树有一处
      本地未提交修改（src-tauri/src/bin/smoke-loop.rs），与本波无关，未触碰。 -->

## 来源

learnhub `b6a8c4e..22d016d` 共 16 个提交，按机制归组：

| 组 | 提交 | 机制 |
| --- | --- | --- |
| LLM 韧性 | `f87ec29`(#198)、`a77cdb6`(#167) | 失败三分类、传输退避重试器、硬超时、parse 失败窗口 |
| 名字裁决 | `f87ec29`(#199) | one_name L0/L1/L2 三层分工 + 批量中立裁决 |
| 内容契约/建课 | `115382c`(#191)、`d5653d7`(#204)、`5746078`(#193) | 契约 v2.16 ambition + 计划摘要 sidecar、概念只读检索、计划态轻确认 |
| 系统动作簿 | `bc36fc0`(#192)、`5f830bc`、`22d016d` | 自主动作统一留痕 + 课程详情折叠节 + 评审修复 |
| 归因分账 | `594bd9c`(#200) | 首答/重答 novelty + elapsed 流利档 + would_flip 审计 |
| 提示词条款批 | `8932abb`/`565a2fe`/`7575d4f`/`e7f912c`(#201–203) | 干扰项五型、完备性三面、材料学六条 |
| 超载信号 | `8a97133`(#194) | 节点容量反馈三选一 + 课程聚合 + 生成装配 |
| 文档 | `3e02db7`、`464c550` | 六张施工图 + 教学法失效类型学研究 |

## 总约束

1. **y02 学习模块内闭环**：v1 不改 `nomifun-common::AppError`、不改
   `nomifun-ai-agent`、不改 provider 侧错误映射。分类只用学习侧已能看到的
   `AppError` 变体；若实施中发现必须动公共代码，先停手记偏差、回报裁决，
   不得静默扩大。
2. **优先复用现有设施**：登记表/别名/近名、prompt 资产注册表、金样、
   generation task 注册表、维护回合报告、既有 UI 时间线优先；不新造第二套。
3. **迁移只编辑 070 基线**：`070_learning_domain_rebuild.sql` 仍是未发布尖端
   （x21 净形基线），y04/y07 的 schema 改动都在该文件对应节内完成，不新增
   迁移文件；改表数/索引时同步 `id_schema_contract` 与迁移测试。
4. **行为变更带开关或可回退臂**：y06 条款沿用
   `NOMI_LEARNING_CONTENT_DISCIPLINE_V2` 的 on/off 双臂；y01 的 L2 裁决
   失败一律降级为提示（fail-open），不新增硬门。
5. **真机臂严格串行**；涉及写读闭环的票跑 `nomicore learning-smoke` 对应档。
6. **开工先复核 file:line**（f1 惯例）：票内行号来自 2026-11-04 盘点，
   learnhub 参考行号固定按 `22d016d` 读。

## 优先级与票表

第一优先级 = y01–y03（真实缺口/健壮性/生成面自查）；第二优先级 = y04–y07
（产品面机制，改造后吸收）。

| id | 票 | 档 | 阻塞/顺序 |
| --- | --- | --- | --- |
| y01 | one_name 三层分工——L0 硬门 / L1 点名 / L2 批量中立裁决 | **高** | 排在 y02 后（同碰 `structure_gen::attempt_once`） |
| y02 | LLM 失败分流——parse 窗口 + 传输失败不占重试预算（学习模块内） | **高** | 先做；无迁移、无公共代码改动 |
| y03 | 生成面只读检索工具 `search_concepts` | **高** | 独立，可与 y02 并行（不动同一文件） |
| y04 | 系统动作簿——自主动作终态留痕 + 课程详情折叠节 | 中 | 迁移线；与 y07 串行编辑 070 |
| y05 | 建课计划态轻确认——填写→计划→确认生成 | 中 | 纯前端，独立 |
| y06 | 内容/题库提示词条款批——干扰项五型、完备性三面、材料学六条 | 中 | 独立（动 prompt 常量与孪生块） |
| y07 | 节点容量反馈信号——capacity_note 落库 + 三选一留痕（零置位） | 中 | 迁移线；排在 y04 后编辑 070 |

## 实施编排

- **阶段 A（y02 → y01，y03 并行）**：y02 先改 `attempt_once` 的错误类型与
  各重试环分流；y01 再在干净的错误类型上插入 L2 裁决。y03 只动 service /
  ai-agent 工具面，与两者不冲突。
- **阶段 B（y04 → y07 串行迁移；y05/y06 并行）**：y04 先在 070 落动作账
  表与 growth 取消语义；y07 再在同一基线加 `capacity_note` 列与反馈表。
  y05 纯前端；y06 只动提示词/断言。
- 每票一提交（阶段 A 的 y02/y01 可各一提交），先并回 `feat/learn0928`；
  真机验收按票串行，不并发挤兑 provider。

## 验证总纲

1. 学习 crate：`cargo test -p nomifun-learning`；触 ai-agent 的票加
   `-p nomifun-ai-agent`；触迁移的票加 `-p nomifun-db`。
2. 前端票：`bun run typecheck`；新增/改动用户可见文案先 `bun run gen:i18n`
   再 `bun run check:i18n`。
3. 聚合门：`cargo check --workspace && bun run check`（PR 前）。
4. 真机：y01 structure（含「求和公式」正例/「双进程与窗口」反例）、y02
   structure/lesson、y03 content、y04 maintenance/growth、y06 content+题库、
   y07 lesson 发布读回；各档 `EXIT=0` 且读数行在场。
5. 每票的零翻转/零副作用判据见票面「验收」；y07 必须证明 attempts/xp/
   review/FSRS 计数不变。

## 明确不吸收（本轮）

- `ambition` + `plan-summary` sidecar：allo 是 DB 单源、无课程文件目录；
  先落已有 `capacity_note` 与节清单事实（y07），`ambition` 等有 UI 消费方
  再单独立票。
- novelty 首答/重答分账与 elapsed 流利档：allo 无统一 replay 投影，quiz
  作答不入 `learning_attempts`，`elapsed_ms` 采而不用；先做离线诊断再议。
- learnhub replay 夹具体系（catalog/compare/expect）：allo 无 harness，
  移植成本高于收益；用 Scripted completer 单测 + 真机 smoke。
- JSONL 动作账本 + 全档扫描 + `shadow`/`Started` 预留位（y04 只吸语义）。
- 超载信号的 `extend→重生成` / `split→拆节` 自动执行（allo 无 est 修改
  API，会留死信）；以及把用户反馈注入共享课时生成（多用户串味）。
- `elapsed` 10s 流利阈值、learnhub 契约中「`和` 仅词中位」的旧表述（与其
  自身代码矛盾，不抄）。

## Decisions so far

- 2026-11-04：开图立票；范围 = 第一 + 第二优先级七票；y02 锁定学习模块
  内闭环；y01 与 y02 顺序按 `structure_gen` 冲突面排定；ambition/摘要、
  novelty、回放夹具不占本轮。
- 2026-11-04：y05 关闭——建课三入口（知识库/描述/学习图）校验后先进入对话框
  内计划态，回显四槽 + 确定性读数，唯一发起点为「开始生成」，返回修改/关闭
  零请求；t5 成本门并入计划卡、独立 Modal 删除（5777dbaca）。

---
id: j03
title: "生长/结构循环行为加固：重试机制 + 真取消"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# j03: 生长/结构循环行为加固（重试机制 + 真取消）

**What to build:** 两类循环行为修复一次到位（同文件合并实施）——①二轮 t1 重试机制
（同错指纹早停 + guard 豁免 + 容量守卫 + 原地重试）；②三轮 t2 长任务注册表与真取消
（修复假取消缺陷）。

**实现决议:** 二轮 t1（全部）+ 三轮 t2（全部）。参考实现：learnhub
`engine/maintenance/mod.rs` RoundRetry、`app/generation.rs`（全套含 mid-call 取消测试）。

**Blocked by:** None（与 j01 并行安全；建议早于 j05/j07——同文件循环改造一次到位）。

**Status:** ready-for-agent

- [x] **同指纹早停**：指纹 = 失败报告规范化 JSON；`growth_body` 与 `structure_gen` 两
      循环同款——`attempt > 1 && fingerprint == prev && !guard` → 立即 exhausted
      （连败两次即停，调用数 2 非 3）
- [x] **「未点名被改」机械守卫**：重裁输出 vs 上一稿 diff——未点名节点逐字段相等，违者
      `guard=true` 固定文案打回；guard 失败豁免早停 + **原地重试恰一次（不占 3 轮预算）**
      ，二次越权计轮
- [x] **容量守卫**：重裁出口 `nodes.len() > 7` → 专用码 `ng.batch.redraft_capacity` +
      定向指令（「按拆分后的形态重新装箱……不要放弃已正确的拆分」）；`ng.batch.size`
      起草轮执法单源不变
- [x] **长任务注册表**：新模块 `service/generation_tasks.rs`（键 `{kind}:{course_id}:
      {node_id|-}`；状态机 running→done|failed|cancelled 终态幂等、同键并发 begin 拒绝、
      AtomicU64 注册序排序）；快照命令 `GET /generation-tasks` + 统一取消端点（存量
      `/generate/cancel`、grow cancel 改造转发）
- [x] **CancellationToken 全调用点双检查**：`growth_body` 每轮起草/重裁/评审前后、
      `structure_gen` 两调用前后 + 循环头、快照写前（写前取消 = 零快照）、lesson/outline
      管线各 LLM 调用前后；completer 调用 `select!` 真中断（中断 = 断 HTTP）；批事务
      原子落库 = 中断零落库
- [x] **中性终态 envelope**：`generate`/`grow` 响应升 `{state, ...}`；取消 = cancelled
      不报错误；生成开始后才取消 = 竞速落败如实 done；WS 对齐（补 structure_cancelled）
- [x] **进度叙事**：step 机器面/detail 人面单源在后端；前端共享订阅 hook（事件缺席不碍
      流程）；三消费方接线（建课对话框/结构图 Tab/学习页）
- [x] **观测旁路**：begin/finish JSONL 落观测面（写失败降级）
- [x] **测试口径**（learnhub 移植）：同错连败两次即停（调用数 2）/ 异错烧满预算 /
      超容定向打回 / 未点名被改原地重试落位、二次越权计轮 / mid-call 取消内层恰被调
      一次且零落库 / 终态幂等 / 同键拒绝 / 注册序排序 / 每类长操作一个取消用例

**Resolution:**

**速览**：全项落地（feat/learn0928，本票单提交）。t1 重试机制（早停/守卫/原地重试/
容量定向）+ t2 长任务注册表与真取消一次到位；learning 258 全绿（+13 新测）、ai-agent
1093 全绿、typecheck 60 = 存量基线、workspace check 干净。

- **重试机制**（新模块 `learning_graph/growth_retry.rs`，`growth_body` 与
  `structure_gen` 同款口径）：
  - **同指纹早停**：生长批指纹按失败面取材——门报告 = `Vec<GateError>` 结构化序列化、
    评审 = `Vec<ReviewProblem>` 序列化、起草/传输错 = 报告逐行规范化；结构循环 =
    逐条款报告规范化。`attempt > 1 && fingerprint == prev && !guard` → 立即耗尽。
  - **「未点名被改」机械守卫**（learnhub 116e805 同构）：重裁输出 vs 上一稿——
    未出现在打回报告点名清单（标题子串匹配）中的既有节点必须逐字段相等
    （title/purpose/est/difficulty/teaches/assumes），被移除同算改动；**新增节点合法**
    （点修允许补铺垫）；被点名节点不核对。违者固定文案（`ng.redraft.scope`）。
  - **guard 豁免 + 原地重试恰一次**：guard 失败不入指纹链（固定文案 ≠ 同输出）、
    同一 attempt 内带守卫原因对最新一稿再裁一次（不占 3 轮预算）；二次越权计轮。
    测试实况：三节点脚本下全流程 6 次调用 3 轮落位——两轮内各自的原地重试都被
    预算豁免（纯计轮在第 3 轮即断）。
  - **容量守卫**：重裁出口拦截 `ng.batch.size`（parse 阶段的超容违约）→ 改判
    `ng.batch.redraft_capacity` 定向指令（携带上一稿节点清单 = 拆分后形态 + 「超出
    留待后续批次，不要放弃已正确的拆分」）；起草轮执法单源（growth_contract）不变。
  - **结构循环早停**：门口对账/契约报告同指纹两轮即停（测试：spines 空的永违约
    产物 2 轮 4 次调用即耗尽，第 3 轮 2 次不发生——每轮耗 2 调用：结构稿 JSON
    合法、对账在地形稿之后才红）。
  - **smoke 读数**：`GrowthRunStats { attempts_used, stop_reason }` 落
    `LearningService`（success | early_stop | exhausted | cancelled），
    `learning-smoke grow/loop` 逐轮输出，读数面非门禁。
- **长任务注册表**（新模块 `service/generation_tasks.rs`）：
  - 键 `{kind}:{course_id}:{node_id|-}`，四类 kind = growth/structure/outline/lesson；
    状态机 `running → done|failed|cancelled` **终态首写即定**（取消先到 = cancelled，
    任务先完成 = 竞速落败如实 done——测试钉住）；同键并发 begin 拒绝（Conflict），
    终态记录被同键新任务替换；AtomicU64 注册序排序（在跑置顶、终态随后倒序）；
    守卫 drop 未收尾 = failed 留痕（abort/panic 兜底）。
  - **端点**：`GET /api/learning/generation-tasks`（快照：在跑置顶 + step/detail/
    elapsed）；`POST /api/learning/generation-tasks/{key}/cancel`（取消胜出 =
    `{state:"cancelled"}`，竞速落败如实返回 done/failed，键不存在 404）；存量
    `/generate/cancel` 改造为注册表薄转发（`cancel_running`，全部可取消在跑任务）；
    legacy status 端点数据源切到注册表（最新在跑任务的 topic/elapsed）。
  - **进度叙事**：`set_step(step 机器面, detail 人面)` 后端单源（draft/review/
    structure 检查点），快照携带；前端共享订阅 hook
    `hooks/useGraphGenerationEvents.ts`（growth_*/structure_* 按课程过滤、
    `structure_cancelled` 驱动「生成中」徽章复位）接学习页工作区；批次时间线/
    概念表为全量自订阅天然覆盖，建课对话框维持既有对话框事件订阅（事件形状
    不同，hook 注释明示边界）。
  - **中性终态与 WS**：`growth_cancelled` 升 phase=cancelled + outcome=cancelled
    （原以 phase=failed 发出——假取消时代的错位一并修正）；新增
    `structure_cancelled`（phase=cancelled）；growth/structure 的 Err 分支按
    任务 token 判别取消（含 CANCELLED_MESSAGE 文案兜底），取消不再伪装失败。
- **真取消接线**（每任务一枚 `CancellationToken`）：
  - **growth_body**：循环头 + 起草/重裁循环头 + 落库前检查点；全部 completer
    调用（教练起草/重裁/概念评审）经 `CancellationCompleter`（新，实现
    `LearningCompleter`，逐方法 `tokio::select!` 与 token 竞速）——调用前已取消
    = 零调用，调用中取消 = 断 HTTP（结果丢弃）。批事务原子落库 = 中断零落库
    （测试：mid-call 取消 → 内层恰被调一次、种子 2 节点零新增、批行 failed、
    任务终态 cancelled、stats stop_reason=cancelled）。
  - **structure_gen**：`generate_structure_artifacts` 增 `cancel: Option<&Token>`
    （Option 保测试与 smoke 直调面）；循环头检查 + 两次 LLM 调用 select! 竞速；
    `run_structure_generation` 快照写前检查（写前取消 = 零快照）+ 任务注册。
  - **大纲流**：`CancellableProvider`（ai-agent）旗标升级为 token——请求前拒绝 +
    流转发 `select!` 竞速（语义与原轮询等价、信号源统一）；`generation_cancel_flag()`
    = 最新在跑 outline 任务的 token。outline HTTP 流任务登记注册表（键课程段 =
    会话 id，建课时课程尚不存在）。
  - **课时流**：任务登记（`lesson:{course}:{lesson}`）+ 快照可见；**循环内 token
    接线维持 ADR-0004 显式差异**（引擎自建 provider，跨 trait 改造留后续票），
    对 lesson 键的取消请求如实拒绝（「断开请求即中断」——HTTP 内联执行天然具备
    真中断，零落库由草稿 TTL 与事务语义保证）。**实施裁决记录**：t2 检查点清单的
    「lesson 管线各 LLM 调用前后」以「注册面到位 + 取消如实拒绝」落位，跨 crate
    token 贯通显式留待后续（Resolution 偏差项）。
  - **观测旁路**：`set_generation_task_observer`（组合根 services.rs 注入）——
    begin/finish 两型 JSONL 追加落 `{data_dir}/observation/learning-tasks.jsonl`，
    observer 缺席或写失败静默降级（与 ArchivingCompleter 同目录约定）。
- **测试口径全落**（learning 258 = 基线 245 + 13 新）：同错两停（调用数 2，stats
  early_stop）/ 异错烧满（3 调用 exhausted）/ 超容定向打回（第三轮装配含专用码 +
  落库）/ 守卫两段（原地重试不占预算 + 二次越权计轮，6 调用 3 轮落位）/
  mid-call 取消（1 调用零落库终态 cancelled）/ 结构早停（4 调用非 6）/ 注册表
  7 测（终态幂等/同键拒绝/取消胜出触发 token/drop 留痕/注册序排序/lesson 拒取/
  wrapper 中断竞速）。
- **实施偏差与边界记录**：① lesson 循环内取消接线留后续（上段，唯一缩项）；
  ② 指纹对 guard 失败重置链（prev=None）而非透传——learnhub 语义「guard 不参与
  早停比对的保守化」；③ 容量守卫实现为重裁 Err 分支的条款码拦截（`ng.batch.size`
  消息在 parse 阶段产生，循环层改判为定向指令）——与票面「nodes.len()>7 前置
  检查」等价（parse 拒收即超容事实），避免在循环层重写 schema 校验；④
  `generate`/`grow` HTTP 响应本身是异步触发面（`{kicked}`/课程详情），outcome
  envelope 落在 WS 终态帧与统一取消端点（`{state}`）——两者才是完成态的承载面。
- **验证**：learning 258 全绿、ai-agent 1093 全绿、`cargo check --workspace` 干净、
  `bun run typecheck` 60 = 存量基线（learning 路径零新增）、check:i18n 无新键
  （进度文案后端单源，无前端新串）。

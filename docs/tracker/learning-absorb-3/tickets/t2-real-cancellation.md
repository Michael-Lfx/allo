---
id: t2
title: "长任务真取消定案（假取消缺陷修复）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

r3 报告实锤：allo 的生长取消是**假取消**——`generation_registry.cancel: AtomicBool`
存在但 `growth_body` 循环（≤3 轮 LLM）与 coach_draft/coach_redraft **零检查点**；用户点
取消后生长继续跑完并落库，仅 Err 收尾分支把 failed 改标 cancelled。这是行为缺陷修复
（P0），learnhub #103 是蓝本。本票定案 allo 的实现：

1. **取消机制**：allo 是 async 栈（learnhub 是 sync 接缝才「不中断在途请求」）——采用
   tokio CancellationToken（或复用 AtomicBool）在**每个 await 点前后双检查**（LLM 调用
   前拦截则零调用；调用返回后检查则丢弃结果）——比 learnhub 更强的真中断；须定义「中断
   后已完成部分」语义（learnhub 靠批内原子写 + 幂等步进兜底；allo 批事务原子落库天然
   满足——中断 = 本批零落库）。
2. **任务注册表**：统一注册表（进程内 Mutex<HashMap> + 快照命令）覆盖四类长操作——
   生长、结构生成/重生成（i02 管线）、课时内容生成（lesson_content_loop）、单题重出
   （若做）；任务键 `{kind}:{course_id}:{node_id|-}`、状态机终态幂等、同键并发 begin
   拒绝、注册序排序判据（allo 批行已有 DB seq 068 可复用）。
3. **中性终态**：取消 = cancelled 不报错误；命令出参升 outcome envelope；WS 事件
   （growth_cancelled 已有）对齐任务状态机；**单步任务生成开始后才取消 = 竞速落败，
   终态如实 done**（诚实原则）。
4. **进度叙事**：步骤叙事映射（同任务名不同型不同文案；step 机器面/detail 人面）+ 前端
   共享订阅 hook（事件缺席不碍流程——命令 promise 是完成态唯一真相）。
5. **观测留痕**：begin/finish 两型 JSONL 旁路（非执法）对齐 ArchivingCompleter 观测面。

产出：落点定案（注册表模块位置、取消检查点清单——逐 LLM 调用点枚举、envelope 形状、
WS/DTO 变更），供切票施工。参考：learnhub `app/generation.rs`（全套含测试：mid-call
取消 → 内层恰被调一次、结果丢弃、零落库）。

## Resolution

**速览**：六项定案——CancellationToken 全调用点双检查（async 栈真中断，强于 learnhub）；
统一注册表模块落 nomifun-learning；中性终态 envelope；四类长操作接入清单；WS/DTO 对齐；
观测旁路。

1. **取消机制**：每任务一个 `tokio_util::sync::CancellationToken`（存注册表）；**检查点
   枚举**（实施时逐点接）：`growth_body` 每轮 coach_draft/coach_redraft 调用前后 + 概念
   评审调用前后；`structure_gen` 两调用前后 + 每轮循环头；`run_structure_generation` 快照
   写前（写前取消 = 零快照，磁盘无半成品的 allo 等价语义）；lesson_content_loop / outline
   管线各 LLM 调用前后。**真中断**：completer 调用用 `tokio::select!` 包裹（token 与
   future 竞速，中断即断 HTTP）——强于 learnhub 的 sync「不中断在途」；**已完成部分
   语义**：批事务原子落库天然满足（中断 = 本批零落库）；结构快照写前检查同律。
2. **注册表**：新模块 `nomifun-learning/src/service/generation_tasks.rs`（进程内
   `Mutex<HashMap<task_key, TaskRecord>>`；键 = `{kind}:{course_id}:{node_id|-}`；状态机
   running→done|failed|cancelled 终态幂等、同键并发 begin 拒绝、注册序 AtomicU64 排序
   ——在跑置顶其余倒序）；快照命令 `GET /api/learning/generation-tasks`；取消命令统一
   `POST /api/learning/generation-tasks/{key}/cancel`（存量 `/generate/cancel` 与 grow
   cancel 端点改造为注册表薄的转发，语义升级为真取消）。
3. **中性终态与 envelope**：取消 = `cancelled` 不报错误；`generate`/`grow` 响应升
   `{state: 'completed'|'failed'|'cancelled', ...}` envelope；WS 事件对齐（growth_cancelled
   已有，补 structure_cancelled）；**生成开始后才取消 = 竞速落败终态如实 done**（诚实
   原则）。
4. **进度叙事**：`step` 机器面英文 + `detail` 人面中文单源在后端（前端不造步骤文案）；
   前端共享订阅 hook（按 kind/course 过滤、事件缺席不碍流程——命令 promise 是完成态
   唯一真相、收尾清叙事防闪现），三消费方：建课对话框、结构图 Tab、学习页。
5. **观测旁路**：begin/finish 两型 JSONL 落 `observation/learning-smoke/` 同目录约定
   （ArchivingCompleter 相邻），写失败降级不阻断。
6. **测试口径**（learnhub 移植）：mid-call 取消 → LLM 内层恰被调一次、结果丢弃、
   零落库；终态幂等（重复 cancel/finish 不改写）；同键并发 begin 拒绝；同秒快照排序按
   注册序；每类长操作一个取消用例。


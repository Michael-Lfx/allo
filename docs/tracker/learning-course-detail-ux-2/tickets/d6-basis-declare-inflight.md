---
id: d6
title: "已会声明在途恢复——后端注册表行 + 前端会话级 pending + 409 友好化"
type: task
status: open
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# d6: 已会声明的在途状态恢复（用户真机报告的 409 现场）

**What to build:** 声明已会是分钟级 LLM 操作，但在途状态只存在组件本地
（`declaringBasis`）——切页再回来组件重挂载、状态归零、按钮恢复可点，再点被
后端槽锁 409 拒绝（用户截图实证），且原始请求的成败结果在返回时已无处落地。
三段修：

1. **后端**：`declare_basis` 注册进生成任务注册表（挂载回合同款
   `begin_generation_task`，kind = `basis_declare`，key
   `basis_declare:{course}:-`，topic「已会声明」）——唯一缺口：目前 basis
   declare 是全学习域**唯一**不进注册表的长任务（审计结论），注册后
   `GET /api/learning/generation-tasks` 即可发现「本课声明正在跑」。
2. **前端会话级 pending**：声明发起时写进程级 pending 标记（key =
   course id），完成/失败清除——组件重挂载后从模块读，同会话内切页往返
   100% 恢复（不依赖任何端点）。
3. **409 友好化**：兜底竞态下 409（message 含
   `basis-declare-{courseId}`）不再弹原始 JSON 错误 Modal，改为非阻塞提示
   「声明仍在后台进行，已为你同步状态」+ 重拉 basis 读数。

## 法证证据（2026-11-10 审计，真机截图 + 实码）

- 现场链：`GateCoverage` 声明按钮 loading = `declaring`（workspace 本地
  state）→ 学习图工作区随路由卸载即丢；重挂载后 `basis.declarations` 若尚为
  0（LLM 未写完锚行），按钮恢复可点 → 再点 → 槽锁
  `basis-declare-{course}`（learning_graph.rs:5482）409（AppError::Conflict，
  nomifun-common error.rs:178/209）→ 原始错误 Modal（截图）。
- **审计定位**：basis declare 是唯一不进注册表的长任务。注册表六类 kind
  （generation_tasks.rs:48-53）：growth / structure / structure_mount /
  structure_repair / outline / lesson；`GET /api/learning/generation-tasks`
  行 DTO 含 kind/course_id/node_id/state/topic（generation_tasks.rs:73-91），
  挂载/修补/生长/课时内容都能从它恢复在途态，唯独声明不行。
- 读侧自愈已存在：声明成功发 WS `basis_declared`（learning_graph.rs:5896），
  工作区重挂载也会重拉 `GET /basis`——冻结展示（declarations > 0）在返回后
  自然成立；缺的只是「在跑」的可见性与重复点击防护。
- 无失败事件：声明失败只有原始请求的 response 能知道（挂载期间的失败在组件
  卸载后无处落地）——失败静默是现状极限，票面如实记档（注册表行的 failed
  状态让重挂载后至少能看到「声明失败过」的痕迹）。

## 实施要点

- [ ] 后端：generation_tasks 注册 `basis_declare` kind；`declare_basis` 在槽
      锁获取 + 校验后 `begin_generation_task`（守卫持有跨 LLM 调用与写事务，
      drop 自动收尾——挂载回合同款）；basis_tests 补「完成后快照含
      basis_declare 行（course 匹配）」断言。
- [ ] 前端：新增 `learningInFlight.ts` 进程级 pending 注册（mark/clear/
      has，key = `basis:{course}` 等）；`declareBasis` 发起时 mark、finally
      clear；工作区挂载时 tasks 快照（kind=basis_declare + course 匹配 +
      queued/running）∪ 会话 pending → `declaring` 传给 GateCoverage（按钮
      loading 且不可点）。
- [ ] 409 兜底：declareBasis catch 判「message 含 CONFLICT/already running
      且含 `basis-declare-{courseId}`」→ `Message.info` 友好提示 + 重拉
      basis，不再 Modal.error。
- [ ] Rust 测试（basis_tests 追加）+ 前端 typecheck/check:i18n（新提示文案
      双侧对称）。

## 验收

- `cargo test -p nomifun-learning` 全绿（新增注册表断言）；`cargo check
  --workspace`。
- `bun run typecheck`（55 错基线一致）+ 学习域单测绿。
- 真机判据（用户侧）：声明进行中切页再回来 → 按钮 loading 不可点；生成完成
  → 冻结展示；竞态双击 → 友好提示不弹原始错误。

## 非目标

- 失败事件的 WS 面（现状极限，记档）；声明可撤（c3 已裁）。

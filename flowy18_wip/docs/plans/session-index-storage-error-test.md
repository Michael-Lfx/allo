# 会话索引损坏时 storage_error 测试核验

状态：已修复（2026-11-17）。

## 结论

`failed_new_task_is_not_hidden_by_navigation` 当前失败点不是 UI 状态断言，而是磁盘断言：
测试仍要求 `sessions/index.json` 保持 `{broken`，但产品行为已改为
「隔离损坏索引 → 从结构化 history 重建 → 写回新的 index.json」。

## 实际失败点与证据

- 测试位置：`crates/flowy-app/src/app/mod.rs:1411`。
- 当前工作区（未提交）已在导航后推进 33ms：`run_until_parked` +
  `advance_clock(33ms)` + `run_until_parked`（1424-1426）。
- 实跑测试二进制：`assert!(storage_error)`（1428）通过，panic 在
  `mod.rs:1430:9` 的 `assert_eq!`；left 为重建后的有效 index JSON，
  right 为 `b"{broken"`。
- `AgentHandle::boot`（`crates/flowy-agent/src/handle.rs:137-230`）：
  - `SessionIndex::load` 返回 `InvalidData` 时调用 `recover_corrupt_index`，
    把文件重命名为 `index.corrupt-<ms>.json`，并投递 `AgentUpdate::StorageError`；
  - 随后 `Engine::open` 读取 `sessions/history.json`（结构化 history）；
  - `import_engine_sessions` 把 history 中的会话并回 UI live 集合；
  - `persist_index` 写回新的 `sessions/index.json`。
- 测试残留目录（panic 前未清理）现场：
  - `sessions/index.corrupt-*.json` 内容为 `{broken`；
  - `sessions/index.json` 为有效 JSON，含新建会话；
  - `sessions/history.json` 含同一会话。

## 与旧假设的差异

1. 「测试没有等待异步通知」只对 HEAD 成立；工作区已加入 33ms 推进（未提交），
   该断言已通过。真正失败的是后面的磁盘断言。
2. 「失败发生在 UI 状态断言，不能据此判断新建会话/持久化是否失败」不成立：
   当前失败在磁盘断言，新建会话与持久化均成功。
3. 当前测试没有预置 history 会话，因此只验证了「索引能重新写出来」，
   没有验证「从 history 恢复会话」。

## 另需注意

`storage_error` 提示条只在 `Screen::Main` 渲染
（`crates/flowy-app/src/app/mod.rs:1116`）。本测试没有登录会话，
`screen == Login`，所以该测试只验证了内部标志位，并未验证提示真正可见。
若要验证「可见」，需要预置 `auth/session.json` 进入 Main，或显式断言渲染条件。

## 建议的修复方向（未实施）

- 保留隔离与重建逻辑，不为了旧断言保留损坏的活动索引。
- 测试改造：
  1. 预置至少一个 `sessions/history.json` 会话（字段：
     `id/title/titled/updated_at/status/messages/todos`）；
  2. 导航后用 GPUI 执行器有界推进（33ms 步进，最多 N 次），
     而不是固定 sleep；
  3. 断言 `storage_error` 为真；如需验证「不被导航清除」，在提示出现后再导航一次；
  4. 磁盘断言改为：损坏原件已隔离且内容为 `{broken`；新的 `index.json` 可解析；
     history 中的会话出现在重建索引中。

## 修复记录（2026-11-17 已实施）

按上述方向重写了 `failed_new_task_is_not_hidden_by_navigation`，未改产品逻辑：

- 预置 `sessions/index.json = {broken` 与一个 `history.json` 会话（`hist-1`）；
- 导航到 new-task 后，按 33ms 步进最多 8 次推进 GPUI pump，直到 `storage_error` 为真；
- 提示到达后再导航一次（Automation），断言 `storage_error` 仍为真；
- 磁盘断言：`index.corrupt-*` 恰好一个且内容为 `{broken`；新的 `index.json` 可被
  `serde_json` 解析；`hist-1` 出现在重建索引的 sessions 中。
- 验证：`cargo test -p flowy-app` → 21 passed / 0 failed（2026-11-17）。

# PROGRESS · 本地受理回执纵线

分支：`feat-flowy-local-receipt`　基线：`ba5ace40a`　worktree：`.agents/worktrees/feat-flowy-local-receipt`

---

## 任务 0 · 基线核对（2026-11-23）

### 环境

| 项 | 实测 |
| --- | --- |
| bun | `1.3.14` |
| cargo | `1.100.0 (3acc79c0c 2026-11-05)` |
| `target/`（worktree 内） | 不存在 —— 首次冷构建 |
| `node_modules/`（worktree 内） | **不存在** —— 新 worktree 必跑 `bun install` |
| `ui/dist/index.html` | 存在 |

### 二进制名更正

任务书写 `./target/debug/flowy-app.exe`。**实际上游产物是 `Flowy.exe`**（另有 `Flowy.d`、`Flowy.pdb`）。

→ 启动命令更正为：

```bash
(./target/debug/Flowy.exe >/dev/null 2>&1 &)
```

### 命令基线

| 命令 | 结果 | 耗时 |
| --- | --- | --- |
| `cargo check -p nomifun-ai-agent` | **通过，0 error**，全部是既有 warning（`nomi-agent` 1 条、`nomifun-learning` 44 条） | 4m57s（冷） |
| `bun run typecheck`（未装依赖时） | **失败**：`Script not found "node_modules/typescript/bin/tsc"` | — |
| `bun run typecheck`（`bun install` 后） | **仍然失败** —— 有 2 个包没装上（见下） | — |
| `bun run typecheck`（补齐 2 个包后） | **通过，exit 0** | — |

### 环境坑：新 worktree 要补 3 步

1. worktree 内**没有** `node_modules` → 必须 `bun install`
2. `bun install` 在 Windows 上对两个包报 `ENOENT: failed to link package (copyfile)`：
   `@tanstack/react-query`、`@excalidraw/excalidraw`
3. 修法：从主工作区 `ui/node_modules/` 直接复制这三个目录过去

```bash
W=<worktree>/ui/node_modules ; M=<主工作区>/ui/node_modules
mkdir -p "$W/@tanstack" "$W/@excalidraw"
cp -r "$M/@tanstack/react-query" "$M/@tanstack/react-virtual" "$W/@tanstack/"
cp -r "$M/@excalidraw/excalidraw" "$W/@excalidraw/"
```

补齐后 typecheck 干净通过。**这条不写下来，下一个人还会卡一遍。**

**这两个数字是后面所有改动的对照基线。任何改动引入的 `cargo check` error 或 typecheck 失败，都算我引入的。**

---

## 受理链路：与审查报告不一致

**审查报告说**：「当前输入框在执行繁忙或云未就绪时拦截提交，pi 的 steering 回调尚未接线。」

**代码读下来只对了一半。**

### 两种拦截不是一回事

| 位置 | 事实 |
| --- | --- |
| `ui/src/renderer/components/chat/SendBox/index.tsx:370` | `allowSendWhileLoading = false`（默认值） |
| 同文件 `:2296` | 忙时拦截条件 **`!allowSendWhileLoading && (isLoading \|\| loading)`** |
| `platforms/nomi/NomiSendBox.tsx:2532` | **传了 `allowSendWhileLoading`（= true）** |
| 同文件 `:2523-2524` | `onSteer={onSteerHandler}`、`steerAvailable` **都已接** |
| 同文件 `:2355` | `disabled={requiresConversationReset \|\| !current_model?.use_model \|\| !modelSelection.isCurrentModelAvailable}` |

结论：

| 情形 | 拦不拦 |
| --- | --- |
| **执行繁忙**（正在跑） | **不拦** —— 走 steer 通道 |
| **云未就绪**（模型不可用 / 需重置） | **拦** |

### steer 通道已经完整

`executeSteer`（`NomiSendBox.tsx:1645-1704`）做了这些事：

1. `ipcBridge.conversation.steer.invoke({ input, conversation_id, files, idempotency_key: uuidv7() })`
2. 拿到 `res.msg_id` —— **编号**
3. `fresh` 时立刻 `addOrUpdateMessage(...)` —— **本地立刻上屏**
4. `replayed_in_flight` / `completed` 各有分支处理
5. 失败时 `removeMessageByMsgId(msg_id)` —— **回滚**

### 后端 steer 要求「有活跃 turn」

`crates/backend/nomifun-conversation/src/service.rs:14005`：

```rust
match runtime.steer(req.content.clone()) {
    Ok(true) => {}
    Ok(false) => return Err(AppError::Conflict(
        "the Agent turn ended before the steer was delivered")),
    Err(error) => return Err(error),
}
```

三条路各管一段：

| 状态 | 走哪条 | 能否落盘拿到编号 |
| --- | --- | --- |
| 有活跃 turn（忙） | `conversation.steer.invoke` | **能** |
| 空闲 + 模型可用 | 普通 `send` | 能 |
| **空闲 + 模型不可用** | **无路可走** —— `disabled` 直接拦住 | **不能** |

---

## 理解的目标 / 顺序 / 最大风险

1. **目标**：证明「忙时也能提交、拿到编号、重开还能找到」
2. **顺序**：文档落地 → 跑起来实测三条未验证点 → 只补真正缺的那一段 → 端到端脚本
3. **最大风险（已发生）**：**第一张开发单的验收场景可能已经通过。**
   审查要的是「A 还在跑慢工具时提交 B」——那正是「有活跃 turn」，走 steer，代码已完整实现。
   若实测确认，本轮价值不在写代码，而在：①给出可重跑的证明 ②文档落地 ③指出审查与代码的偏差
4. **真缺口（推断）**：无可用模型时消息无处可落。这正是「本地受理回执」的字面含义
5. **约束**：只在本分支 commit、不推送；不动既有 migration / `package.json` / CI / 既有测试断言

---

## 任务 1 · 文档落地 ✅

`docs/architecture/flowy-v0/` 现有 11 个文件：

```
最终方案.md  实现对照.md  证据.md  一页纸.md
参照.md      先记.md      审查交接.md  V0-执行计划.md
README.md（索引）  PROGRESS.md  BLOCKED.md
```

验收：`ls docs/architecture/flowy-v0/ | wc -l` → 11

---

## 未验证的三条（跑起来才能答）

1. `steerAvailable` 运行时是否恒为 true
2. 有活跃 turn 时点提交，UI 上出现的是「发送」还是「插话」按钮
3. 拿到 `msg_id` 后重开客户端，该消息是否真在列表里

---

## 下一步

- [ ] `bun install` 完成后补 `bun run typecheck` 基线
- [ ] 构建并启动应用（bash 后台，`Flowy.exe`），实测上述三条
- [ ] 按实测结果决定：验证已有通道，还是补「无模型时也能受理」
- [ ] 写端到端脚本 `accept.sh`
- [ ] commit（不推送）

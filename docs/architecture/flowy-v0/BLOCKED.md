# BLOCKED · 待裁决清单

> 拿不准的事写这里，跳过继续做别的。执行者不停下来等。

---

## 1. 审查报告与代码不一致：受理链路可能已经全通

**记录时间**：2026-11-23（任务 0）

**审查报告的原话**：

> 当前输入框在执行繁忙或云未就绪时拦截提交，pi 的 steering 回调尚未接线。

**我在 `ba5ace40a` 上读到的代码**：

| 位置 | 事实 |
| --- | --- |
| `ui/src/renderer/components/chat/SendBox/index.tsx:370` | `allowSendWhileLoading = false`（默认） |
| 同文件 `:2296` | 拦截条件 `!allowSendWhileLoading && (isLoading \|\| loading)` |
| `platforms/nomi/NomiSendBox.tsx:2532` | 传了 `allowSendWhileLoading` → **为 true** |
| 同文件 `:2523-2524` | `onSteer={onSteerHandler}`、`steerAvailable` **都已接** |
| 同文件 `:1645-1704` | `executeSteer` → `ipcBridge.conversation.steer.invoke(...)`，返回 `msg_id`，`fresh` 时立刻上屏，带 `idempotency_key`，失败回滚 |
| `BasicRuntimeSendBox.tsx:902`、`AcpSendBox.tsx:813` | 同样传了 `allowSendWhileLoading` |

**所以三个平台通道的「忙时提交 + 拿编号 + 立刻可见 + 幂等 + 回滚」看起来都已经实现。**

**我为什么还是标 BLOCKED，而不是直接宣布「已存在」**：

1. 我只读了代码，**没跑起来**。静态读出的「已接线」和运行时可用是两件事。
2. `steerAvailable` 是硬写的 prop，但它上游会不会被条件化，我没查。
3. `SendBox` 的 `disabled` prop 来自外部注入，**注入点还没追到** —— 「云未就绪」很可能走的是这条，而不是 `loading`。
4. 审查者是跑过的，我是读的。**跑过的证据权重更高。**

**三条路，需要裁决**：

- **A** 先追 `disabled` 注入点 + 把应用跑起来实测一次，再决定要不要动代码
- **B** 直接按「已验证存在」处理，本轮只补文档和端到端脚本
- **C** 不信代码、不信审查，重跑一遍最小复现

**我的判断**：走 **A**。因为它同时能回答「disabled 从哪来」和「运行时到底拦不拦」，成本比 C 低、比 B 稳。

---

## 2. 二进制名与任务书不符

任务书写 `./target/debug/flowy-app.exe`，**实际上游构建产物是 `Flowy.exe`**。

已在 `PROGRESS.md` 更正。如果还有别处引用旧名，一并改。

---

## 3. 首次构建耗时未知

worktree 内无 `target/`，`cargo check -p nomifun-ai-agent` 从零开始，耗时未测完。

**影响**：如果超过 30 分钟，后面的每次验证都要按这个节奏排，不能频繁全量。

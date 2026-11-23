# BLOCKED · 待裁决清单

> 拿不准的事写这里，跳过继续做别的。执行者不停下来等。

---

## 🔴 1. 严重：仓库里有两个项目，我第一轮调研错了对象

**记录时间**：2026-11-23（第 2 轮）

### 事实

这个仓库里有**两个互不相干的应用**：

| | 根 workspace | `flowy18_wip/`（嵌套独立 workspace） |
| --- | --- | --- |
| 技术栈 | Tauri + React | **GPUI（`gpui-ce`）+ pi-rust** |
| 包名 | `Flowy`（`apps/desktop/Cargo.toml`） | **`flowy-app`**（`crates/flowy-app`） |
| 产物 | `target/debug/Flowy.exe` | **`flowy18_wip/target/debug/flowy-app.exe`** |
| 与方案的关系 | **无关** | **本方案描述的就是它** |

**方案文档里写的是 pi-rust、GPUI、`flowy-agent-core`** —— 全部指向 `flowy18_wip`。

### 我第一轮做错了什么

我在**根 workspace** 里查了半天的「受理链路」：

- `ui/src/renderer/components/chat/SendBox/index.tsx`
- `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx`
- `crates/backend/nomifun-conversation/src/service.rs`
- `allowSendWhileLoading`、`executeSteer`、`NomiSendBox:2355` 的 `disabled`

**这些全都属于 Tauri/React 那个产品，跟 `flowy18_wip` 一点关系都没有。**

**已经写进 `PROGRESS.md` 的那一整节「受理链路：与审查报告不一致」，对本单无意义，需要重做。**

### 我上一轮的「二进制名更正」也是错的

我说任务书里的 `flowy-app.exe` 应改成 `Flowy.exe`。**反了。**

- `flowy-app.exe` **是对的** —— 它是 `flowy18_wip` 的产物
- `Flowy.exe` 是另一个产品的产物

`PROGRESS.md` 里那条更正已撤回。

### 启动命令的正确形态

`flowy18_wip` 有自己的 `target/`，所以相对路径的 cwd 是 `flowy18_wip/`：

```bash
cd flowy18_wip
(./target/debug/flowy-app.exe >/dev/null 2>&1 &)
```

主工作区里已有一个可用的旧二进制（`flowy18_wip/target/debug/flowy-app.exe`），但**它不在我的 worktree 里**——worktree 内的 `flowy18_wip/target/` 是空的，要冷构建。

### 需要裁决

- **默认走法（我替领导拍的板）**：本单的代码目标就是 `flowy18_wip/`，根 workspace 一行不碰。已在构建 `flowy-app`。
- 如果本单其实要改 Tauri 那个产品，**请纠正我**，我把目标整体换过去（那时 `Flowy.exe` 才是对的）。

### 为什么这条必须先记

**这正是我自己在 §11.23b 写下的那个病：「我测的全是我想到的那条路」。**
这次更糟——我连「哪条路」都选错了对象。**如果不记下来，下一轮就会在这个错误前提上继续写代码。**

---

## 2. 该看的是 `flowy18_wip` 里哪几个 crate

本单要动的三处，在 `flowy18_wip` 里分别对应（**待逐条核实**）：

| 要改的东西 | 根 workspace（错） | 应该是（待核实） |
| --- | --- | --- |
| 输入框 / 提交 | `ui/src/renderer/...` | `crates/flowy-chat` 或 `crates/flowy-ui` |
| 会话受理 / 回执 | `crates/backend/nomifun-conversation` | `crates/flowy-agent` / `crates/flowy-core` |
| pi-rust loop | — | `crates/flowy-agent-core` |

**下一轮第一件事：把这张表逐条落实成「文件:行号」，核实完才准写代码。**

---

## 3. 首次冷构建

- 根 workspace `cargo check -p nomifun-ai-agent`：**4m57s**（已测，但属于错对象）
- `flowy18_wip` `cargo build -p flowy-app`：**进行中，耗时未知**

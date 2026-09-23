# Flowy 新旧版本架构演进与 Windows 终端输出（STDOUT）截断问题排查及修复指南

本文档面向团队研发与技术支持同学，详细说明 Flowy 新旧版本架构的演进差异、近期用户反馈的 Windows 终端执行返回空 STDOUT 问题的根因剖析以及完整的修复方案与排查建议。

---

## 一、 Flowy 新旧版本架构演进对比

为了彻底解决跨平台资源消耗大、本地端口依赖重、多进程生命周期管理复杂等历史问题，Flowy 完成了从早期基于 Electron 的包装架构到全新 Rust 原生微内核架构的全面重构。

### 1. 新老架构核心差异对照表

| 维度 | 旧版本 Flowy（基于 Electron / OpenClaw） | 新版本 Flowy（当前版本：Tauri 2 + Rust） |
| :--- | :--- | :--- |
| **底层核心** | Electron + Node.js 运行时 + 本地 Gateway | Tauri 2 + Rust 原生后端（axum + tokio + SQLite） |
| **前端架构** | Web 页面包裹在 Electron BrowserWindow | React 19 + TypeScript + Vite 6 单页应用（SPA） |
| **命令执行引擎** | 依赖 Node.js `child_process` 猴子补丁（monkey-patch） | Rust 原生进程运行时（`nomi-process-runtime`） |
| **命令拦截/鉴权** | 依赖本地 `18789` 端口网关服务 + 状态文件通信 | Rust 微内核原生调度与内存安全沙箱，**无需开放端口** |
| **状态记录机制** | 依赖磁盘文件 `terminal-command-interception-state.json` | 内存化会话注册表（Session Registry）与数据库持久化 |
| **进程管理/防泄漏** | Node.js `tree-kill` / 轮询查找子进程 | Win32 原生 `JobObject` / POSIX `setpgid` 原生进程树托管 |
| **运行模式** | 单一桌面端模式 | 支持 **Desktop 桌面模式** 与 **Web 无头/云端服务器模式** 双宿主 |

---

### 2. 旧版本 Flowy 架构回顾（历史实现）
- **实现机制**：旧版本在启动时通过 `fetch-preload.cjs` 对 Node.js 底层的 `child_process.spawn` / `exec` 进行拦截，将终端执行请求转发至本地常驻的 OpenClaw 网关服务（默认监听本地 `18789` 端口）。
- **状态同步**：通过在用户数据目录下读写 `terminal-command-interception-state.json` 记录 `gatewayReady` 与 `protectionArmed` 等状态。
- **痛点与隐患**：
  - 极易受本地端口冲突、防火墙拦截、杀毒软件阻断影响；
  - 若网关服务崩溃或端口未就绪，会导致所有终端工具调用被静默阻塞；
  - 跨进程状态文件读写存在竞态条件与陈旧状态残留。

---

### 3. 新版本 Flowy 架构特点（当前实现）
- **去端口化与高内聚**：新版本将所有 Agent 核心调度、终端执行、文件 I/O、数据库存储下沉至 Rust 原生层，**彻底废弃了 18789 端口及外部拦截脚本**，完全无需在本地开放任何额外命令网关端口。
- **Win32 原生进程运行时**：
  - 在 Windows 平台，通过 Win32 API 创建带有 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 标志的 Windows JobObject，主进程或会话结束时，操作系统内核级自动级联回收整个进程树（包括所有子进程与孙进程），从根本上杜绝孤儿进程与句柄泄漏。
  - 支持 **Piped Transport（非交互式流管道）** 与 **ConPTY Transport（伪终端交互模式）**，兼顾高吞吐数据流与 TUI 复杂终端应用。

---

## 二、 用户反馈问题排查与根因剖析

### 1. 现象描述
部分用户在升级新版本客户端后反馈如下现象：
1. 在 Windows 平台上使用 `Bash` 或 `exec_command` 执行命令时，工具返回 **Exit Code 0**，但 **STDOUT 为空**（即使执行 `echo 123` 或 `dir` 也无输出）；
2. 用户在命令末尾加上重定向（例如 `dir > out.txt`）时，`out.txt` 文件中内容完全正常；
3. 用户在排查日志时发现存在 `terminal-command-interception-state.json`，且显示 `gatewayReady: false, protectionArmed: false`，同时发现本地没有监听 18789 端口，并观察到有两个 Flowy 进程在运行。

---

### 2. 误区澄清与干扰项排除
- **关于 18789 端口与 JSON 状态文件**：
  用户日志中提及的 `terminal-command-interception-state.json` 文件的最后修改时间（mtime）早于软件本次启动时间，这属于**旧版 Electron 遗留的历史文件**。新版本 Flowy 完全不再使用 18789 端口与该文件，因此该日志属于干扰信息，并非导致本次无输出的原因。
- **关于双进程现象**：
  Tauri 2 架构下，桌面应用包含一个主进程（负责窗口与系统托盘）以及一个后端服务/渲染进程，两个进程运行属于正常现象。

---

### 3. 真实 Bug 根因：PowerShell 管道输出截断与过早退出
通过对 Rust 进程运行时模块（`nomi-process-runtime`）的代码深入追踪，定位到了根本原因：

在 Windows 平台下，为了保证多行脚本与环境变量的正确执行，管道传输模式默认使用 PowerShell 宿主包裹用户脚本。原先的 PowerShell 包装器代码如下：

```powershell
# [问题代码]
$nomifunBlock = [scriptblock]::Create($nomifunScript)
& $nomifunBlock
exit 0
```

#### 为什么会导致空 STDOUT？
1. **PowerShell 管道对象机制**：PowerShell 中的 `& $scriptblock` 执行后产生的是 .NET 管道对象流（Pipeline Objects），而非直接写出到标准输出的文件描述符。这些对象需要经过 PowerShell 的默认格式化器（`Out-Default`）转换为文本字节流并写入 OS stdout 管道句柄。
2. **`exit 0` 强行终止宿主**：紧随其后的 `exit 0` 会在 PowerShell 管道格式化器完成文本渲染、以及底层 .NET Console / Win32 标准输出缓冲区 Flush 之前，**直接强制终止 `powershell.exe` 进程**。
3. **管道缓冲区被内核销毁**：进程强制退出导致操作系统底层的匿名管道句柄被关闭，尚未排空的输出缓冲区直接丢失。因此外层的 Rust 运行时读取到了 EOF，拿到了 Exit code 0，但 STDOUT 字节数为 0。
4. **重定向为什么有效**：当用户执行 `dir > out.txt` 时，文件写入是由操作系统直接将数据写到磁盘文件，避开了 PowerShell 标准输出管道到宿主进程的流转过程，因此文件中有内容。

---

## 三、 修复方案与技术实现

针对上述根因，对 `nomi-process-runtime` 及相关工具模块进行了系统性修复：

### 1. 显式管道格式化与强制流刷新
在 `crates/shared/nomi-process-runtime/src/platform/windows.rs` 中重构了 PowerShell 的执行封装：

```powershell
# [修复后代码]
$nomifunBlock = [scriptblock]::Create($nomifunScript)
& $nomifunBlock | Out-Default
[Console]::Out.Flush()
[Console]::Error.Flush()
```
- **`| Out-Default`**：显式将命令执行的所有输出对象流送入默认输出格式化器，确保管道数据完全转换为文本流。
- **`[Console]::Out.Flush()` & `[Console]::Error.Flush()`**：显式刷新标准输出与错误流的底层缓冲区，确保所有字节落入 OS 管道。
- **移除提前的 `exit 0`**：允许 PowerShell 脚本正常排空退出，既保证了管道完整性，又保留了用户脚本自身返回的 Exit Code 及异常捕获逻辑。

---

### 2. 跨平台工具层与测试契约防回归
1. **新增契约测试**：在 `crates/shared/nomi-process-runtime/tests/process_contract.rs` 中新增 `windows_powershell_pipe_transport_captures_stdout_and_cmdlet_output` 测试，覆盖：
   - PowerShell Cmdlet 输出（如 `Write-Output`）
   - Native 原生命令输出（如 `cmd /c echo`）
   - 多行连续输出与换行完整性
2. **Python 探针清理宽限与预算再平衡**：在 `crates/agent/nomi-tools/src/exec_command.rs` 中放宽 Python 解释器探针（script 模式启动前依次探测 `py` / `python3` / `python` 候选）的终止与回收宽限（terminate 50ms / reap 500ms），避免高负载下探针进程被误判为失控；同时将探针总窗口从 2s 提升至 3s（`PYTHON_PROBE_MAX`），保证 3 个候选并存时每个候选仍有约 425ms 的执行时间片——若沿用 2s 总窗，清理预算（575ms）会把每个候选的执行时间压缩到约 91ms，冷启动解释器会被误杀并误报 `python_unavailable`。

---

## 四、 验证结果

修复后已在本机 Windows 环境完成以下验证（2026-09，实测结果）：

- ✅ **`cargo test -p nomi-process-runtime`**：全量通过（共 186 项，含本次新增的管道输出契约测试 `windows_powershell_pipe_transport_captures_stdout_and_cmdlet_output`，以及钉住退出码语义的既有契约 `windows_powershell_preserves_final_native_and_pipeline_status`）。
- ⚠️ **`cargo test -p nomi-tools --lib`**：319 通过 / 7 失败。7 个失败全部为 `worktree::tests` 的 git CRLF 断言差异（本机 Windows git 环境的存量问题，所涉模块本次未改动）；另 `bash::tests::timeout_cleans_the_shell_process_and_its_marker_grandchild` 存在并行负载 flake，单独运行与重跑均可通过。以上均与本次改动无关。
- ✅ **PowerShell 退出码语义实测**：成功路径（尾部 Flush 后）进程退出 0；`cmd /c exit 7; Write-Output recovered` 退出 0；`Write-Output x; cmd /c exit 7` 由内联 `exit $LASTEXITCODE` 保留退出 7。
- ✅ **全量仓储规则门禁**：
  - 进程运行时边界检查（`check:process-runtime-boundary`）通过；
  - 浏览器平台边界检查（`check:browser-platform-boundary`）通过；
  - Windows 控制台黑框隐藏契约检查（`check:windows-console-hide`）通过；
  - 错误与支持矩阵契约（`check:error-surface-contract`）通过。

---

## 五、 技术支持与研发排查指南

后续若遇到类似终端执行问题，请按以下步骤快速定界：

1. **版本确认**：
   - 确认用户安装的是新版 Flowy（基于 Tauri 2，主程序为原生可执行文件），无需理会任何旧版 18789 端口或 `terminal-command-interception-state.json`。
2. **模式确认（Pipe vs PTY）**：
   - **Windows（Agent 工具）**：`exec_command` / Bash 一律使用 Pipe Transport（规避 ConPTY 控制台黑窗闪烁），`tty` 参数会被静默忽略；交互式用户终端走独立的 ConPTY 路径，与 Agent 工具无关；
   - **macOS / Linux（Agent 工具）**：普通脚本/编译/检查命令走 Pipe Transport（`tty: false`，默认），检查输出是否包含标准流格式化；交互式命令/TUI 工具（如 vim、top、交互式 REPL）需启用 PTY 模式（`tty: true`）。
3. **输出重定向与编码**：
   - Windows 默认代码页非 UTF-8（如 GBK/CP936），新版 Flowy 会自动通过活动代码页进行流式字符解码与无损 UTF-8 转换，排查时可关注是否存在非标准二进制流破坏终端格式。

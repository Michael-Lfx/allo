# MXC 进程包装层可行性——最小验证清单

> **最后维护：** 2026-09-21 · 性质：**只读验证清单**（不含生产代码改动）
> 归属：`agent-harness-permission-approval-sandbox.zh.md` §5.1 D 组的前置验证
>
> **✅ 五门已全部实测（2026-09-21，Windows 11 build 29671 + MXC `ca7ea12`）。
> 完整证据见 [`agent-harness-mxc-verification-record.zh.md`](agent-harness-mxc-verification-record.zh.md)。**
>
> | 门 | 结论 |
> |---|---|
> | D-A | **❌ 阻断成立** —— 外层 Job 带 UI 限制时 `processcontainer` 每次 spawn 都失败 `ERROR_NOT_SUPPORTED` |
> | D-B | **⚠️ 只到 wrapper** —— 沙箱内进程是另一个 PID；必须补沙箱会话级存活探针 |
> | D-C | **✅ 0 存活** —— 强杀 wrapper 不遗留沙箱内进程树（机制未定，依赖前应补对照） |
> | D-D | **❌ 不可分型** + **🔴 只读策略在 Tier 1 主机上不生效**（沙箱内可读用户主目录） |
> | D-E | **⚠️ 放弃 P1，转 P2** —— state-aware 有 `windows_sandbox` 可用，非仅 `isolation_session` |
>
> **总判定：不能走"每命令包装 `wxc-exec`"（P1）。**
> 另有两个与退路选择无关、但同样阻断的前置：**只读策略失效** 与
> **默认 `ui.disable` 会打死 Node/.NET/pwsh 等原生运行时**。
> 下文 §一 保留实验前的推断原貌；§三 各门的判定已按实测更新。
>
> **目的：** 在投入 C 组（策略作者层）与 F 组（残留物治理）之前，先确定
> `wxc-exec.exe` 能否作为**透明包装层**接入 `nomi-process-runtime` 现有的
> Job Object + `ExactProcessIdentity` 生命周期。
>
> **为什么必须先做：** 这个问题的答案决定接入方案是"改命令构造"还是"重建一套进程身份与清理语义"。
> 两者工作量差一个量级，且如果都不可行，强制形态的方案要整体重做——那时 C / F 的投入已作废。
>
> **证据分级：** 【源】本仓库源码 · 【MXC】MXC 源码（sparse clone，`main`）· 【官】一手官方文档 · 【验】本文档待执行的验证

---

## 一、已定位的阻断点（读这一节即可理解为什么需要这份清单）

### 1.1 事实链

| # | 事实 | 出处 |
|---|---|---|
| F1 | DSH 为每个受管子进程创建 Job Object，设 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，并把子进程 `AssignProcessToJobObject` 进去 | 【源】`nomi-process-runtime/src/platform/windows.rs:1670-1695`、`:200`/`:290`/`:419`/`:1100` |
| F2 | DSH 的 `arm_process_job` **只设** `KILL_ON_JOB_CLOSE`，**未设任何 breakaway 限制**（`JOB_OBJECT_LIMIT_BREAKAWAY_OK` / `SILENT_BREAKAWAY_OK`） | 【源】`platform/windows.rs:1677-1695` |
| F3 | MXC 的 `processcontainer` 后端自己创建 Job Object、设 UI 限制、把**仍挂起**的子进程分配进去；失败即 `TerminateProcess` 并返回错误 | 【MXC】`process_container/common/src/appcontainer_runner.rs:1186-1206`，UI 限制在 `:1192`，分配在 `:1193` |
| F4 | MXC 的 `UiJobObject::new()` 用 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，随后 `set_ui_limits` 调 `SetInformationJobObject(JobObjectBasicUIRestrictions)` | 【MXC】`process_container/common/src/job_object.rs:235-287` |
| F5 | MXC 创建子进程用 `CREATE_SUSPENDED \| EXTENDED_STARTUPINFO_PRESENT`，属性表里只有 `SECURITY_CAPABILITIES` / `ALL_APPLICATION_PACKAGES_POLICY` / `MITIGATION_POLICY` / `HANDLE_LIST`——**没有** `PROC_THREAD_ATTRIBUTE_JOB_LIST` | 【MXC】`appcontainer_runner.rs:1123-1144`、`:858-1040` |
| F6 | 官方文档：若一个已在 job 中的进程被加入另一个 job，"the jobs are nested **by default if the system can form a valid job hierarchy and neither job sets UI limits**" | 【官】[Nested Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs) |

### 1.2 推断

```
DSH 启动 wxc-exec.exe  →  F1：wxc-exec.exe 进入 DSH 的 Job
MXC 沙箱化子进程        →  F3：子进程隐式继承 DSH 的 Job（F2 无 breakaway）
MXC 分配自己的 Job      →  F6：MXC 的 Job 设了 UI 限制（F4）→ 无法形成嵌套
                        →  AssignProcessToJobObject 失败
MXC 的错误路径          →  F3：TerminateProcess + 返回错误
```

**结论（待验证）：`processcontainer` 后端在 DSH 的 Job 内可能每次 spawn 都失败。**

### 1.3 必须区分两种"失败"

| 情形 | 现象 | 含义 |
|---|---|---|
| **硬失败** | `AssignProcessToJobObject` 返回 `ERROR_ACCESS_DENIED`，MXC 终结子进程并报错 | 阻断成立，必须选退路 |
| **静默降级** | 分配"成功"但作业语义不同（例如实际形成了嵌套或走到了别的路径） | **更危险**——需要 D-C 确认清理覆盖面真的成立 |

**D-A 的验证必须能区分这两者**，不能只看"命令有没有跑起来"。

---

## 二、验证环境

| 项 | 要求 |
|---|---|
| OS | **Windows 11 24H2 (build 26100) 或更高**。`processcontainer` 的 Tier 1 需要 `26100`；`isolation_session` 需要 Insider `26340.9212`【官】 |
| MXC 版本 | 记录 commit（本文引用的是 `main` sparse clone）；**schema 与 SDK 版本必须对齐**，0.8 与 0.6/0.7 的网络字段不兼容【官】 |
| MXC 构建 | 按 repo 的 `build.bat`（Windows 发布构建）；产出 `wxc-exec.exe` 与 `@microsoft/mxc-sdk`【官】 |
| 后端选择 | 默认 `processcontainer`；其余（`windows_sandbox` / `wslc` / `microvm` / `isolation_session` / `hyperlight`）需 `--experimental`【官】 |
| DSH 侧 | 只读验证即可：`capability.rs` / `platform/windows.rs` / `recovery.rs` 的既有形态；**本阶段不改生产代码** |
| 隔离要求 | 若用 `--audit`：**只在隔离开发机 / CI**。`--audit` 期间 AppContainer 限制不被强制【官】 |

**先记录版本指纹**（后续所有结论都绑定它）：

```powershell
wxc-exec.exe --version
wxc-exec.exe --probe          # UI 限制可执行性矩阵（supported_ui_limit_mask 的实测来源）
```

---

## 三、验证门

### D-A · 沙箱化进程能否加入调用方 Job 层次

> **假设：** 在 DSH 的 Job 内启动 `wxc-exec.exe` 后，MXC 能否成功为沙箱内子进程建立它的 Job 归属？
>
> **卡住谁：** 决定 D 组是"改命令构造"（D-1）还是"重建身份与清理语义"（D-2）。**最关键的一门。**

**方法一（最小、先做）：观察 MXC 自己的错误**

在 DSH 的 Job 语义**内**与**外**各跑一次同一命令，比较 MXC 的输出：

| 场景 | 做法 | 期望（若阻断成立） |
|---|---|---|
| Job 内 | 由 DSH 的 `ProcessSupervisor` 路径启动（受管，进 DSH 的 Job） | MXC 在 job 分配处失败并报 `AssignProcessToJobObject` 相关错误 |
| Job 外 | 直接在 shell 里跑同一个 `wxc-exec.exe config.json -- <cmd>`（不进 DSH 的 Job） | 正常执行 |

**若两者行为不同（Job 内失败、Job 外成功）→ 阻断确认。**
若两者都成功 → 说明**不是**走 `AssignProcessToJobObject` 那条路，继续方法二确认实际归属。

**方法二（决定性）：检查目标进程的实际 Job 归属**

用 `IsProcessInJob` 查询沙箱内真实进程的即时 job，以及它是否在 DSH 的 job 链上：

```
IsProcessInJob(hProcess_in_sandbox, dsh_job, &result)
QueryInformationJobObject(dsh_job, JobObjectBasicProcessIdList, ...)
```

| 观测 | 判定 | 含义 |
|---|---|---|
| 沙箱进程 **在 DSH 的 job 链上** 且 MXC 的 job 是它的即时 job | ✅ **D-1 成立** | 嵌套形成；DSH 的 `TerminateJobObject` 覆盖面延伸到沙箱内 |
| 沙箱进程 **不在** DSH 的 job 链上 | ⚠️ **D-2** | 有 breakaway 或独立归属，清理语义已变 |
| MXC 报 job 分配失败 | ❌ **阻断成立** | 必须选退路 |

**通过标准（D-1）：** 沙箱内真实进程在 DSH 的 job 链内，`TerminateJobObject(dsh_job)` 能终结它，
且 MXC 的 UI 限制仍然生效（两者都要，不能只顾一个）。

**🛑 实测结论（2026-09-21）：❌ 阻断成立 —— 走"MXC 报 job 分配失败"这一行。**

| 场景 | 外层 Job | 结果 |
|---|---|---|
| `wxc-exec` 独立运行 | 无 | ✅ `WXC_RAN_OK`，exit 0 |
| `wxc-exec` 在 Job 内（`KILL_ON_JOB_CLOSE` **+ UI 限制**） | `0x10` | ❌ exit `0xFFFFFFFF`，`CreateProcessW failed with error code 50` |
| `wxc-exec` 在 Job 内（`KILL_ON_JOB_CLOSE` **无 UI 限制**） | 无 | ✅ `WXC_RAN_OK`，exit 0 |

内核层已独立复现同一机制（不依赖 MXC）：子进程的即时 Job UI 限制为 `0x0` 时
`AssignProcessToJobObject(self, own_job)` 成功；为 `0x10` 时返回 **win32=50 `ERROR_NOT_SUPPORTED`**。
与官方文档 "neither job sets UI limits" 条款方向一致。

**→ 因此 R1 上升为最直接可行的一条，R4（长驻沙箱 / 换后端）优先级上升，
R2 仍不推荐。详见验证记录 §六。**

**退路（按代价排序）：**

| 退路 | 做法 | 代价 / 代价的另一面 |
|---|---|---|
| **R1 · DSH 让出 Job** | DSH 对"沙箱化会话"不创建自己的 Job，改为依赖 MXC 的 job + `KILL_ON_JOB_CLOSE` | 需重做"进程树清理已证明"的语义；且 wrapper 崩溃时 DSH 的兜底消失 |
| **R2 · 让子进程 breakaway** | DSH 的 job 允许 breakaway，MXC 创建时带 `CREATE_BREAKAWAY_FROM_JOB` | 沙箱树**脱离** DSH 的 Job → DSH 的 `TerminateJobObject` 不再覆盖它。**净损失**，只在 MXC 侧能保证清理时可接受 |
| **R3 · 立 MXC 支持嵌套** | 要求 MXC 不在 job 上设 UI 限制，或改用 `PROC_THREAD_ATTRIBUTE_JOB_LIST` 在创建时挂多个 job | 需要上游改动；`PROC_THREAD_ATTRIBUTE_JOB_LIST` 在 `winnt.h` 中存在但**官方文档未记载**，可行性未知【验】 |
| **R4 · 换后端 / 换形态** | 用 `windows_sandbox`（VM 级，进程在一个独立系统里，与 DSH 的 Job 无关）或 MXC 的 state-aware 生命周期（沙箱作为长驻服务，而非每命令包装） | 前者重（整 VM）；后者绕开"每命令 job 冲突"但需重写调用形态。见 D-E |

**⚠️ 不要选"R2 + 以为没事"**：breakaway 之后 DSH 的清理证明**不再成立**，
而 `ChildProcessCleanup` 的整个契约是建立在"清理已证明"上的（`command_builder.rs:518-531`）。

---

### D-B · `ExactProcessIdentity` 捕获到的是谁

> **假设：** DSH 拿到的 child PID 是 `wxc-exec.exe`（wrapper），还是沙箱内真实进程？
>
> **为什么与 D-A 不同：** 即使 D-A 通过（清理覆盖面成立），
> `ExactProcessIdentity` 仍可能指向 wrapper，使孤儿检测与 PID-reuse 防御**静默失真**——
> **测试可能依然全绿**，这是最难发现的破坏。

**方法：** 沿 `recovery.rs:87` 的 `capture_child_identity` 与 `:120` 的 `probe_process_identity`
核对捕获到的 PID / 创建时间，与以下三者比对：

1. `wxc-exec.exe` 的 PID（DSH 的直接子进程）
2. 沙箱内真实进程的 PID（MXC 的报告 / 进程树枚举）
3. `ChildProcessCleanup` 实际持有的句柄

| 观测 | 判定 | 后果 |
|---|---|---|
| wrapper 与沙箱进程 **同一 PID** | ✅ 透明 | 既有语义不变 |
| 不同 PID，且 DSH 只持 wrapper | ⚠️ 半透明 | 沙箱内进程的孤儿检测**有缺口**；PID-reuse 防御只覆盖 wrapper |
| 不同 PID，且 DSH 能拿到沙箱内进程身份 | ⚠️ 需改契约 | 身份捕获要改，但可行 |

**通过标准：** DSH 持有的身份能唯一标识"整个沙箱会话"，并且该身份的存活/退出
与沙箱内进程树的存活/退出**一一对应**。

**⚠️ 实测结论（2026-09-21）：落到第 2 行 —— "不同 PID，且 DSH 只持 wrapper"。**

| 观测 | 实测值 |
|---|---|
| DSH 直接子进程（`wxc-exec.exe`） | pid **42544**（`capture_child_identity` 拿到的是这个） |
| 沙箱内真实进程 | pid **45596**，其 `ppid` = **42544**（即它是 wrapper 的子进程） |
| 沙箱内进程在调用方 Job 链上吗 | **在**（出现在 `JobPids` 采样里，`in_job=1`） |
| 沙箱内**自己看到的**即时 Job UI 限制 | **`0x0`** —— MXC 为它建了自己的无 UI 限制 Job，且它继承了调用方 Job |

**→ D-B 的退路成为必需：必须补一个"沙箱会话级"的存活探针，
否则 `recovery.rs` 的孤儿回收对沙箱会话失效。**

**退路：** 若只能拿到 wrapper 身份，则必须补一个"沙箱会话级"的存活探针
（例如查询 MXC 的作业/会话状态），否则 `recovery.rs` 的孤儿回收对沙箱会话失效。

---

### D-C · Job 的 `KILL_ON_JOB_CLOSE` 覆盖面

> **假设：** DSH 的 Job 关闭/终结时，沙箱内的整个进程树会被终结吗？
>
> **为什么必须单独验：** DSH 现在依赖 Job 作为"进程树清理已证明"的载体。
> 如果覆盖面只到 wrapper，则 wrapper 被杀后沙箱内的进程**可能存活**——
> 而 `taskkill /T` 之类之所以危险，正是因为它在补这个洞。

**方法：** 在沙箱内启动一个会派生子进程的命令（`cmd /c start /b ...` 或 node 脚本 fork），
然后触发 DSH 的强制终结路径（`force_kill` / `TerminateJobObject`），检查：

1. 沙箱内子进程是否退出
2. 是否存在游离进程（沙箱内、但不在任何 DSH 可见的 job 里）
3. `QueryInformationJobObject(JobObjectBasicAccountingInformation)` 的
   `ActiveProcesses` 是否归零——**DSH 已用它做"job 已空"的判定**（`platform/windows.rs:2026-2034`）

| 观测 | 判定 |
|---|---|
| 全部退出，`ActiveProcesses` 归零 | ✅ 覆盖面成立 |
| 有游离进程 | ❌ 清理证明**失效**，必须补 |
| `ActiveProcesses` 归零但系统里仍有沙箱进程 | ❌❌ **最坏**：计数器撒谎，DSH 的"已证明"变成假证明 |

**通过标准：** 三种观测下都无游离进程，且 `ActiveProcesses` 与实际系统状态一致。

**✅ 实测结论（2026-09-21）：通过 —— 0 survivors。**

```
[kill] job contents before force-kill: [1840,26440,46028]    <- wrapper + 沙箱内 PS + 孙进程
[kill] === FORCE-KILL the direct child (wrapper) ===
[kill] wrapper exit code = 4294967295
[kill] job contents after wrapper kill: []                    <- Job 已空
[kill] --- liveness while job still OPEN ---  全部 gone
[kill] --- liveness AFTER job close ---       全部 reaped, survivors = 0
```

三种观测都过：沙箱内子进程退出、无游离进程、Job 进程表归零且系统里确实无残留
（"计数器撒谎"的最坏情况**未发生**）。

**⚠️ 但机制未定：** 强杀 wrapper 时沙箱内进程全部消失，无法区分是
(a) MXC 自身的 `KILL_ON_JOB_CLOSE`、(b) DSH 的 Job 兜住、还是 (c) OS 连带终止子进程。
**对 DSH 的结论相同（无孤儿），但若要依赖它，应补一次"只关 DSH 的 Job、不杀 wrapper"的对照。**

**退路：** 在 `ProcessSupervisor` 的终结路径上增加"沙箱后端专用"的终结点
（调 MXC 的 stop/deprovision，或用 MXC 自己的 job 作为清理载体），
并让 `ChildProcessCleanup` 的"已证明"语义包含它。

---

### D-D · 运行时拒绝能否与启动失败区分

> **假设：** 沙箱在**进程已启动后**拦截系统调用（例如越界写、出网）时，
> DSH 能否把它和"spawn 失败 / wrapper 崩溃"区分开？
>
> **为什么是 D 组而不是 E 组：** E 组负责**错误形状**，但前提是"运行时拒绝"这种信号
> **首先得能从 MXC 侧拿到**。现有 `enforce_sandbox`（`platform/windows.rs:2874-2886`）
> 只做 spawn **前**的静态拒绝，没有为运行时拒绝留通道。

**方法：** 构造一个**策略允许启动、但运行中越界**的命令（沙箱内写 `readonlyPaths` 之外的路径、
或 `egress: deny` 下发一个请求），然后观察：

| 观测 | 需要区分开的三种情况 |
|---|---|
| 退出码 | 沙箱拒绝 vs 程序自身失败 vs wrapper 失败 |
| stderr 内容 | MXC 是否输出可识别的拒绝信息；是否与 `denials.json` 的分类对应 |
| `denials.json` | `--audit` / `captureDenials` 模式下能否拿到**结构化**的 access/resource 分类 |

**通过标准：** 能稳定地判定"这是沙箱策略拒绝"，并拿到至少一个稳定的分类码——
足以支撑 E 组的 `RetryDecision`（策略拒绝 → `Retryable`；后端不支持 → `Fatal`）。

**❌ 实测结论（2026-09-21）：不通过 —— 拒绝生效，但不可分型。**

拒绝本身是真的（净室复现：删文件后重跑，`blocked\` 与 `System32` 的写入均未落盘，
`scratch\` 的写入落盘）。但**表面信息只有普通 Windows 拒绝**：

| 操作 | 沙箱内表现 | 事后文件 |
|---|---|---|
| 写 `scratch\`（`readwritePaths`） | `OK` | ✅ 存在 |
| 写 `blocked\`（未列出） | `CLR:UnauthorizedAccessException` | ❌ 不存在 |
| 写 `System32` | `CLR:UnauthorizedAccessException` | ❌ 不存在 |
| `cmd` 子进程写 `blocked\` | 文本 `Access is denied.`，exit 1 | ❌ 不存在 |

**判定：沙箱拒绝与"程序自己没权限"在子进程错误面上完全同形** ——
无专用标志、无稳定分类码、退出码也不区分（子进程正常 `return 0`）。
`denials.json` / `captureDenials` 本轮未验证（`--audit` 需隔离机/权限）。

**🔴 同时发现一条与 D-A 无关的更重阻断：只读路径策略在 Tier 1 主机上不生效。**

```
READ1=OK  READ2=OK  LIST=OK  SYSTEM_READ=OK  HOMEDIR_READ=OK   <- 读到了 C:\Users\15165\.gitconfig
```

即 `readwritePaths` / `readonlyPaths` **对"读"没有约束力**，被约束的只有"写"。
DSH 的 Bash 工具链必须能读工作区外路径（`~/.cargo/registry`、`%Bun%` 缓存、`%TEMP%`），
而那里恰好有 `~/.cargo/credentials.toml`、`~/.gitconfig`、`~/.ssh`。
**网络隔离因此从"纵深防御的一层"变成"唯一防线"**（本轮实测 `egress: deny` 生效）。

**退路：** 若 MXC 只给出非结构化 stderr，则 DSH 侧无法可靠分型，
必须退化为"越界即 `Abort`（停下等人）"的保守口径——**可用但会伤完成率**。

---

### D-E · 退路方案是否更好（形态决策）

> **假设：** 如果 D-A 只能走 R1–R3，那么"每命令包装 `wxc-exec`"这个形态本身是否就该放弃？
>
> **这一门不验证 MXC 行为，是决策门**——它的结论会反过来决定 D-A 的退路选择。

**要比较的两个形态：**

| 形态 | 描述 | 与 DSH Job 的关系 |
|---|---|---|
| **P1 · 每命令包装** | 每次 spawn 把命令包成 `wxc-exec.exe config.json -- <cmd>` | **冲突点就在这里**（D-A） |
| **P2 · 长驻沙箱会话** | 用 MXC 的 state-aware 生命周期（`provision` → `start` → `exec` → `stop` → `deps provision`）把沙箱作为一个**长驻服务**，DSH 通过它执行命令 | 沙箱进程不在 DSH 的每命令 Job 内，冲突消失 |

**P2 的代价（必须一并评估，否则是"看起来解决了"）：**

- 沙箱成为**有状态的长期资源**：需要生命周期管理、崩溃恢复、并发访问控制
  （而 DSH 的 `ProcessSupervisor` 已经是这样一套东西——两套生命周期要合并还是并存？）
- **`exec` 是否还返回可用的 stdio / 子进程身份**？若返回的是一个会话内的句柄而非 PID，
  D-B 的问题会以另一种形式回来
- ~~今天只有 **Isolation Session** 后端实现了 state-aware 生命周期~~ —— **此条已被源码证伪，见下**

**⚠️ 实测结论（2026-09-21）：放弃 P1，转 P2 评估。**

**先纠正一条此前的转引错误：state-aware 不止一个后端。**

| 事实 | 位置 |
|---|---|
| `impl StatefulSandboxBackend for IsolationSessionRunner` | `src/backends/isolation_session/common/src/state_aware.rs:79` |
| **`impl StatefulSandboxBackend for WindowsSandboxRunner`** | `src/backends/windows_sandbox/lifecycle/src/state_aware.rs:603` |
| **`impl StatefulSandboxBackend for WslcStateAwareRunner`** | `src/backends/wslc/common/src/state_aware.rs:53` |
| SDK 联合类型 | `sdk/node/src/state-aware-types.ts:22-25` → `'isolation_session' \| 'windows_sandbox' \| 'wslc'` |
| 契约版本 | `STATE_AWARE_VERSION = '0.9.0-alpha'` |

因此"P2 会把后端锁死在 `isolation_session`"这条代价**不成立**：
`windows_sandbox` 也在其中，而它**不经过"每命令套 Job"**（沙箱在独立 VM 内）。

**本机可用性（`--probe`）：** `isolationSessionAvailable: **false**` ·
`hyperlightAvailable: false` · `windows_sandbox` 需 `--experimental` 且需 Windows Sandbox 功能 ·
`wslc` 需 WSL2 + 预拉镜像。
**⇒ 本机 P2 只能走 `windows_sandbox`（VM 级，重）。**

| 形态 | 判定 |
|---|---|
| **P1 · 每命令包装** | ❌ **否决**：D-A 阻断；即便 R1 让出 Job，D-B 仍只到 wrapper、D-D 仍不可分型 |
| **P2 · 长驻沙箱会话** | ⚠️ **唯一出路**，架构可行；本机只能走 `windows_sandbox`，且引入"两套生命周期如何共存"的新问题 |
| **R1 · DSH 让出 Job** | ⚠️ 可解 D-A（§三 已证外层 Job 无 UI 限制时正常），但**解决不了 D-D 的不可分型**，且放弃 DSH 自身清理兜底 |

**通过标准（决策）：** 明确选 P1 或 P2，并写明另一个形态被否决的**具体理由**，
而不是"P1 有阻断所以退到 P2"。P2 必须独立成立。

---

## 四、执行顺序与判定矩阵

```text
D-A（job 归属）────┬─→ 若走 R1/R2/R3 ─→ D-E（形态决策）
                   │
                   └─→ D-B（身份）──┬─→ D-C（清理覆盖面）
                                    │
D-D（拒绝分型）─────────────────────┴─→ 交给 E 组
```

**并行建议：** D-A 与 D-D 无依赖，可同时开。D-B / D-C 应在 D-A 有结论后做
（若 D-A 直接不可行，D-B / D-C 的形态会随退路变化）。

**🛑 判定（2026-09-21 实测后）：落到第 4 行的 ❌ 分支。**

| D-A | D-B | D-C | D-D | 结论 |
|---|---|---|---|---|
| ✅ 嵌套成立 | ✅ 同一身份 | ✅ 覆盖完整 | ✅ 可分型 | **P1 可行**，C/F 按原计划 |
| ✅ | ⚠️ 只到 wrapper | ✅ | ✅ | P1 可行，但需补**沙箱会话级存活探针**（D-B 退路） |
| ✅ | ✅ | ❌ 有游离 | ✅ | P1 需补**沙箱后端专用终结点**（D-C 退路）——量级上升 |
| ❌ 阻断成立 | — | — | — | 评估 R1–R3；转 D-E 的 P2 评估，C 组形状待定 |
| — | — | — | ❌ 不可分型 | 不阻断接入，但完成率受损；E 组退化为"越界即 Abort" |

**🛑 实测落点（2026-09-21）：不是矩阵里任何一行 —— 是"两行同时成立"。**

```
D-A = ❌ 阻断成立
D-B = ⚠️ 只到 wrapper
D-C = ✅ 覆盖完整
D-D = ❌ 不可分型（且叠加只读策略失效）
```

**⇒ P1 全部形态否决；C 组形状待 P2 结论。**
且 D-D 暴露的"只读策略不生效"**独立于 D-A**，任何 Windows 沙箱方案都要面对它。

**下一步（按实测结论重排）：**

| 优先 | 事项 | 依据 |
|---|---|---|
| **1** | **验证 `windows_sandbox` 的 state-aware 会话**（本机 P2 唯一路径） | D-E |
| **2** | **`ui.disable` × 运行时的组合矩阵**：哪些工具链在哪种 UI 策略下能跑 | `ui.disable` 默认值会打死 Node/.NET/pwsh 7 |
| **3** | **把只读敞口立为独立问题** | D-D §5.3，影响整个 Windows 沙箱方案而非仅 MXC 接入 |
| **4** | **D-C 的机制对照**（只关 Job、不杀 wrapper） | 若将来依赖"Job 兜住沙箱" |
| **5** | **E 组按"越界即 Abort"落地** | D-D 已判不可分型，E 组设计前提已变 |
| **6** | 向 MXC 上游提 issue：`processcontainer` 在带 UI 限制的宿主 Job 内无法启动 | D-A，可复现 |

**无论结论如何，这三条今天就能并行开工：**

1. **A 组前置决策**（`[approvals]` 默认口径 / 无人值守超时行为 / MXC 定位）
2. **B 组策略学习**（`--audit`，零依赖，产出喂 C / E / F）
3. **E 组错误契约**（`hint` / `RetryDecision` / `Denied` vs `Abort`）——不依赖 D 的结论

---

## 五、引用的关键位置索引

**MXC（`main`，sparse clone）：**

| 事实 | 路径:行 |
|---|---|
| 子进程创建（`CREATE_SUSPENDED` + 属性表） | `src/backends/process_container/common/src/appcontainer_runner.rs:1123-1144` |
| 属性表内容（无 `JOB_LIST`） | 同上 `:858-1040` |
| Job 创建 / UI 限制 / 分配 / 失败即终结 | 同上 `:1186-1206` |
| `UiJobObject`（`KILL_ON_JOB_CLOSE` + `set_ui_limits` + `assign_process`） | `src/backends/process_container/common/src/job_object.rs:235-299` |
| `ResumeThread`（挂起→恢复的顺序） | `appcontainer_runner.rs:1375-1383` |
| Windows Sandbox 的 scoped teardown（取代 `taskkill /F /IM`） | `src/backends/windows_sandbox/lifecycle/src/teardown.rs`、`control_plane/os.rs:243` |
| Guest 侧仍有 `taskkill`（VM 内） | `src/backends/windows_sandbox/guest/src/executor.rs:169-184` |

**DSH：**

| 事实 | 路径:行 |
|---|---|
| Job 创建与 `KILL_ON_JOB_CLOSE`（无 breakaway 限制） | `crates/shared/nomi-process-runtime/src/platform/windows.rs:1670-1695` |
| `AssignProcessToJobObject` 的四处调用点 | 同上 `:200`、`:290`、`:419`、`:1100` |
| `enforce_sandbox` 静态拒绝（3 分支） | 同上 `:2874-2886` |
| `ActiveProcesses` 归零判定 | 同上 `:2026-2034` |
| `TerminateJobObject` | 同上 `:1870` |
| `ProcessError` 与分类码 | `crates/shared/nomi-process-runtime/src/request.rs:111-165` |
| 清理证明契约 | `crates/shared/nomi-process-runtime/src/command_builder.rs:508-531` |
| 子进程身份捕获 | `crates/shared/nomi-process-runtime/src/recovery.rs:87-155` |
| `SandboxPolicy` 三变体 | `crates/shared/nomi-process-runtime/src/capability.rs:4-8` |
| macOS Seatbelt 的对照实现 | `crates/shared/nomi-process-runtime/src/platform/unix.rs:3119-3192` |
| 浏览器侧的错误形状对照 | `crates/agent/nomi-browser-engine/src/engine.rs:184-185`；`nomi-browser-engine/src/actions.rs:867-891` |

**外部：**

| 事实 | 来源 |
|---|---|
| 嵌套作业与 UI 限制的互斥条件 | [Nested Jobs — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs)【官】 |
| 后端版本要求 / `--experimental` / 策略面 | [MXC README](https://github.com/microsoft/mxc/blob/main/README.md)【官】 |
| 方向性网络 schema / default-deny | [MXC Sandbox Policy 0.8.0](https://github.com/microsoft/mxc/blob/main/docs/sandbox-policy/0.8.0/policy.md)【官】 |
| 「任何 MXC profile 都不应被视为安全边界」 | MXC README 的 Warning【官】 |

# D-A 验证记录：MXC 能否在调用方 Job 内启动 `processcontainer`

> **日期：** 2026-09-21 · **性质：实验记录（时点快照）**，非契约
> **对应：** [`agent-harness-mxc-process-wrapper-feasibility.zh.md`](agent-harness-mxc-process-wrapper-feasibility.zh.md) 的验证门 D-A
> **分支：** `feat/mxc-feasibility-verification`（基线 `origin/main` @ `37701ba85`）
>
> **结论（一句话）：在"调用方把 `wxc-exec.exe` 放进一个 Job Object"的前提下，
> `processcontainer` 后端在本机**稳定失败**，返回 `WIN32_ERROR(50)` = `ERROR_NOT_SUPPORTED`。
> 把同一 Job 的 UI 限制去掉，则同一个命令成功。**
> 即：**D 组的硬阻断点成立**。

---

## 一、环境与基线

| 项 | 值 |
|---|---|
| OS | Windows 11 Insider Preview, build **29671**（`Dev`，UBR 1000）· 远高于 `processcontainer` Tier 1 所需的 26100 |
| MXC 源 | `ca7ea12ac6bd9f5420d6adecb37e32a8158da476`（2026-09-18，"Pin the directional egress default…"） |
| MXC 构建 | `cargo build -p wxc`（dev profile，1m23s），产物 `wxc-exec.exe` 0.8.0 / 11.4 MB |
| schema | `stableLatest = 0.8.0-alpha`；策略用 `version: "0.8.0-alpha"` + 方向性网络字段 |
| DSH 基线 | `origin/main` @ `37701ba85`，分支 `feat/mxc-feasibility-verification` |
| 测试策略 | `containment: "processcontainer"`，`process.commandLine = "cmd /c echo WXC_RAN_OK"`，`filesystem.readwritePaths` 指向一个 scratch 目录，网络 `egress/ingress` 全 `deny`。**`ui` 段留空** = MXC 的 default-deny UI 策略 |

**注意：`ui` 段留空不是"没有 UI 限制"。** MXC 的 `resolve_ui_restrictions`
（`src/core/wxc_common/src/ui_policy.rs:47-90`）在 `ui.disable == false` 时仍会默认
block 剪贴板读+写，并在 `!ui.injection` 时 block 输入注入。所以上表配置走的是
**默认带 UI 限制**的路径。

---

## 二、实验矩阵与结果

| # | 场景 | 外层 Job | 结果 |
|---|---|---|---|
| **1** | `wxc-exec` 直接在 shell 中运行（无外层 Job） | 无 | ✅ 输出 `WXC_RAN_OK`，exit **0** |
| **2** | `wxc-exec` 在 KILL_ON_JOB_CLOSE **+ UI 限制** 的 Job 内 | `0x10` | ❌ exit **4294967295 (0xFFFFFFFF)**，`CreateProcessW failed with error code 50` |
| **3** | `wxc-exec` 在 KILL_ON_JOB_CLOSE **无 UI 限制** 的 Job 内 | 无限制 | ✅ 输出 `WXC_RAN_OK`，exit **0** |

**场景 2 的完整错误（原文）：**

```json
{"error":{"code":"backend_error","extended_error":"CreateProcessW(PROC_THREAD_ATTRIBUTE_SECURITY_ENVIRONMENT) failed: WIN32_ERROR(50) (working directory: C:\\workspace\\allo\\target\\mxc-feasibility\\scratch (from filesystem policy (process.cwd omitted)))","message":"CreateProcessW failed with error code 50 (0x00000032)."}}
```

`0x32 = 50 = ERROR_NOT_SUPPORTED`。

---

## 三、内核层复现（独立于 MXC）

为排除"是不是 MXC 自己的 bug"，用一段最小的 Job Object harness 复现同一机制。
harness 建一个 Job、设 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`、以 `CREATE_SUSPENDED`
建子进程、`AssignProcessToJobObject` 进 Job、再 resume——与
`nomi-process-runtime/src/platform/windows.rs:1670-1695` 的 `arm_process_job` 同形。
子进程内再**查询内核**（`QueryInformationJobObject(NULL, JobObjectBasicUIRestrictions)`）
确认自己的即时 Job，然后尝试 `AssignProcessToJobObject(self, own_job)`。

| 运行时子进程的即时 Job UI 限制 | `AssignProcessToJobObject(self)` |
|---|---|
| `0x0`（无） | ✅ OK（嵌套形成；即时 Job 变为 `0x4`） |
| `0x10`（有） | ❌ **win32=50 `ERROR_NOT_SUPPORTED`** |

**这一列是唯一变量，结果完全由它决定。** 与官方文档的措辞一致：

> If a process that is already in a job is added to another job, the jobs are nested by default
> **if the system can form a valid job hierarchy and neither job sets UI limits**.
> — [Nested Jobs, Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs)

### 3.1 一个必须纠正的方法论教训

第一版 probe 用 `Set-Clipboard` 是否失败来"证明"被外层 Job 限制——**这是错的**。
非交互 pwsh 的 `Set-Clipboard` 不走被 `JOB_OBJECT_UILIMIT_WRITECLIPBOARD` 拦截的路径，
于是它**总是报"可写"**，让一个已经被限制的进程看起来没被限制。

v1 因此给出了一个**错误的"嵌套允许"结论**。v3 改为直接查询内核后结论反转。
**教训：容器/沙箱类验证里，任何"用业务操作是否失败来推断约束生效"的探针都不可信，
必须直接查询内核状态。**

（附带修正：`JOB_OBJECT_UILIMIT_WRITECLIPBOARD` 是 **`0x4`**，不是 `0x10`——
`0x10` 是 `JOB_OBJECT_UILIMIT_DISPLAYSETTINGS`。早期记录里写错了位。）

---

## 四、源码级事实

**MXC 侧：**

| 事实 | 位置 |
|---|---|
| 自建 Job、设 UI 限制、把**仍挂起**的子进程分配进去；失败即 `TerminateProcess` 并返回 | `src/backends/process_container/common/src/appcontainer_runner.rs:1186-1206`（UI 在 `:1192`，分配在 `:1193`） |
| Job 创建即 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`；`set_ui_limits` 调 `SetInformationJobObject(JobObjectBasicUIRestrictions)` | `.../common/src/job_object.rs:235-299` |
| 子进程创建标志 `EXTENDED_STARTUPINFO_PRESENT \| CREATE_SUSPENDED \| CREATE_UNICODE_ENVIRONMENT`（**无 breakaway**） | `appcontainer_runner.rs:1122-1127` |
| 属性表含 `SECURITY_CAPABILITIES` / `ALL_APPLICATION_PACKAGES_POLICY` / `MITIGATION_POLICY` / `HANDLE_LIST`（**无 `PROC_THREAD_ATTRIBUTE_JOB_LIST`**） | `appcontainer_runner.rs:858-1040` |
| UI 限制掩码会与 `supported_ui_limit_mask()` 求交 | `job_object.rs:269-270` |
| 默认 UI 策略为 default-deny（剪贴板读写 + 输入注入） | `src/core/wxc_common/src/ui_policy.rs:47-90` |
| `ui.disable` 时设全部标志 | 同上 `:53-66` |

**DSH 侧：**

| 事实 | 位置 |
|---|---|
| `arm_process_job` 只设 `KILL_ON_JOB_CLOSE`，**未设任何 breakaway 限制** | `crates/shared/nomi-process-runtime/src/platform/windows.rs:1677-1695` |
| `AssignProcessToJobObject` 四处调用点 | 同上 `:200`、`:290`、`:419`、`:1100` |
| `enforce_sandbox` 只有 3 个静态分支 | 同上 `:2874-2886` |
| `ActiveProcesses` 归零判定 | 同上 `:2026-2034` |

---

## 五、根因判定：已验证的部分与未验证的部分

**已验证（可直接作为判据）：**

1. **在本机、当前 MXC 版本，把 `wxc-exec.exe` 放进一个 Job Object 后，
   `processcontainer` 后端无法启动沙箱子进程**，返回 `ERROR_NOT_SUPPORTED`（场景 2 vs 3）。
2. **外层 Job 的 UI 限制是这条失败的决定性变量**（§三 的对照）。
3. **机制属 Windows 嵌套作业限制**，不是 MXC 的普通 bug：它与官方文档的
   "neither job sets UI limits" 条款方向一致，且在 MXC 之外被独立复现。

**未验证（写入决策前应补）：**

1. **精确的失败调用点**。错误文本报的是 `CreateProcessW(PROC_THREAD_ATTRIBUTE_SECURITY_ENVIRONMENT)`，
   而源码里 `AssignProcessToJobObject` 的失败路径
   （`appcontainer_runner.rs:1200-1205`）也应产生一个 `WxcError::Process`。
   当前证据**不能区分**是
   (a) `CreateProcessW` 因 SECURITY_ENVIRONMENT 属性在 Job 内被拒，
   (b) 之后的 `AssignProcessToJobObject` 被拒（且错误文本来自更上一层），
   还是 (c) 两者叠加。§三 的内核复现证明 (b) 这类失败**确实会发生**，
   但没有证明它**就是本场景**发生的那个。
   → **补法：** 在 MXC 的 `appcontainer_runner.rs:1186-1206` 加临时日志（或跑 `--debug`），
   看 `UiJobObject::new` / `set_ui_limits` / `assign_process` 三步各走到哪一步。
2. **DSH 真实 Job 与 harness Job 的差异**。harness 用裸 `CREATE_SUSPENDED`；
   DSH 走 `EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT`
   且带属性表。（实际影响可能为零，但未验证。）
3. **`set_ui_limits` 返回的错误码**。它经 `map_err` 包装成 `WxcError::Process(...)`
   字符串，原始 Win32 码会丢失——这本身是个可报给上游的问题：
   **`ERROR_NOT_SUPPORTED` 这种可诊断信息被字符串化，调用方无法按码分型**，
   正是本文档层 3（类型化拒绝）要解决的问题在 MXC 侧的对应物。
4. **其他后端是否同样受影响**。`windows_sandbox`（VM 内跑，与宿主 Job 无关）理论上不受影响，
   未测；`isolation_session` 需 Insider 26340.9212，本机 29671 是否满足未测。

---

## 六、对退路评估的影响

本文档的退路（R1–R4，见可行性清单 §三 D-A）据此可以重排：

| 退路 | 本次验证后的评估 |
|---|---|
| **R1 · DSH 让出 Job**（沙箱化会话不建 DSH 的 Job，改依赖 MXC 的 job） | **变成最直接可行的一条**。场景 1 与场景 3 都证明：**只要不给 `wxc-exec` 套一个带 UI 限制的外层 Job，`processcontainer` 就能正常工作**。代价是 DSH 失去自己的 Job 兜底，需重做"进程树清理已证明"的语义 |
| **R2 · 允许 breakaway** | 仍不推荐：breakaway 让沙箱树脱离 DSH 的 Job，`ChildProcessCleanup` 的"已证明"语义失效。**净损失** |
| **R3 · 要求 MXC 支持嵌套** | 本次验证给了上游一个**具体、可复现的缺陷报告**：`processcontainer` 在已有 Job（带 UI 限制）的宿主进程内无法启动。可作为 issue 提交 |
| **R4 · 换后端 / 换形态** | 仍然成立，且**优先级上升**：MXC 的 state-aware 生命周期（长驻沙箱会话）或 `windows_sandbox` 都不经过"每命令套 Job"这条路径，天然绕开本阻断 |

**关键推论：** DSH 现在对每个受管子进程都建 Job。**"沙箱化会话"必须与"普通受管会话"
走不同的 Job 策略**，否则 `processcontainer` 用不了。这不是接线细节，是架构决策。

---

## 七、复现材料

以下脚本在验证期间位于 `target/mxc-feasibility/`（`target/` 被 gitignore，不入库）。
逻辑与关键参数已完整记在本文档 §二/§三，可据此重建：

| 文件 | 作用 |
|---|---|
| `nest-probe.ps1` | 子进程侧：查自己的即时 Job UI 限制 → 建自建 Job → `AssignProcessToJobObject(self)` → 复查。**自校验**：查不到即时 Job 时明确报"inconclusive"而不是给结论 |
| `job-harness.ps1` | 父进程侧：建同形 Job（可配 `-UiMask`）、`CREATE_SUSPENDED` 建子进程、分配进 Job、resume、报子进程退出码；并回验 `IsProcessInJob` |
| `policy-d-a.json` | MXC 策略 0.8.0-alpha，`containment: "processcontainer"`，`ui` 留空（默认 UI 限制） |

**复现命令（三行）：**

```powershell
$wxc = "<mxc>\src\target\debug\wxc-exec.exe"
& $wxc "<...>\policy-d-a.json"                                  # 期望: WXC_RAN_OK, exit 0
& pwsh -File job-harness.ps1 -Target $wxc -TargetArgs "<...>\policy-d-a.json" -UiMask 0x10   # 期望: 失败 50
& pwsh -File job-harness.ps1 -Target $wxc -TargetArgs "<...>\policy-d-a.json" -UiMask 0      # 期望: WXC_RAN_OK
```

---

## 八、对可行性清单的更新

- **D-A：✅ 已验证 —— 阻断成立**（在"调用方持有 Job"前提下）
- **D-B / D-C**：**仍未验证**，且因为 P1 形态被阻断，这两门的形态随退路决定而变
  （若走 R1，`ExactProcessIdentity` 捕获的仍是 wrapper，D-B 的问题原样存在）
- **D-D**：未验证，可独立推进
- **D-E：优先级上升** —— 它不再只是"退路之一"，而是很可能成为主路径
- **建议下一步：** 先做 D-E（P2 长驻沙箱会话形态的可行性），
  再做 R1 的代价评估（DSH 放弃自身 Job 后如何维持清理证明）

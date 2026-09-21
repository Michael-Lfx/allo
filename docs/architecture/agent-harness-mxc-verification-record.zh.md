# MXC 接入可行性验证记录（D-A 至 D-E）

> **日期：** 2026-09-21 · **性质：实验记录（时点快照）**，非契约
> **对应：** [`agent-harness-mxc-process-wrapper-feasibility.zh.md`](agent-harness-mxc-process-wrapper-feasibility.zh.md) 的五个验证门
> **分支：** `feat/mxc-feasibility-verification`（基线 `origin/main` @ `37701ba85`）
>
> **总判定：D 组不能走"每命令包装 `wxc-exec`"这条形态（P1），必须转向 P2（长驻沙箱会话）
> 或 R1（DSH 让出 Job）。D-D 还额外暴露了一个与 D-A 无关、但同样阻断的事**：
> **只读路径策略在本机不生效**。

---

## 一、环境与基线

| 项 | 值 |
|---|---|
| OS | Windows 11 Insider Preview, build **29671**（Dev, UBR 1000） |
| MXC 源 | `ca7ea12ac6bd9f5420d6adecb37e32a8158da476`（2026-09-18） |
| MXC 构建 | `cargo build -p wxc`（dev），`wxc-exec.exe` 0.8.0 |
| schema | `stableLatest = 0.8.0-alpha`；state-aware 契约 `0.9.0-alpha` |
| DSH 基线 | `origin/main` @ `37701ba85` |
| `--probe`（本机能力） | `tier: base-container`（Tier 1）· `baseContainerApiPresent: true` · **`bfscfgPresent: false` + `bfsCompiledIn: false`（Tier 2 彻底不可用）** · `baseContainerSupportsDenyPaths: true` · `isolationSessionAvailable: **false**` · UI 位 10/10 可拦 |

### 1.1 两个"环境前置"，缺一则所有验证都跑不起来

| 前置 | 现象 | 解法 |
|---|---|---|
| **`ui.disable` 必须为 `false`** | 默认 UI 策略 block Win32k 系统调用；Node / .NET 等运行时报 `STATUS_DLL_INIT_FAILED (0xC0000142)`，MXC 自身提示 "Enable UI access… set `ui.disable: false`" | 策略加 `"ui": { "disable": false }`（**该 JSON 形态只有 `disable` / `clipboard` / `injection` 三个字段**；`allowWindows` 是 SDK `SandboxPolicy` 的字段，写进 JSON 会 `unknown field` 报错） |
| **子进程运行时的选择** | `pwsh` 7 在沙箱内起不来："pwsh.exe versions before 7.7 require read-only access to the root drive (`C:\`)"；Node 默认策略下起不来（Win32k）；**.NET Framework 二进制起不来**（`ERROR_DLL_INIT_FAILED`） | 用 **Windows PowerShell 5.1**（`C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`）作为沙箱内探针运行时；沙箱内 **CIM/WMI 不可用**，父进程查询要走 Toolhelp |

> 这两条本身就是结论：**DSH 的 Bash 工具链（cargo / bun / git / gh）大多是原生 exe 或 node/pwsh 脚本，
> 默认策略下会大面积起不来。** 见 §五 的完成率推论。

---

## 二、D-A · 沙箱化进程能否加入调用方 Job 层次 —— ❌ 阻断成立

| 场景 | 外层 Job | 结果 |
|---|---|---|
| `wxc-exec` 独立运行 | 无 | ✅ `WXC_RAN_OK`，exit 0 |
| `wxc-exec` 在 Job 内（`KILL_ON_JOB_CLOSE` **+ UI 限制**） | `0x10` | ❌ exit `0xFFFFFFFF`，`CreateProcessW failed with error code 50` |
| `wxc-exec` 在 Job 内（`KILL_ON_JOB_CLOSE` **无 UI 限制**） | 无 | ✅ `WXC_RAN_OK`，exit 0 |

`0x32 = 50 = ERROR_NOT_SUPPORTED`。

**内核层独立复现**（不依赖 MXC）：子进程查自己的即时 Job UI 限制，
`0x0` 时 `AssignProcessToJobObject(self, own_job)` 成功；`0x10` 时返回 **win32=50**。
与官方 "neither job sets UI limits" 条款方向一致。

**判定：D-1（透明包装）不可行；DSH 现在对每个受管子进程都建 Job，故 `processcontainer` 不能用。**

### 2.1 一个错误结论的纠正（方法论）

第一版探针用 `Set-Clipboard` 是否失败来"证明"被外层 Job 限制——**错的**。
非交互 pwsh 的 `Set-Clipboard` 不走被 `JOB_OBJECT_UILIMIT_WRITECLIPBOARD` 拦截的路径，
**总是报"可写"**，让已被限制的进程看起来没被限制；据此一度得出"嵌套允许"的相反结论。
改成直接查询内核（`QueryInformationJobObject(NULL, JobObjectBasicUIRestrictions)`）后结论反转。

> **教训：容器/沙箱验证里，"用业务操作是否失败来推断约束生效"的探针不可信，必须直接查内核状态。**

（附带修正：`JOB_OBJECT_UILIMIT_WRITECLIPBOARD` 是 `0x4`，`0x10` 是 `DISPLAYSETTINGS`。）

---

## 三、D-B · `ExactProcessIdentity` 捕获到的是谁 —— ⚠️ 只到 wrapper（半透明）

**方法：** 把 `wxc-exec` 放进与 `arm_process_job` 同形的 Job（`KILL_ON_JOB_CLOSE`，
**无 UI 限制**，以避开 D-A 的阻断），沙箱内跑 PS 5.1 探针自报身份，同时读 Job 的进程表。

**沙箱内探针原始输出：**

```
PROBE|pid=45596|self=powershell|ppid=42544|parent=powershell.exe|job_ui=0x0|job_ui_err=-|in_job=1|job_active=2|grandchild=12080
```

（同一实验第二次运行：`pid=23548|ppid=26464|grandchild=45928`）

**Job 进程表采样（harness 视角，即 DSH 的视角）：**

```
[harness] direct child pid = 42544
[harness] job process list sample: [42544]                (active=3)
[harness] job process list sample: [42544,45596,33040]    (active=3)
[harness] job list AFTER child exit: []                   (active=0)
```

**判读：**

| 观测 | 值 | 含义 |
|---|---|---|
| DSH 直接子进程（`wxc-exec.exe`） | pid **42544** | DSH 的 `capture_child_identity` 拿到的是这个 |
| 沙箱内真实进程 | pid **45596**，其 `ppid` = **42544**，即它是 `wxc-exec.exe` 的子进程 | **不同 PID** |
| 沙箱内进程在这个 Job 链上吗 | **在**（出现在 `JobPids` 里，`in_job=1`） | 清理覆盖面在（见下） |
| 沙箱内**自己看到的**即时 Job UI 限制 | **`0x0`** | MXC 为它建了自己的 Job（无 UI 限制），且**它继承了调用方 Job** |

**判定：⚠️ 半透明 —— "不同 PID，且 DSH 只持 wrapper"。**

所以可行性清单 D-B 的退路成为必需：

> 必须补一个"**沙箱会话级**"的存活探针，否则 `recovery.rs` 的孤儿回收对沙箱会话失效。

**附带证伪：** 因为沙箱内进程在 `JobPids` 里，说明 §二 我最初推断的"嵌套被 UI 限制挡住"
在**外层 Job 无 UI 限制**时并不发生 —— MXC 的 `j = UiJobObject::new()` + `set_ui_limits` +
`assign_process` 三步是**成功**的（否则 `appcontainer_runner.rs:1200-1205` 会终结并报错，
而我们拿到了正常输出）。这同时解释了 D-A 的失败：**只有外层 Job 自带 UI 限制时才会失败**。

---

## 四、D-C · `KILL_ON_JOB_CLOSE` 覆盖面 —— ✅ 成立（0 存活）

**方法（`kill-harness.ps1`）：** 同形 Job → spawn `wxc-exec` → 沙箱内 `spawnchild_sleep`
（探针再派生一个 `cmd.exe` 孙进程）→ 收集 Job 进程表 → **强杀 wrapper** → 观察。

**原始结果：**

```
PROBE|pid=26440|ppid=1840|parent=powershell.exe|job_ui=0x0|in_job=1|grandchild=44456
[kill] direct child pid = 1840
[kill] job contents before force-kill: [1840,26440,46028]           <- wrapper + 沙箱内 PS + 孙进程
[kill] observed pids so far: [1840,26440,46028,44456,41468,45100]   <- 期间累计 6 个
[kill] === FORCE-KILL the direct child (wrapper) ===
[kill] wrapper exit code = 4294967295
[kill] job contents after wrapper kill: []                          <- Job 已空
[kill] --- liveness while job still OPEN ---
[kill] gone   pid=1840 / 26440 / 46028 / 44456 / 41468 / 45100
[kill] job handle closed
[kill] --- liveness AFTER job close (survivors = orphans) ---
[kill] reaped   pid=1840 / 26440 / 46028 / 44456 / 41468 / 45100
[kill] survivors = 0
```

**判读（对照可行性清单 D-C 的三种观测）：**

| 观测 | 结果 |
|---|---|
| 沙箱内子进程是否退出 | ✅ 全部退出 |
| 是否存在游离进程 | ❌ 无（0 survivors） |
| `ActiveProcesses` 是否归零 | ✅ Job 进程表变空 |
| "计数器撒谎"最坏情况 | ✅ 未发生（进程表为空 **且** 系统里确实没有残留） |

**判定：✅ 覆盖面成立。** 强杀 wrapper 不会遗留沙箱内进程树 —— 这是**对 DSH 有利**的一条：
`ChildProcessCleanup` 的"已证明"语义在沙箱会话上仍然成立。

> 注意归因的诚实性：**这不是"DSH 的 Job 兜住了沙箱"的证明。** 沙箱内进程在 Job 链上，
> 但 wrapper 被强杀时它们也全部消失，无法区分是
> (a) MXC 自身的 `KILL_ON_JOB_CLOSE` 起作用、
> (b) DSH 的 Job 兜住、
> 还是 (c) wrapper 死时 OS 连带终止了它的子进程。
> **对 DSH 的结论相同（无孤儿），但机制未定 → 若要依赖它，应补一次"只关 DSH 的 Job、不杀 wrapper"的对照。**

---

## 五、D-D · 运行时拒绝能否与启动失败区分 —— ❌ 不可分型 + 🔴 发现只读策略失效

### 5.1 拒绝**确实生效**（先确认这一点）

**净室复现**（删除全部目标文件后重跑）：

| 操作 | 沙箱内结果 | 事后文件是否存在 |
|---|---|---|
| 写 `scratch\`（`readwritePaths`） | `OK` | ✅ 存在 |
| 写 `blocked\`（未列出） | `CLR:UnauthorizedAccessException` | ❌ 不存在 |
| 写 `blocked\`（raw `CreateFile`） | `CLR:MethodInvocationException` | ❌ 不存在 |
| 写 `C:\Windows\System32` | `CLR:UnauthorizedAccessException` | ❌ 不存在 |
| sandbox 退出码 | **0**（子进程自己 return 0） | — |

`cmd` 子进程对照（非 PowerShell，排除 CLR 包装）：

```
cmd /c echo x > ...\blocked\fromcmd.txt
→ "Access is denied."
→ fromcmd.txt exists = False
```

**结论①：写拒绝真实生效**，且在子进程里表现为**普通 Windows `ERROR_ACCESS_DENIED`
（"Access is denied"）**。

### 5.2 判定：❌ 不可分型

对照可行性清单 D-D 的通过标准（"能稳定地判定这是沙箱策略拒绝，并拿到至少一个稳定的分类码"）：

| 需要区分 | 实际 |
|---|---|
| 沙箱拒绝 vs 程序自身失败 | ❌ **不可区分** —— 都是 `ERROR_ACCESS_DENIED` 文本/异常 |
| 退出码区分 | ❌ **无**（子进程正常 return 0；一次对照中 `cmd` 返回 exit 1，但那是 cmd 自己的） |
| MXC 是否给出专用标记 | ❌ **无**（子进程侧看不到任何 MXC 标识） |
| `denials.json` / `captureDenials` | 未验证（`--audit` 模式，Windows ProcessContainer only，需隔离机/权限，本轮未跑） |

**→ E 组的 `RetryDecision` 无法从子进程错误面可靠推导。** 必须走 D-D 的退路：
**越界一律 `Abort`（停下等人）的保守口径**，或改从 MXC 的外层错误通道取信息（未验证）。

### 5.3 🔴 与 D-A 无关的第二个阻断：只读路径策略在本机**不生效**

**读边界实测**（同一策略：`readonlyPaths` 只列了 `C:\` 与 `build\`）：

```
READ1=OK        <- 读 blocked\secret.txt（未在任何清单里）
READ2=OK        <- 读 blocked\secrets\deep.txt
LIST=OK         <- 列 blocked\ 目录
SYSTEM_READ=OK  <- 读 C:\Windows\win.ini
HOMEDIR_READ=OK <- 读 C:\Users\15165\.gitconfig   ← 用户主目录
```

**在真实 MXC 沙箱内读到了用户主目录的 `.gitconfig`。** 即：

> **`readwritePaths` / `readonlyPaths` 在一台 Tier 1 (BaseContainer) 主机上
> 对"读"没有约束力；被约束的只有"写"。**

对照 `--probe` 的 `baseContainerSupportsDenyPaths: true`：BaseContainer **技术上能**执行
拒绝路径，但**当前 schema/后端没有把它接到"读"上**，而 `deniedPaths` 在 README 里仍标为
Windows 未支持。

**为什么这条比 D-A 更严重：**

1. DSH 的 Bash 工具链**必须**能读工作区外的路径才能工作（`~/.cargo/registry`、
   `~/.bun/install/cache`、`%TEMP%`、工具链目录）。而工作区外的路径里恰好有
   `~/.cargo/credentials.toml`、`~/.gitconfig`、`~/.ssh`、`%APPDATA%` 下的各种 token。
2. 所以"把 `~/.cargo` 加进白名单"这个唯一可行的做法，等于**把凭据目录一起授给沙箱**。
   §5.4 说明这为什么不是致命 —— 但前提是网络隔离**必须**同时成立。
3. 行业口径正是"文件隔离与网络隔离缺一不可"：缺文件隔离 → 能逃逸重获网络；
   缺网络隔离 → 能外泄。本机的情形是**文件隔离的一半（读）缺失**，于是
   **网络隔离从"纵深防御的一层"变成"唯一防线"**。

### 5.4 网络隔离确实生效（本轮唯一没打折的边界）

| 探针 | 沙箱内 | 沙箱外基线 |
|---|---|---|
| `http_egress`（`http://example.com/`） | `CLR:WebException`（连接失败） | `OK_HTTP:200` |
| `tcp_egress`（`1.1.1.1:443`） | 失败/超时 | （本机基线也超时，非判据） |

**结论②：`egress.default: "deny"` 生效。** 这是把 §5.3 的读敞口从"灾难"降级为
"必须以网络隔离为唯一防线"的那一条。

---

## 六、D-E · 是否该放弃"每命令包装"（形态决策）—— 结论：应当放弃

### 6.1 关键事实（修正一条此前的转引错误）

**state-aware 生命周期不止 IsolationSession 一个后端实现。** 源码实证：

| 事实 | 位置 |
|---|---|
| `impl StatefulSandboxBackend for IsolationSessionRunner` | `src/backends/isolation_session/common/src/state_aware.rs:79` |
| **`impl StatefulSandboxBackend for WindowsSandboxRunner`** | `src/backends/windows_sandbox/lifecycle/src/state_aware.rs:603` |
| **`impl StatefulSandboxBackend for WslcStateAwareRunner`** | `src/backends/wslc/common/src/state_aware.rs:53` |
| SDK 的联合类型 | `sdk/node/src/state-aware-types.ts:22-25`：`Extract<ContainmentBackend, 'isolation_session' \| 'windows_sandbox' \| 'wslc'>` |
| state-aware 契约版本 | `STATE_AWARE_VERSION = '0.9.0-alpha'`（`:38`） |
| 生命周期六原语 | `provisionSandbox` / `startSandbox` / `execInSandbox` / `execInSandboxAsync` / `stopSandbox` / `deprovisionSandbox`（`sdk/node/src/state-aware.ts`） |

**这直接改写了可行性清单 D-E 里"P2 可能把后端锁死在 isolation_session"这一条代价** ——
`windows_sandbox` 也在其中，而它**不经过"每命令套 Job"**（沙箱在一个独立 VM 里）。

### 6.2 本机可用性

| 后端 | 本机状态 |
|---|---|
| `isolation_session` | ❌ `isolationSessionAvailable: false`（`--probe`，build 29671） |
| `windows_sandbox` | 未直接探测；`--probe` 不报它，需 `--experimental` 且需 Windows Sandbox 功能 |
| `wslc` | 需要 WSL2 + 预拉镜像（`--setup-wslc`） |
| `hyperlight` | ❌ `hyperlightAvailable: false` |
| `processcontainer` | ✅ 但受 D-A 阻断（P1 形态） |

**⇒ 在本机，P2 形态要么走 `windows_sandbox`（重，整个 VM），要么不可用。**

### 6.3 决策

| 形态 | 评估 |
|---|---|
| **P1 · 每命令包装** | ❌ **否决**。D-A 阻断（外层 Job 有 UI 限制即失败）；且即便 R1 让出 Job，D-B 仍只到 wrapper、D-D 仍不可分型 |
| **P2 · 长驻沙箱会话** | ⚠️ **架构上可行且是唯一出路**，但本机只能用 `windows_sandbox`（VM 级，重）；且引入了"DSH 的 `ProcessSupervisor` 与 MXC 的沙箱生命周期如何共存"的新问题 |
| **R1 · DSH 让出 Job** | ⚠️ 仍可作为 P1 的解药（§三 已证明外层 Job 无 UI 限制时 MXC 正常），但**解决不了 D-D 的不可分型**，且放弃 DSH 自身的清理兜底 |

**D-E 判定：应当放弃 P1，转 P2 评估。** 但 P2 有三个已识别的未决代价（见 §八）。

---

## 七、五门汇总

| 门 | 结论 | 对 DSH 的意义 |
|---|---|---|
| **D-A** | ❌ **阻断成立** | `processcontainer` 在 DSH 的 Job 内每次 spawn 都失败 |
| **D-B** | ⚠️ **只到 wrapper** | 必须补沙箱会话级存活探针，否则孤儿回收失效 |
| **D-C** | ✅ **0 存活** | 强杀不遗留沙箱内进程树；但机制未定，依赖前应补对照 |
| **D-D** | ❌ **不可分型** + 🔴 **只读策略失效** | E 组只能退化为"越界即 Abort"；网络隔离成为唯一防线 |
| **D-E** | ⚠️ **放弃 P1，转 P2** | state-aware 有 `windows_sandbox` 可用；但本机重 |

### 7.1 最重的两条（与选哪条退路无关）

1. **只读策略在 Tier 1 主机上不生效**（§5.3）—— 这不是 MXC 的接线问题，
   是当前后端的能力缺口。它意味着"给 agent 一个能跑 cargo 的沙箱"和
   "不让 agent 读到凭据"在 Windows 上**同时做不到**，只能靠网络隔离兜。
2. **默认 UI 策略会打死原生运行时**（§1.1）—— Node / .NET / pwsh 7 默认起不来。
   DSH 的 Bash 工具链正是这些。**这一条若不先解决，接上沙箱的当天 DSH 就不可用。**

---

## 八、下一步（按结论重排）

| 优先 | 事项 | 为什么 |
|---|---|---|
| **1** | **验证 `windows_sandbox` 后端的 state-aware 会话**（P2 唯一本机可行路径） | 决定 P2 是否真能落地；需 `--experimental` + Windows Sandbox 功能 |
| **2** | **补 `ui.disable` 与运行时的组合矩阵**：哪些工具链在哪种 UI 策略下能跑 | §1.1 是"接上即不可用"的风险，必须先摸清 |
| **3** | **把只读敞口作为独立问题上报/记录** | 它影响的不是 MXC 接入，而是**整个 Windows 沙箱方案的可行性** |
| **4** | **D-C 的机制对照**（只关 Job、不杀 wrapper） | 若将来依赖"Job 兜住沙箱"，需要这个证据 |
| **5** | **E 组按"越界即 Abort"落地** | D-D 已判定不可分型，E 组的设计前提变了 |
| **6** | 向 MXC 上游提 issue：`processcontainer` 在带 UI 限制的宿主 Job 内无法启动 | 可复现的具体缺陷（§二） |

---

## 九、复现材料

验证脚本位于 `target/mxc-feasibility/`（`target/` 被 gitignore，未入库）。
逻辑与关键参数已完整记在本文各节，可据此重建。主要文件：

| 文件 | 作用 |
|---|---|
| `nest-probe.ps1` | 内核层嵌套复现（自校验：查不到即时 Job 时明确报 inconclusive） |
| `job-harness.ps1` | DSH 同形 Job + `CREATE_SUSPENDED` + Job 进程表枚举 + 关闭前后存活对比 |
| `kill-harness.ps1` | D-C：强杀 wrapper 后的树清理验证 |
| `sbx-probe.ps1` | 沙箱内身份探针（**PS 5.1**，Toolhelp 查父进程，因为沙箱内 CIM 不可用） |
| `deny-probe.ps1` / `read-probe.ps1` | D-D：拒绝分型与读边界 |
| `policy-d-*.json` | 各门用的 0.8.0-alpha 策略 |

**关键命令形态：**

```powershell
# D-A
& wxc-exec.exe policy.json                                  # 期望 WXC_RAN_OK
& pwsh -File job-harness.ps1 -Target wxc-exec.exe -TargetArgs policy.json -UiMask 0x10  # 期望失败 50
& pwsh -File job-harness.ps1 -Target wxc-exec.exe -TargetArgs policy.json -UiMask 0     # 期望 WXC_RAN_OK

# 所有沙箱内策略都必须带
#   "ui": { "disable": false }
```

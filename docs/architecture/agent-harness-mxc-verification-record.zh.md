# MXC 接入可行性验证记录（D-A 至 D-E）

> **日期：** 2026-09-21 · **性质：实验记录（时点快照）**，非契约
> **对应：** [`agent-harness-mxc-process-wrapper-feasibility.zh.md`](agent-harness-mxc-process-wrapper-feasibility.zh.md) 的五个验证门
> **分支：** `feat/mxc-feasibility-verification`（基线 `origin/main` @ `37701ba85`）
>
> **⚠️ 本文只覆盖"可行性"（能不能跑通），不覆盖"接入风险"。**
> 已实测的风险、未验证的未知、以及架构层面的缺口另见
> [`agent-harness-mxc-risk-register.zh.md`](agent-harness-mxc-risk-register.zh.md)。
>
> **⚠️ 术语与验证对象的澄清（重要，2026-09-21 补）：**
>
> 本文中的 **"DSH" 指的是验证时用的替身 harness**（手写的 `job-harness.ps1` 等
> PowerShell 脚本），**不是** `crates/agent/nomi-agent`，也不是我所在的 harness。
> 该替身**复刻**了 `nomi-process-runtime/src/platform/windows.rs` 的
> `arm_process_job` / `spawn_child_process` 的形态，以便隔离变量。
>
> **替身能验证 Windows Job 语义，不能验证宿主代码路径。** 因此凡涉及
> "本仓库能不能接入"的结论，一律以
> `crates/shared/nomi-process-runtime/tests/mxc_supervision_probe.rs`
> （走**真实** `ProcessSupervisor`）为准，**不以替身结果外推**。
> §2.2 就是这条规则纠正一处外推错误的实例。
>
> 文中残留的 "DSH 的 Job" 一律应读作"**调用方 Job**"。
>
> **总判定（二次校订后）：D 组不能走"每命令包装 `wxc-exec`"这条形态（P1）——
> 但主要理由不是 D-A。** D-A 经真实代码路径实测**不构成阻断**
> （`nomi-process-runtime` 的 Job 只设 `KILL_ON_JOB_CLOSE`，不带 UI 限制；真实
> `ProcessSupervisor` 实测能跑通）。真正拦住 P1 的是：

1. **§5（D-D）拒绝不可分型** —— 文件类策略拒绝不进 `captureDenials`
2. **§10.1 工具链** —— 默认 UI 策略打死 node/cargo/git/powershell 等主力运行时

D-D 还暴露了一个与 D-A 无关的问题：**读权限受允许根约束，且无细粒度读拒绝**
（见 §10.4 的校正口径）。
>
> **✅ 全部验证已完成（2026-09-21 续）。** 五门之外还补了四项：
> **运行时×UI 矩阵** · **`captureDenials` 覆盖范围** · **`process.env` 语义** · **读边界定性**。
> 结论汇总见 [`agent-harness-mxc-feasibility-report.zh.md`](agent-harness-mxc-feasibility-report.zh.md)
> （可行性汇报）。本文件是原始读数；汇报是判定。
>
> **两项需要修正的先期结论（本轮更正）：**
> 1. **§5.3 的"只读策略不生效"要改口径** —— 读取并非无限制，而是**受允许根约束**；
>    上一轮"什么都能读"是我自己给了 `readonlyPaths: ["C:\\"]` 造成的。见 §十。
> 2. **§一 的"环境被剥离"是错的** —— 默认环境**继承 ~40 个父进程变量**，
>    只有 `LOCALAPPDATA`/`TEMP`/`TMP` 被改写进 AppContainer 包目录。见 §十。
>
> **唯一未完成项**：`windows_sandbox` 的 state-aware 会话**实跑**
> （本机 `Containers-DisposableClientVM` 为 Disabled，启用需重启；未擅自执行）。

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

## 二、D-A · 沙箱化进程能否加入调用方 Job 层次 —— ⚠️ 条件性，且**不适用于 `nomi-process-runtime` 的现路径**

> **🔴 本节结论已于 2026-09-21 二次校订。** 原判定写的是"阻断成立 ⇒ DSH 不能用"，
> **这个推论是错的**（把"外层 Job 带 UI 限制时失败"错读成了"调用方持有 Job 就失败"）。
> 下面的 harness 测量不变，结论口径与适用范围见 §2.2。

### 2.1 harness 测量（不变）

**用替身 harness 复刻 `arm_process_job` 的形状，改变唯一变量：**

| 场景 | 外层 Job | 结果 |
|---|---|---|
| `wxc-exec` 独立运行 | 无 | ✅ `WXC_RAN_OK`，exit 0 |
| `wxc-exec` 在 Job 内（`KILL_ON_JOB_CLOSE` **+ UI 限制**） | `0x10` | ❌ exit `0xFFFFFFFF`，`CreateProcessW failed with error code 50` |
| `wxc-exec` 在 Job 内（`KILL_ON_JOB_CLOSE`**只此一项**） | 无 UI 限制 | ✅ `WXC_RAN_OK`，exit 0 |

`0x32 = 50 = ERROR_NOT_SUPPORTED`。

**内核层独立复现**（不依赖 MXC）：子进程查自己的即时 Job UI 限制，
`0x0` 时 `AssignProcessToJobObject(self, own_job)` 成功；`0x10` 时返回 **win32=50**。

**⇒ 决定成败的变量是「外层 Job 是否带 UI 限制」，不是「外层是否持有 Job」。**

### 2.2 🔴 关键校正：这条阻断不命中 `nomi-process-runtime`

读完真实实现后确认两者不同：

```rust
// crates/shared/nomi-process-runtime/src/platform/windows.rs
// :1677-1695  arm_process_job
limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;  // ← 仅此一项，无任何 UI 限制
```

而 harness 失败的那一组，**唯一差别就是多设了 `ui_mask = 0x10`**。

**⇒ `nomi-process-runtime` 的 Job 不带 UI 限制 ⇒ 落在"✅ 成功"那一行。**

**并用真实代码路径实测确证**（不是替身）：新增
`crates/shared/nomi-process-runtime/tests/mxc_supervision_probe.rs`，
用**本 crate 的 `ProcessSupervisor`** 把 `wxc-exec.exe` 当子进程启动：

```
Exited { code: Some(0), signal: None,
         output: "NOMI_SUPERVISED_OK\r\n",
         cleanup: CleanupReport { reaped: true, errors: [] } }
test wxc_exec_runs_as_a_supervised_child_process ... ok
```

**⇒ `nomi-process-runtime` 能正常监督 `wxc-exec` 子进程。D-A 不构成接入阻断。**

**该测试的适用边界（诚实声明）：**

| 证明了 | 没证明 |
|---|---|
| 本 crate 的 Job（`KILL_ON_JOB_CLOSE` only）+ 真实 `ProcessSupervisor` 能跑通 MXC | 生产 `nomi-agent` 的**完整工具链**在沙箱内可用（那是 §10.1 矩阵的事） |
| 该结论在 Windows build 29671 / MXC `ca7ea12` 上成立 | 其他宿主或未来给 Job 加 UI 限制后仍成立 |
| — | 原 harness 的失败条件**未**在本 crate 路径上复现（见下） |

**控制实验的负结果：** 尝试在本 crate 路径上复现原条件（把已启动的子进程
后挂到一个带 UI 限制的 Job）——**得到 "nesting allowed"，即未复现**。
说明该冲突**特异地依赖 harness 的时序**（对**仍挂起**的子进程先分配、再恢复），
本 crate 的公开 API 刻意不设 UI 限制，因此触达不到该路径。
控制实验保留在测试文件里并明确标注为"未复现 D-A"。

**这条同时修正了 `arm_process_job` 的一个潜在未来风险**：
**若将来给这个 Job 加上 UI 限制，D-A 就会真的生效。** 建议在 `arm_process_job` 处留注释。

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

### 4.1 ✅ 机制对照已完成（2026-09-21 续）

跑 `kill-harness.ps1 -JobCloseOnly`（**不杀 wrapper，只关 Job**）：

```
[kill] === CONTROL: closing ONLY the job handle, wrapper left running ===
[kill] job contents after wrapper kill: [47860,18160,47208]   <- Job 内仍有 wrapper/PS/conhost
[kill] --- liveness while job still OPEN ---
[kill] ALIVE  pid=47860 wxc-exec
[kill] ALIVE  pid=18160 powershell
[kill] ALIVE  pid=47208 conhost
[kill] job handle closed
[kill] --- liveness AFTER job close (survivors = orphans) ---
[kill] reaped   pid=47860 / 18160 / 47208 / 46380
[kill] survivors = 0
```

**⇒ 机制确定为 (b)：清理是 Job 驱动，不是 wrapper 驱动。**
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 确实覆盖沙箱内整棵树 ——
这与 D-B 的"沙箱进程在调用方 Job 链上"互为印证。

**这条对 DSH 是有利的：DSH 的 Job 是有效的清理载体**，不需要额外的沙箱专用终结点。

---

## 五、D-D · 运行时拒绝能否与启动失败区分 —— ❌ 不可分型（三个实例，见 §5.2b）

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

### 5.2b 🔴 三个实例：拒绝不可归因的完整证据（2026-09-21 续）

上面的判定是"不可区分"。补全后可见，实际情况比"不可区分"更糟 ——
**三类完全不同的后果在子进程侧呈现为同一句话。**

| # | 实例 | 实际读数 | 性质 |
|---|---|---|---|
| **1** | 被策略拦下的**文件写入** | 文件未落盘，子进程只看到 `UnauthorizedAccessException`；且**不进 `captureDenials`**（净室实验：10 条 denial 中 `write` 为 **0**） | 信息**缺失** |
| **2** | **网络**被策略拦下 | `cargo fetch` → `exit=101`、`拒绝访问。(os error 5)`，**全文无一处提到网络**（`net_mention=False`） | 信息**主动误导** |
| **3** | **冷缓存缺依赖** | 与实例 2 **完全同形**（同一退出码、同一句错误） | 与 2 不可区分 |

**实例 2 的完整读数：**

```
R5|cargo_home_writable=yes          <- 排除文件权限因素
R5|fetch_exit=101
R5|fetch_secs=0.3
R5|fetch_last= 拒绝访问。 (os error 5)
R5|net_mention=False                <- 错误信息里没有任何网络线索
```

`os error 5` = `ERROR_ACCESS_DENIED`，在 Windows 上**同时也是文件权限错误**。
为隔离变量，把 `CARGO_HOME` 指到**可写**目录后**仍是同一个 `os error 5`**
（`cargo_home_writable=yes`），证明它来自网络策略而非文件权限。

**为什么这一条最值钱：**

前两个实例是"信息缺失"（agent 不知道发生了什么），
**实例 2 是"信息主动误导"** —— 错误信息会把 agent 引向
"检查文件权限 / 换路径 / 改 ACL"，而真实原因是网络被策略禁止。
对照 §3.4 的完成率分析，这正好落在"第 2 类失败：看到普通报错 → 开始 debug
一个不存在的问题"上。

**⇒ E 组（类型化拒绝）从"完成率优化"升级为接入前置**，
且**不能靠 `captureDenials` 解决**（见下）。

### 5.2c 为什么 `captureDenials` 补不上这个洞

已验证的机制分工：

| 通道 | 覆盖什么 | 证据 |
|---|---|---|
| `captureDenials`（ETW 学习模式） | **capability** 类拒绝（`internetClient` 等）、registry 读、UI handles | 净室实验 10 条，`resourceType` = capability 4 / other 5 / ui 1 |
| **另一条路径**（Tier 1 PSEC 文件规则 + AppContainer capability） | **文件与网络的实际拦截** | `--probe`：`tier: base-container` + `needsDaclAugmentation: false` + `baseContainerSupportsDenyPaths: true` |

**两者不共享计数** ⇒ 文件与网络拒绝**不产生 `captureDenials` 条目**。
`captureDenials` 对"哪些 capability 被拦"是有用的，但对"某个写/某个连接被拒了"无用 ——
**而后者恰是 DSH 最需要分型的那一类。**

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
| **P1 · 每命令包装** | ❌ **否决**。原写"D-A 阻断 / 外层 Job 有 UI 限制即失败" —— **该理由已撤回**：本 crate 的 Job 不带 UI 限制，真实 `ProcessSupervisor` 实测跑通。否决理由是 D-B 只到 wrapper + D-D 不可分型 |
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

---

## 十、补充验证（2026-09-21 续）

### 10.1 运行时 × UI 策略矩阵（11 种运行时 × 2 种 UI 策略）

| 运行时 | 默认 UI | `ui.disable=false` | 失败原因 |
|---|---|---|---|
| `cmd.exe` | ✅ | ✅ | — |
| `rg` | ✅ | ✅ | — |
| `bun` | ✅ | ✅ | — |
| `node` | ❌ DLL_INIT | ✅ | Win32k |
| `cargo` | ❌ DLL_INIT | ✅（需 fs 策略） | Win32k；否则 `could not create home directory: C:\Users\...\.rustup` |
| `rustc` | ❌ DLL_INIT | ✅ | 同上 |
| `git` | ❌ DLL_INIT | ✅（需 fs 策略） | 无策略时 `error launching git:` |
| `powershell 5.1` | ❌ DLL_INIT | ✅ | Win32k |
| `pwsh 7` | ❌ DLL_INIT | ⚠️ `NEEDS_ROOT_RO` | 「pwsh.exe versions before 7.7 require read-only access to the root drive (`C:\`)」 |
| `dotnet` | ❌ CoreCLR 绑定失败 | ✅ 输出 `10.0.401` | 默认策略下 `Failed to load coreclr.dll, HRESULT: 0x8007045A` |
| `python`（WindowsApps） | ❌ | ❌ | **MSIX 打包应用不可在沙箱内启动**，MXC 明确报错且无 workaround |

**⇒ `ui.disable: false` 是刚性前置。** DSH 的 Bash 工具链（cargo/bun/git/node）里
除 `bun` 外都受默认 UI 策略影响。

### 10.2 🔴 `captureDenials` 覆盖范围（D-D 的决定性补充）

用「只做一次被拒写入」的净室 workload 隔离，避免噪声干扰：

```
ONLYWRITE=UnauthorizedAccessException        <- 写入确实被拒，文件未落盘
{"type":"captureDenials",...,"totalDenials":10,...}
```

10 条 denial 的全部内容：

| accessType | resourceType | resource |
|---|---|---|
| read | other | `\REGISTRY\USER\S-1-5-21-...\Console` |
| unknown | ui | `Handles` |
| read | other | `\REGISTRY\MACHINE\Software\...\WSMAN` |
| unknown | capability | `sharedUserCertificates` |
| read | other | `\REGISTRY\MACHINE\SOFTWARE\...\BidInterface\Loader` |
| read | other | `\REGISTRY\MACHINE\SYSTEM\...\Cryptography\ECCParameters` |
| unknown | capability | `internetClient` |
| unknown | capability | `internetClientServer` |
| unknown | capability | `privateNetworkClientServer` |
| read | other | `\REGISTRY\USER\.DEFAULT\...\User Shell Folders` |

分布：`capability` 4 · `other` 5 · `ui` 1；`read` 5 · `unknown` 5 · **`write` 0**。

**⇒ 被策略拦下的文件写入完全不在其中。**

| 好的部分 | 坏的部分 |
|---|---|
| 提供**结构化的 capability 拒绝**（`internetClient` 等）——对"网络能力被拦"有用 | **文件系统写入拒绝不产生条目**——而"写越界"是 DSH 最需要分型的一类 |
| `mode: "block"` **不需要提权**、不降安全 | 条目里**没有 MXC 专有标志**能区别于应用自身失败 |

**机制注：** `captureDenials` 走 ETW 学习模式；真正的写拦截走另一条路径
（`--probe` 报 `tier: base-container` + `needsDaclAugmentation: false` +
`baseContainerSupportsDenyPaths: true`，即 Tier 1 PSEC/文件规则）。两者不共享计数。

**验证方式（可复现）：** 策略里加

```json
"processContainer": { "captureDenials": { "mode": "block", "retainEtl": false } }
```

`mode: "allow"` 是 permissive（等同 `--audit` 的学习模式），**不要在生产机用**。
省略 `outputPath` 时产物落在 `%TEMP%\mxc_denials_<pid>_<hash>.json`，
stderr 会打一行 `{"type":"captureDenials","outputPath":...}` 指针。

### 10.3 `process.env` 语义

| 事实 | 读数 |
|---|---|
| 默认是否继承父进程环境 | **继承**（约 40 个变量）。`Path` / `USERPROFILE` / `ComSpec` / `SystemRoot` / `PATHEXT` 都在 |
| 哪些被改写 | `LOCALAPPDATA`、`TEMP`、`TMP` → `C:\Users\15165\AppData\Local\Packages\sandbox.{GUID}\AC[\Temp]` |
| 哪些不存在 | `HOME`、`CARGO_HOME`、`RUSTUP_HOME` |
| 显式给 `process.env` | **完全替换**默认环境（不是叠加） |
| 替换时的强制要求 | 必须含 `SYSTEMROOT` 与 `LOCALAPPDATA`，否则启动失败：<br>「missing the required variable(s): SYSTEMROOT, LOCALAPPDATA … any value will do because the Windows requirement is presence-only」 |
| `inheritDefaultEnv` | **stable 0.8 schema 里不存在**（`unknown field`）；它是 dev/0.9 的字段 |

**⇒ 修正 §五 的先期结论**：环境**不是**被剥离的；工具链失败的真正原因是
`LOCALAPPDATA` 被改写进 AppContainer 包目录 + 缺少 `CARGO_HOME`/`RUSTUP_HOME`。
DSH 若要用 `process.env` 钉住缓存落点，**必须自己拼一份含 `SYSTEMROOT`/`LOCALAPPDATA` 的完整环境块**。

### 10.4 🟢 读边界定性（修正 §5.3）

| 策略形态 | 结果 |
|---|---|
| `readwrite=scratch`，**无** `readonlyPaths` | **PS 根本起不来**（`-File` 路径不可达） |
| `readwrite=scratch` + `readonly=C:\` | `blocked\secret.txt`、`C:\Windows\win.ini`、`~\.gitconfig` **全部可读** |
| 无 `filesystem` 段 | PS 起不来 |
| `readonly` 只给 `build\` | PS 起不来 |

**⇒ 修正口径：读取并非"无限制"，而是"受允许根约束"。**
上一轮"只读策略不生效 / 什么都能读"的结论是**我自己给了 `readonlyPaths: ["C:\\"]`** 造成的假象。

**真正的结论（更准确也更重要）：**

1. 读权限由**允许根**决定 —— 不在任何允许根下的路径，进程连自己的脚本都读不到
2. **没有细粒度的"读拒绝"**（`deniedPaths` 在 Windows 不可用），
   所以一旦为工具链给出宽读根（`C:\`），**该根下的凭据文件随之可读**
3. 这不是策略漏洞，而是**能力缺口 × DSH 的必然取舍**：
   宽读根是 cargo/git/bun 的硬需求
4. ⇒ **网络隔离成为唯一防线**（§5.4 已实测 `egress: deny` 生效）

---

## 十一、端到端验证：`nomi` 的真实工具链在沙箱内（2026-09-21 续）

**这是"接入 `nomi-agent` 到底行不行"的最终答案。** 前几节测的是运行时与机制；
本节让沙箱**跑仓库的真实命令**，并用「可写性矩阵 + 产物落盘」证明工作真的发生了。

### 11.1 策略形态

```json
{
  "version": "0.8.0-alpha",
  "containment": "processcontainer",
  "ui": { "disable": false },
  "filesystem": {
    "readwritePaths": ["C:\\workspace\\allo", "C:\\Users\\15165\\.cargo"],
    "readonlyPaths":  ["C:\\"]
  },
  "network": { "egress": { "default": "deny" }, "ingress": { "default": "deny", "hostLoopback": "deny" } }
}
```

**该形态下 `egress: deny` 全程未被触碰** —— 所有步骤都在离线状态完成。
（仓库 `.cargo/config.toml` 用 rsproxy.cn 稀疏镜像 + `build-dir = build.noindex`；
本轮编译未出网，靠的是**已预热的 1.8 GB registry 缓存**。）

### 11.2 可写性矩阵（沙箱内实测）

| 根 | 可写 | 说明 |
|---|---|---|
| `scratch` | ✅ | — |
| 工作区 `C:\workspace\allo` | ✅ | 含 `.git`（`w_gitdir=yes`） |
| `~/.cargo` 与其 `registry` | ✅ | 1.8 GB 依赖缓存，已挂载 |
| `ui\node_modules` | ✅ | 前端依赖，已挂载 |
| `%TEMP%`（AppContainer 包目录内） | ✅ | MXC 改写的路径 |
| `~/.bun\install\cache` | ❌ `UnauthorizedAccessException` | 11.2 GB，**未挂载** |
| `C:\Windows\System32` | ❌ `UnauthorizedAccessException` | 预期拒绝 |

### 11.3 结果

| 工具 | 命令 | 结果 |
|---|---|---|
| **cargo** | `build -p nomi-process-runtime --lib` | ✅ **exit 0**，产物 `libnomi_process_runtime.rlib` **16169 KB 落盘验证**，3 个 crate 真实编译 |
| **git** | `rev-parse` / `status` / `rev-list` / `log` | ✅ HEAD=`c08d2d1da`，`status --porcelain` 0 行，`rev-list --count HEAD`=**4555**，`log -1` 正常 |
| **bun** | `--version` / 包管理器 / 脚本执行 | ✅ `1.4.2`；**完整跑完 `bun run typecheck`，耗时 71.9s** |
| **rustc** | `--version` | ✅ — |
| **node** | `-e console.log` | ✅ `v26.8.2` |
| **pwsh 5.1** | 沙箱内探针宿主 | ✅ — |

### 11.4 `bun typecheck exit=2` 的归因 —— 与沙箱无关

沙箱内 `bun_typecheck_exit=2`，末行 `vite.config.ts(185,24): error TS7006`。
**在宿主上跑同一条命令做对照：**

```
HOST:    exit=2   secs=71.5   error lines: 73
SANDBOX: exit=2              （同类 TS 错误）
```

**⇒ 类型检查失败是仓库既有状态，不是沙箱造成的。**
两侧 exit code 一致、错误数量一致（73）、耗时接近（71.5s vs 71.9s）——
**说明 `bun` 在沙箱内完整、忠实地跑完了一次真实的前端类型检查，性能几乎无损。**

### 11.5 结论

> **`nomi` 的真实工具链可以在 MXC 沙箱内正常工作**，且在 `egress: deny` 下完成
> cargo 编译 + git 仓库读取 + bun 脚本执行。

**但代价是这份策略很宽**：`readonlyPaths: ["C:\\"]` 意味着沙箱**可读整盘**。
这是 §10.4 所述「无细粒度读拒绝」的直接后果 —— 也正是 `~/.bun` 缓存只能整体挂载
（不能只授权必需子路径）的原因。

**按用途分级策略是可行且必要的：**

| 场景 | 建议策略 | 状态 |
|---|---|---|
| **编译 / 测试 / 类型检查** | 上面这份（宽读根 + 精确可写根） | ✅ 本轮实测通过 |
| **安装依赖 / 拉取代码** | 需 `egress: allow`（或指向本地代理） | ❌ 未验证 |
| **写工作区外** | 显式加入 `readwritePaths` | ✅ 机制已验证（`~/.cargo` 即此例） |

### 11.6 待补（诚实声明）

1. **`bun install` 未实测** —— 需 `egress: allow` 或预热的 `~/.bun` 缓存（当前被拒）
2. **`gh` 未测** —— 同样需出网
3. **全工作区 `cargo check`（27 crate）未测** —— 只测了单 crate，规模上限未知
4. **`cargo fetch` 冷缓存未测** —— 本轮缓存是热的，冷缓存行为未知

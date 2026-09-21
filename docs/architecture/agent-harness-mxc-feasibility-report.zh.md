# MXC 接入可行性汇报

> **日期：** 2026-09-21 · **性质：结论性汇报**（决策文档），证据为时点快照
> **分支：** `feat/mxc-feasibility-verification`（基线 `origin/main` @ `37701ba85`）
> **验证环境：** Windows 11 Insider build **29671** · MXC `ca7ea12ac6bd9f5420d6adecb37e32a8158da476`（`wxc-exec` 0.8.0，本机构建）
> **原始证据：** [`agent-harness-mxc-verification-record.zh.md`](agent-harness-mxc-verification-record.zh.md)
> **验证清单：** [`agent-harness-mxc-process-wrapper-feasibility.zh.md`](agent-harness-mxc-process-wrapper-feasibility.zh.md)
> **上游语境：** [`agent-harness-permission-approval-sandbox.zh.md`](agent-harness-permission-approval-sandbox.zh.md)

---

## 一、结论

> **不建议在当前状态下把 MXC 接入为 DSH 的执行边界。**
> 但**建议立即接入它的学习/采集面**（`captureDenials`），因为它零风险、
> 不需要提权、且能产出 DSH 目前完全缺失的能力面证据。

**三条决定性理由（每一条都独立足以否决"现在就当边界用"）：**

| # | 理由 | 证据强度 |
|---|---|---|
| 1 | **UI 策略与 DSH 的 Job 模型互斥** —— MXC 要设 UI 限制，DSH 必须持有 Job 才能证明进程树清理。两者同时成立时 `processcontainer` 每次 spawn 都失败 | 强：端到端 + 内核层双重复现 |
| 2 | **被策略拦下的文件写入不会出现在 `captureDenials` 里** —— 主机拿不到文件类拒绝的分类，无法支撑 E 组的 `RetryDecision` | 强：净室隔离实验 |
| 3 | **默认策略下 DSH 的主力工具链跑不起来**，需要一次性打开多项策略开关（含 MSIX 的 Python 无解） | 强：11 × 2 运行时矩阵 |

**可行的起点是 `captureDenials` 而不是强制隔离**：它给的是**能力面证据**
（被拦的 capability 清单），这正是 B 组策略学习缺的那部分输入。

---

## 二、验证覆盖面

| 门 | 状态 | 一句话结论 |
|---|---|---|
| D-A | ✅ 完成 | 外层 Job 带 UI 限制 → `ERROR_NOT_SUPPORTED(50)` |
| D-B | ✅ 完成 | DSH 只持 wrapper 身份，沙箱进程是另一个 PID |
| D-C | ✅ 完成 | **机制确定**：清理由 Job 驱动（关 Job 即杀树），非 wrapper 驱动 |
| D-D | ✅ 完成 | 拒绝生效但**不可分型**；`captureDenials` 抓不到文件类拒绝 |
| D-E | ✅ 完成 | **放弃 P1**；state-aware 三个后端，本机只能走 `windows_sandbox`，**但该功能未启用** |
| 附 | ✅ 完成 | 运行时×UI 矩阵 · 环境变量语义 · 读边界 · `captureDenials` 覆盖范围 |

**未完成的唯一一项**：`windows_sandbox` 的 state-aware 会话**实跑**。
本机 `Containers-DisposableClientVM` 为 **Disabled**、`WindowsSandbox.exe` 不存在，
启用需 `Enable-WindowsOptionalFeature` + **重启**——未擅自执行（见 §六 待决）。

---

## 三、关键发现（每条都有原始读数支撑）

### 3.1 🔴 UI 策略 × Job 模型互斥（D-A）

| 外层 Job | 结果 |
|---|---|
| 无 | ✅ `WXC_RAN_OK`，exit 0 |
| `KILL_ON_JOB_CLOSE` + UI 限制 `0x10` | ❌ exit `0xFFFFFFFF`，`CreateProcessW ... WIN32_ERROR(50)` |
| `KILL_ON_JOB_CLOSE` 无 UI 限制 | ✅ `WXC_RAN_OK`，exit 0 |

内核层独立复现（不依赖 MXC）：子进程即时 Job UI 限制 `0x0` → `AssignProcessToJobObject` 成功；
`0x10` → **win32=50**。与官方 "neither job sets UI limits" 条款一致。

**冲突的实质**：DSH 的 `arm_process_job`（`platform/windows.rs:1677-1695`）必须建 Job 来承载
`ChildProcessCleanup` 的"清理已证明"语义；而 MXC 必须设 UI 限制才能拦剪贴板/输入注入。
**两者不可同时满足。**

**注意一个反直觉的限定条件**（本轮新发现）：**"调用方自己给子进程设了 UI 限制"并不触发失败。**
测试中 `job_ui=0x0` 出现在**调用方持有 UI 限制 Job** 的情形下且工作正常；
只有**外层 Job 自身带 UI 限制**才失败。

### 3.2 ✅ D-C 机制确定：Job 驱动，非 wrapper 驱动

| 实验 | 结果 |
|---|---|
| 强杀 wrapper | Job 内容清空，6 个进程全部 reaped，**0 survivors** |
| **只关 Job、不杀 wrapper**（对照） | Job 内容为空，但 wrapper/沙箱 PS/conhost 在关 Job 前**仍 ALIVE**；关 Job 后**全部 reaped，0 survivors** |

**⇒ JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE 确实覆盖沙箱内整棵树。**
这消除了验证清单里"D-C 机制未定"的保留：**DSH 的 Job 是有效的清理载体**。

### 3.3 🔴 `captureDenials` 抓不到文件类拒绝（D-D 的决定性阻断）

**用一个"只做一次被拒写入"的净室 workload 隔离验证：**

```
ONLYWRITE=UnauthorizedAccessException      <- 写入确实被拒
totalDenials: 10                            <- 有捕获，但内容是：
  [read   ] other      \REGISTRY\USER\...\Console
  [unknown] ui         Handles
  [unknown] capability sharedUserCertificates
  [unknown] capability internetClient
  [unknown] capability internetClientServer
  [unknown] capability privateNetworkClientServer
  [read   ] other      \REGISTRY\MACHINE\SYSTEM\...\ECCParameters
  ...
>>> 被拒的 blocked\only.txt 写入完全不在其中
```

分布：`capability` 4 · `other` 5 · `ui` 1；`read` 5 · `unknown` 5 · **`write` 0**。

**判读：**
- ✅ **好消息**：`captureDenials` 提供**结构化的 capability 拒绝**（`internetClient` 等），
  这对"哪些网络能力被拦"是有用的分类
- ❌ **坏消息**：**被策略拦下的文件系统写入不产生任何 capture 条目**。
  主机从 `denials.json` 里读不到"某个写被拒了"
- ⇒ 而"写越界"恰恰是 DSH 最需要分型的那一类（`cargo`/`bun` 要写工作区外）

**这两条合起来改写了 D-D 的结论**：不是"没有结构化数据"，而是
**"有结构化数据，但它与 DSH 关心的拒绝类型正交"**。

（机制注：`captureDenials` 走 ETW 学习模式；
真正的写拦截走**另一条路径**——`--probe` 报的 `tier: base-container` +
`needsDaclAugmentation: false` + `baseContainerSupportsDenyPaths: true`，
即 Tier 1 的 PSEC/文件规则。两者不共享计数。）

### 3.4 🟡 运行时 × UI 策略矩阵（11 × 2）

| 运行时 | 默认 UI | `ui.disable=false` | 失败原因 |
|---|---|---|---|
| `cmd.exe` | ✅ | ✅ | — |
| `rg` | ✅ | ✅ | — |
| `bun` | ✅ | ✅ | — |
| `node` | ❌ DLL_INIT | ✅ | Win32k |
| `cargo` | ❌ DLL_INIT | ✅ | 配合 fs 策略后 OK |
| `rustc` | ❌ DLL_INIT | ✅ | 同上 |
| `git` | ❌ DLL_INIT | ✅（需 fs 策略） | 无策略时 `error launching git` |
| `powershell 5.1` | ❌ DLL_INIT | ✅ | Win32k |
| `pwsh 7` | ❌ DLL_INIT | ⚠️ NEEDS_ROOT_RO | 7.7 以下需根盘只读 |
| `dotnet` | ❌ CoreCLR 绑定失败 | ✅ `10.0.401` | 默认策略下 CoreCLR 起不来 |
| `python`（WindowsApps） | ❌ | ❌ | **MSIX 打包应用不可在沙箱内启动，无解** |

**⇒ 结论：`ui.disable: false` 是刚性前置**，不是可选优化。
且 `LOCALAPPDATA` 被 MXC 改写进 AppContainer 包目录
（`...\Packages\sandbox.{GUID}\AC`），这会改变工具链的缓存/配置落点。

### 3.5 🟢 只读边界比初判更好（纠正 §上一轮结论）

| 策略形态 | 结果 |
|---|---|
| `readwrite=scratch`，**无** `readonlyPaths` | **PS 根本起不来**（`-File` 路径不可达） |
| `readwrite=scratch` + `readonly=C:\` | 读 `blocked\secret.txt`、`C:\Windows\win.ini`、`~\.gitconfig` **全部 OK** |

**⇒ 读取并非"无限制"，而是"受允许根约束"**：给了 `readonlyPaths: ["C:\\"]`
才导致"什么都能读"。**上一轮"只读策略不生效"的表述需要修正为**：
*读权限由允许根决定，不提供细粒度的读拒绝（`deniedPaths` 在 Windows 不可用）；
DSH 为了让工具链工作必须给出宽读根，于是凭据目录随之可读。*

**这不是策略漏洞，是能力缺口 + DSH 的必然取舍**：宽读根是工具链的硬需求，
于是**网络隔离成为唯一防线**（实测 `egress: deny` 生效）。

### 3.6 🟡 `process.env` 语义（影响能否修正工具链行为）

| 事实 | 说明 |
|---|---|
| 默认继承 ~40 个父进程变量 | `Path`、`USERPROFILE`、`ComSpec`、`SystemRoot` 都在；`HOME`/`CARGO_HOME`/`RUSTUP_HOME` **不在** |
| `LOCALAPPDATA` / `TEMP` / `TMP` | **被改写**到 `...\Packages\sandbox.{GUID}\AC[\Temp]` |
| 显式给 `process.env` | **完全替换**默认环境，不是叠加（stable 0.8 无 `inheritDefaultEnv`；该字段是 dev/0.9） |
| 替换时的强制要求 | 必须含 **`SYSTEMROOT` 与 `LOCALAPPDATA`**，否则启动失败（MXC 报错明确点名） |

**⇒ DSH 若想钉住 `CARGO_HOME`/`TEMP` 到可写目录，必须自己拼一份完整环境块**，
把 `SYSTEMROOT`/`LOCALAPPDATA` 一起带上。这是一条**可实施的修正路径**。

---

## 四、可行性判定

### 4.1 形态判定

| 形态 | 判定 | 依据 |
|---|---|---|
| **P1 · 每命令包装** | ❌ **否决** | §3.1 UI×Job 互斥；§3.3 拒绝不可分型 |
| **P1 + R1（DSH 让出 Job）** | ❌ **否决** | 解决 §3.1，但 §3.3 仍不可分型；且放弃 DSH 自身清理兜底 |
| **P2 · 长驻沙箱会话** | ⚠️ **唯一未否决，但未实跑** | 本机只能走 `windows_sandbox`，而该功能 **Disabled** |
| **仅采集面（`captureDenials`）** | ✅ **建议立即采用** | §3.3 证明它在能力面有价值且零成本 |
| **不接入，维持现状** | ✅ 作为兜底 | 但 Windows 继续零隔离 |

### 4.2 三条独立否决理由的强度

1. **§3.1（UI×Job）** —— 强度最高。有端到端 + 内核层双重证据，
   且**有明确的机制解释**（Windows 嵌套作业的 UI 限制条款）。
   不是"配置问题"，是**两个设计假设的直接冲突**。
2. **§3.3（拒绝不可分型）** —— 强度高。净室隔离实验，且用"只做一次被拒写入"的
   workload 排除了噪声干扰。
3. **§3.4（工具链）** —— 强度高但**可工程化绕过**：`ui.disable: false`
   + 宽读根 + 完整 env 块 + 可写根，是能配出来的。**成本是策略复杂度**，
   不是不可能。python 的 MSIX 除外（无解）。

**⇒ 真正拦住 MXC 的是 1 和 2，不是 3。** 3 只是抬高接入成本。

---

## 五、建议

### 5.1 立即做（零风险、不依赖任何未决项）

| # | 行动 | 理由 |
|---|---|---|
| **1** | **接入 `captureDenials`（`mode: "block"`）作为能力面采集**：在隔离机上跑 DSH 的真实任务，收集 `capabilities` 类拒绝 | 零提权、不降安全（`block` 非 permissive）、产出 DSH 缺失的能力面证据 |
| **2** | **把 B 组策略学习改用 `captureDenials` 而非 `--audit`** | `--audit` 注入 `permissiveLearningMode`（AppContainer 限制**不被强制**），只能在隔离机跑；`captureDenials.mode:"block"` 保持强制 |
| **3** | **E 组按"越界即 `Abort`"落地** | §3.3 已判不可分型，E 组的设计前提已变 |
| **4** | **向 MXC 上游提两个 issue** | ① `processcontainer` 在带 UI 限制的宿主 Job 内无法启动（可复现）② 文件类策略拒绝不进 `captureDenials` |

### 5.2 需要先拍板才能做

| # | 行动 | 待决 |
|---|---|---|
| **5** | 实跑 `windows_sandbox` 的 state-aware 会话 | 需启用 Windows 功能 **+ 重启**（见 §六） |
| **6** | 是否接受 `ui.disable: false` 带来的 UI 隔离损失 | 该开关同时放开 Win32k，削弱剪贴板/输入注入隔离 |

### 5.3 明确不建议

- ❌ 把 MXC 当作"现在就能生效的安全边界"接入（README 自陈"任何 MXC profile 都不应被视为安全边界"）
- ❌ 为接入 MXC 而让 DSH 放弃 Job（§4.1 P1+R1）
- ❌ 在用户机器上跑 `--audit`

### 5.4 与既有文档的关系

- 本文的 §3.1/§3.3 应作为**新证据**补进
  [`agent-harness-permission-approval-sandbox.zh.md`](agent-harness-permission-approval-sandbox.zh.md) §层 2 与 §层 3
- **层 3（类型化拒绝）的设计前提需要重审**：原设想"从 MXC 拒绝里取分类码"，
  现证明文件类拒绝无分类码 → 层 3 必须回到"DSH 侧自行分类（按调用点/参数）"
- 层 0（策略写保护）与层 8（闭环）**不受本文结论影响**，仍是独立优先项

---

## 六、待决（需要人类决定）

| # | 问题 | 影响 | 建议 |
|---|---|---|---|
| **D-1** | 是否启用 `Containers-DisposableClientVM` 并**重启本机**以实跑 `windows_sandbox` 的 state-aware 会话？ | 决定 P2 是"唯一出路"还是"也走不通" | 若 P2 是主路径则必须启用；否则 P2 只能留作未验证 |
| **D-2** | 是否接受 `ui.disable: false`（放开 Win32k）作为接入前提？ | 削弱 UI 隔离；但不开则 node/cargo/git/PS 全部不可用 | 建议接受，并把它记入策略基线而非隐藏默认值 |
| **D-3** | DSH 是否愿意为沙箱会话**放弃自身 Job**（若最终走 P1+R1）？ | 影响清理证明的承载方式 | 倾向不走此路（§4.1） |
| **D-4** | MSIX 打包的 Python 在沙箱内无解，是否接受"沙箱会话内无 Python"？ | 影响 `python` 类工具与脚本技能 | 需产品侧确认可接受性 |

---

## 七、验证成本与可复现性

| 项 | 值 |
|---|---|
| 分支 | `feat/mxc-feasibility-verification` |
| MXC 构建 | `cargo build -p wxc`，约 1m23s（dev） |
| 脚本 | `target/mxc-feasibility/`（gitignored，未入库）；逻辑已完整记入验证记录 §九 |
| 复现关键 | 所有沙箱内策略**必须**带 `"ui": { "disable": false }`；探针运行时用 **PowerShell 5.1**（pwsh 7 起不来） |

---

## 八、一页速览

```
问：MXC 现在能当 DSH 的执行边界吗？
答：不能。三条独立理由：UI×Job 互斥 / 文件类拒绝不可分型 / 工具链需多处开口。

问：哪一条最致命？
答：UI×Job 互斥 —— 它是两个设计假设的直接冲突，不是配置问题。

问：那 MXC 现在对 DSH 有什么用？
答：captureDenials 的采集面。零提权、不降安全、产出 DSH 缺失的能力面证据。
    建议把 B 组策略学习从 --audit 改为 captureDenials.mode=block。

问：还有多少不确定？
答：一项 —— windows_sandbox 的 state-aware 会话未实跑（本机功能 Disabled，需重启）。
    其余五门 + 四个附属项都已实测。

问：下一步最该做什么？
答：① 接 captureDenials 采集 ② E 组改"越界即 Abort" ③ 提两个上游 issue
    ④ 拍板是否启用 Windows Sandbox 功能以验证 P2
```

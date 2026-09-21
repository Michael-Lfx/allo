# MXC 接入可行性汇报

> **日期：** 2026-09-21 · **性质：结论性汇报**（决策文档），证据为时点快照
> **分支：** `feat/mxc-feasibility-verification`（基线 `origin/main` @ `37701ba85`）
> **验证环境：** Windows 11 Insider build **29671** · MXC `ca7ea12ac6bd9f5420d6adecb37e32a8158da476`（`wxc-exec` 0.8.0，本机构建）
> **原始证据：** [`agent-harness-mxc-verification-record.zh.md`](agent-harness-mxc-verification-record.zh.md)
> **风险登记：** [`agent-harness-mxc-risk-register.zh.md`](agent-harness-mxc-risk-register.zh.md)
> —— **本文答"能不能做"，风险清单答"做了会踩什么"。可行性验证不覆盖后者。**
> **验证清单：** [`agent-harness-mxc-process-wrapper-feasibility.zh.md`](agent-harness-mxc-process-wrapper-feasibility.zh.md)
> **上游语境：** [`agent-harness-permission-approval-sandbox.zh.md`](agent-harness-permission-approval-sandbox.zh.md)

---

## 一、结论

> **结论（2026-09-21 两次校订后）：MXC 在技术上已经可用 ——
> `nomi` 的真实工具链能在沙箱内跑通（cargo / git / bun，且 `egress: deny`）；
> 缺的不是能力，是"越界可归因"这一环。**
>
> **建议：先在非交互/离线场景试点，同时把 E 组（类型化拒绝）提升为接入前置。**
> 并立即接入 `captureDenials` 的采集面 —— 它零风险且产出目前完全缺失的能力面证据。

**唯一剩余的拦路石：**

| # | 理由 | 证据强度 |
|---|---|---|
| **1** | **被策略拦下的文件写入不会出现在 `captureDenials` 里** —— 越界不可归因，无法支撑 E 组的 `RetryDecision` | 强：净室隔离实验 |
| 2 | **读权限需宽读根、且无细粒度读拒绝** —— 凭据目录随之可读，网络隔离成为唯一防线 | 强：策略形态对照 |

**已撤回的两条（供追溯）：**

| ~~初版理由~~ | 撤回依据 |
|---|---|
| ~~UI 策略与 Job 模型互斥~~ | `nomi-process-runtime` 的 Job **不带 UI 限制**；用真实 `ProcessSupervisor` 实测跑通 `wxc-exec`（§3.1） |
| ~~默认策略下工具链跑不起来~~ | 端到端实测：`ui.disable:false` + 宽读根 + 精确可写根下，cargo 编译 / git 读取 / bun typecheck 全部正常（§3.7） |

**方法论教训（写进本文档以免重犯）：** 初版用 PowerShell **替身** harness 复刻
`arm_process_job` 的形状，再把结论外推到"本仓库不能用"。
**替身能验证 Windows 语义，不能验证宿主代码路径。** 凡涉及本仓库可否接入的结论，
一律以 `crates/shared/nomi-process-runtime/tests/mxc_supervision_probe.rs`
（走真实 `ProcessSupervisor`）为准。

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

### 3.1 ⚠️ UI 策略 × Job 模型：条件性，**不命中 `nomi-process-runtime`**

| 外层 Job | 结果 |
|---|---|
| 无 | ✅ `WXC_RAN_OK`，exit 0 |
| `KILL_ON_JOB_CLOSE` + UI 限制 `0x10` | ❌ exit `0xFFFFFFFF`，`CreateProcessW ... WIN32_ERROR(50)` |
| `KILL_ON_JOB_CLOSE` **只此一项** | ✅ `WXC_RAN_OK`，exit 0 |

内核层独立复现（不依赖 MXC）：子进程即时 Job UI 限制 `0x0` → `AssignProcessToJobObject` 成功；
`0x10` → **win32=50**。与官方 "neither job sets UI limits" 条款一致。

**⇒ 决定变量是「外层 Job 是否带 UI 限制」。**

**🔴 真实代码路径实测（`tests/mxc_supervision_probe.rs`）：** 用本 crate 的
`ProcessSupervisor` 启动 `wxc-exec.exe`：

```
Exited { code: Some(0), output: "NOMI_SUPERVISED_OK",
         cleanup: CleanupReport { reaped: true, errors: [] } }
```

而 `nomi-process-runtime` 的 `arm_process_job`（`platform/windows.rs:1677-1695`）
**只设 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`**，无 UI 限制
⇒ **落在成功那一行，D-A 不构成阻断。**

**⚠️ 这是未来风险而非当前缺陷**：若给该 Job 加上 UI 限制，D-A 立即生效。
建议在 `arm_process_job` 处留注释，并把这个条件写进接入时的评审清单。

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

> **⚠️ 本节初版据此把"工具链"列为否决理由之一，该定位已被 §3.7 的端到端结果推翻**：
> `ui.disable: false` 是**可配置的一次性前置**，不是不可逾越的障碍。

### 3.7 ✅ 端到端验证：`nomi` 的真实工具链在沙箱内**能跑**（新增，2026-09-21）

在 `ui.disable: false` + 宽读根 + 精确可写根 + **`egress: deny`** 下，让沙箱跑仓库真实命令：

| 工具 | 命令 | 结果 |
|---|---|---|
| **cargo** | `build -p nomi-process-runtime --lib` | ✅ exit 0，`libnomi_process_runtime.rlib` **16169 KB 落盘验证**，3 crate 真实编译 |
| **git** | `rev-parse` / `status` / `rev-list` / `log` | ✅ HEAD `c08d2d1da`，`rev-list --count HEAD`=**4555** |
| **bun** | 包管理器 + 脚本执行 | ✅ `1.4.2`，**完整跑完 `bun run typecheck`（71.9s）** |
| node / rustc | version / eval | ✅ |

**`bun typecheck` 报 exit=2 与沙箱无关** —— 宿主对照同为 `exit=2`、同为 **73 个** TS 错误、
耗时 71.5s vs 71.9s。**⇒ 真实前端类型检查在沙箱内完整跑完，性能几乎无损。**

**可用策略（已验证）：**

```json
{ "ui": { "disable": false },
  "filesystem": { "readwritePaths": ["<workspace>", "<CARGO_HOME>"], "readonlyPaths": ["C:\\"] },
  "network": { "egress": { "default": "deny" } } }
```

**⇒ 这一条把"工具链"从否决理由降级为"可配置前置"。**
代价是策略宽（`readonlyPaths: ["C:\\"]` = 可读整盘，即 §3.3 的后果），
但**编译 / 测试 / 类型检查这一整类任务在离线沙箱内是可行的**。

**未验证的边界：** `bun install` / `gh`（需出网）· 全工作区 27 crate ·
冷缓存 `cargo fetch`。

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
| **P1 · 每命令包装** | ⚠️ **可试点（有条件）** | 唯一剩余拦路石是 §3.3 拒绝不可分型；**Job 冲突已撤回**（§3.1）、**工具链已验证可跑**（§3.7）。建议先在**非交互/离线**场景试点 |
| **P1 + R1（调用方让出 Job）** | ❌ **不必要** | R1 原本是为解 Job 冲突；该冲突不成立，故 R1 不再需要 |
| **P2 · 长驻沙箱会话** | ⚠️ **未否决，但未实跑** | 本机只能走 `windows_sandbox`，而该功能 **Disabled** |
| **仅采集面（`captureDenials`）** | ✅ **建议立即采用** | §3.3 证明它在能力面有价值且零成本 |
| **不接入，维持现状** | ✅ 作为兜底 | 但 Windows 继续零隔离 |

### 4.2 唯一剩余的拦路石

**§3.3（文件类拒绝不可分型）** —— 强度高。净室隔离实验，用"只做一次被拒写入"的
workload 排除了噪声干扰。**这是当前唯一把 P1 从"可行"压回"有条件试点"的因素。**

已撤回的两条理由（供追溯）：

- ~~§3.1 UI×Job 互斥~~ —— 条件不成立；`nomi-process-runtime` 的 Job 不带 UI 限制，实测跑通
- ~~§3.4 工具链不可用~~ —— 已由 §3.7 端到端实测推翻：cargo / git / bun 在离线沙箱内正常

**⇒ 结论收敛为：技术上能跑，缺的是"拒绝可归因"这一环。**
这恰好把优先级指向 **E 组**（类型化拒绝）——它不再只是"完成率优化"，
而是**接入 MXC 的前置**。

---

## 五、建议

### 5.1 立即做（零风险、不依赖任何未决项）

| # | 行动 | 理由 |
|---|---|---|
| **1** | **接入 `captureDenials`（`mode: "block"`）作为能力面采集**：在隔离机上跑真实任务，收集 `capabilities` 类拒绝 | 零提权、不降安全（`block` 非 permissive）、产出缺失的能力面证据 |
| **2** | **把 B 组策略学习改用 `captureDenials` 而非 `--audit`** | `--audit` 注入 `permissiveLearningMode`（限制**不被强制**），只能在隔离机跑；`captureDenials.mode:"block"` 保持强制 |
| **3** | **E 组优先做，且定位升级为"接入前置"** | §3.3 判定文件类拒绝不可分型。E 组不再是完成率优化，而是让越界可归因的**必需件** |
| **4** | **把 §3.7 已验证的策略固化为"沙箱化会话（离线编译/测试）"的基线** | `ui.disable:false` + 宽读根 + 精确可写根 + `egress:deny` 已实测可跑 cargo/git/bun |
| **5** | **向 MXC 上游提 issue** | 文件类策略拒绝不进 `captureDenials`（可复现） |

### 5.2 需要先拍板才能做

| # | 行动 | 待决 |
|---|---|---|
| **6** | 在**非交互 / 离线**场景试点 P1（编译、测试、类型检查） | 需接受 `ui.disable:false` 与宽读根（见 §六 D-2） |
| **7** | 实跑 `windows_sandbox` 的 state-aware 会话 | 需启用 Windows 功能 **+ 重启**（见 §六 D-1） |

### 5.3 明确不要做

- ❌ 把 MXC 当作"现在就能生效的完整安全边界"接入（README 自陈"任何 MXC profile 都不应被视为安全边界"）
- ❌ 在**交互式**场景先试点（那会立刻撞上 §3.3 的不可归因 + 无人值守超时未定义）
- ❌ 为接入 MXC 而让 `nomi-process-runtime` 放弃 Job（R1 已不必要）
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
问：MXC 现在能接入吗？
答：技术上能。nomi 的真实工具链已在沙箱内跑通（cargo 编译 + git 读取 +
    bun typecheck），而且是 egress: deny 离线跑通的。

问：那还差什么？
答：一样东西 —— 越界不可归因。被策略拦下的文件写入不进 captureDenials，
    所以 DSH 分不清"策略拒绝"和"程序自己没权限"。
    这正是 E 组（类型化拒绝）的活，它因此从优化项升级为接入前置。

问：之前说"UI×Job 互斥"和"工具链跑不起来"，还算数吗？
答：都不算数，已撤回。
    前者：nomi 的 Job 只设 KILL_ON_JOB_CLOSE，真实 ProcessSupervisor 实测跑通。
    后者：ui.disable:false + 宽读根 + 精确可写根 = cargo/git/bun 全部正常。

问：代价是什么？
答：策略宽。readonlyPaths: ["C:\"] 是可读整盘，因为 Windows 没有细粒度读拒绝。
    于是网络隔离（egress: deny，已实测生效）成为唯一防线。

问：还有多少不确定？
答：① windows_sandbox 的 state-aware 会话未实跑（本机功能 Disabled，需重启）
    ② bun install / gh 未测（都需出网）
    ③ 全工作区 27 crate 与冷缓存 cargo fetch 未测

问：下一步最该做什么？
答：① E 组（接入前置）② 接 captureDenials 采集
    ③ 在非交互/离线场景试点 P1 ④ 提上游 issue（文件类拒绝不进 captureDenials）
```

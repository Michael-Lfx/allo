# MXC 接入风险清单

> **日期：** 2026-09-21 · **性质：风险登记册**（与验收文档分开：*可行* ≠ *可接入*）
> **分支：** `feat/mxc-feasibility-verification`（基线 `origin/main` @ `37701ba85`）
> **环境：** Windows 11 build 29671 · MXC `ca7ea12`（`wxc-exec` 0.8.0，本机构建）
>
> **本文与另两份文档的分工：**
> - [`agent-harness-mxc-feasibility-report.zh.md`](agent-harness-mxc-feasibility-report.zh.md) —— **能不能做**（结论：能）
> - [`agent-harness-mxc-verification-record.zh.md`](agent-harness-mxc-verification-record.zh.md) —— **凭什么这么说**（原始读数）
> - **本文 —— 做了之后会踩什么**（可行性验证**不覆盖**这一层）
>
> **读法：** 可行性验证证明的是**充分性**（这条路能走通），
> 不是**安全性**。本文登记的是充分性之外的全部已知风险，
> 分「已实测」「未验证」「架构未动」三类。

---

## 一、已实测的风险（本轮有读数）

### R1 🔴 冷缓存 + `egress: deny` = 不透明失败

```
fetch_exit=101   fetch_secs=0.3   拒绝访问。(os error 5)
net_mention=False
```

**关键读数：错误信息里完全没有网络字样，只有裸的"拒绝访问"。**
而 `os error 5`（`ERROR_ACCESS_DENIED`）在 Windows 上同时也是**文件权限**错误。

**⇒ 沙箱把"网络被禁"与"文件不可写"呈现成同一个错误。**
依赖缺失、网络被策略禁、目录不可写——**agent 与 cargo 都无法区分**。

这是「拒绝不可归因」（见 §四 D-D）的**第三个实例，也是最糟的一个**：
前两个是"信息缺失"，这一个是**信息主动误导**。
（另注：本轮为隔离变量，`CARGO_HOME` 指到可写目录后**仍是同一个 `os error 5`**，
说明它确实来自网络策略而非文件权限。）

### R2 🟡 `~/.bun/install/cache` 被拒 → `bun install` 必然失败

| 根 | 可写 | 大小 |
|---|---|---|
| `~/.bun\install\cache` | ❌ `UnauthorizedAccessException` | **11.2 GB** |
| `~/.cargo` + `registry` | ✅ | 1.8 GB |

体积使"整体挂载"成为唯一选项（无法只授权必需子路径，因为 Windows 没有细粒度读/写拒绝）。
**在现有策略形态下 `bun install` 不可用。**

### R3 🟡 `taskkill` 在沙箱内失败

```
C_taskkill_exit=1
ERROR: The user name or password is incorrect.
```

按 PID 打开进程需要 `PROCESS_TERMINATE`，AppContainer 拿不到。

**功能风险：** 任何**按 PID 杀进程**的工具（含 `taskkill`、部分测试运行器、
`Stop-Process`）在沙箱内失败。
**未测的对照：** 持有句柄时的 `TerminateProcess` —— 多数测试运行器走这条，通常可用。
**⇒ 影响面需要单独界定，不能假定"能跑 cargo 就能跑 cargo test"。**

### R4 🟡 进程扇出行为异常

```
A_spawned=12   A_alive_after_spawn=8   A_still_running_after_3s=0
```

12 个并发子进程创建后**只有 8 个存活**，3 秒后**归零**。
而 cargo 的并行编译（多个 rustc）在同期**正常完成**。

**⇒ 沙箱对子进程生命周期施加了某种压制，机制未知。真实并发上限未测。**
对本仓库尤其相关：`AGENTS.md` 声明约 15,900 个测试、`cargo nextest` 高度并行。

### R5 🟡 `RUSTUP_HOME` 绝不可重定向（踩过）

把 `RUSTUP_HOME` 指向空目录 → 工具链直接不可用，而报错信息**误导**为
`help: run 'rustup default stable' to download the latest stable release`。

**配套事实：`process.env` 是「完全替换」而非「叠加」**（stable 0.8 无 `inheritDefaultEnv`），
会连带丢掉 `PATH` / `SystemRoot` / `LOCALAPPDATA`。
**⇒ 若要用环境变量钉缓存落点，只能钉 `CARGO_HOME` 一类，且必须保留完整基础环境块。**

### R6 🟢 AppContainer 配置文件不泄漏

```
当前 sandbox.* 配置数: 0
```

多轮沙箱运行后**零残留**，MXC 自行清理容器资料。**这条是好消息**，无需治理。

### R7 🟡 工作区可写 ⇒ 沙箱内可改构建执行面

`.cargo/config.toml` 在**工作区内且被 git 跟踪**，而沙箱需要工作区可写。

**⇒ 沙箱内的进程可以改写它**（改 registry 源、改 linker、加 `rustflags`）。
`build.rs` 与之叠加后，形成"工作区可写 ⇒ 构建时可执行任意代码"的链路。

**注：** `egress: deny` 使这条**不是任意代码执行**（也出不了网），但它是
**沙箱内的构建面完整性**风险，且改动会进 git diff。

---

## 二、未验证的技术风险（真正的未知）

| # | 风险 | 为什么重要 | 现状 |
|---|---|---|---|
| **U1** | ~~`egress: allow` 下的实际行为~~ | `bun install` / `gh` / 拉代码全都要网络——**日常开发的硬需求** | **🔴 已测 → 见 §2.5：被本地代理链条卡死** |
| **U2** | **全工作区 27 crate + `tauri build`** | 只测单 crate。`tauri` 会把 `ui/dist`（约 34 MB）嵌进二进制，而仓库 `.cargo/config.toml` 自陈这会让 rustc **栈溢出**（已设 `RUST_MIN_STACK=128MB`）——AppContainer 下是否够用未知 | **⚠️ 已测但被 U1 阻断**（依赖下载失败，未进入编译阶段）→ §2.6 |
| **U3** | **build script / proc-macro 的出网需求** | `build.rs` 在沙箱内执行（**安全上更好**），但需要下载的 build script 会被网络策略打断 | 未测（同样被 U1 阻断） |
| **U4** | **cargo 是否真的需要写 `registry`** | 本轮给了 `~/.cargo` 可写，故没测出下限。**若只读够用，策略能显著收紧** | 未测 |
| **U5** | **`--target-dir` 强制全量重建** | 仓库用共享 `build.noindex`；沙箱必须指定独立 target-dir ⇒ **首次全量重建**，且伴随 canonicalize 警告刷屏 | 机制已知，代价未测 |
| **U6** | **`cargo test` 的并发与子进程清理** | R3（`taskkill` 失败）+ R4（扇出异常）的组合面。测试运行器普遍会派生并终止子进程 | 未测 |
| **U7** | **build script 在沙箱内的失败模式** | 需要网络或需要写系统位置的 build script 会以什么错误暴露？大概率又是 `os error 5`（见 R1） | 未测 |
| **U8** | **MSIX 打包的 Python 在沙箱内无解** | `python.exe`（WindowsApps）是 MSIX，**无法在容器内启动**，无 workaround | 已验证是死路，需产品侧确认可接受 |

### 2.5 🔴 U1 实测结论：本地代理链被沙箱切断（新增，2026-09-21）

**这不是"没测"，是"测了且不通"。**

**⚠️ 准确的问题定性：触发条件是"请求要经过本机端口上的代理"，与联网权限无关。**

一开始的猜测是"把联网权限打开就好了"——**实测证明不行**。出网与 loopback 是**两档独立权限**：

| 权限 | 控制字段 | 本机状态 |
|---|---|---|
| 出网 | `network.egress` | ✅ 已开且**有效** |
| 本机端口（loopback） | `network.ingress.hostLoopback`，需 OS 的 **ingress 支持位**（`PSE_SUPPORT_NETWORK_INGRESS`） | ❌ **系统不提供，无法打开** |

**代码级依据：**

```rust
// base_container_runner.rs:446-449
/// Whether BaseContainer can enforce `network.ingress.hostLoopback = "allow"`.
pub fn supports_ingress_host_loopback_allow() -> bool {
    Self::is_process_security_environment_usable()
        && Self::query_psec_ingress_support().unwrap_or(false)      // ← 独立于 egress 的一位
}
```

**现象：** 在 `egress: allow` 下，直接访问外网**可以**，但**凡是被派往 loopback 代理的连接全部超时**。

| 探针（全部在 `egress: allow` 下） | 结果 |
|---|---|
| `gh api /rate_limit`（直连，不经代理） | ✅ **exit 0，0.6 秒**，确实到达 GitHub |
| `cargo fetch`（经代理） | ❌ exit 101，**95.9 秒**后 `[28] Timeout was reached (Failed to connect to 127.0.0.1 port 7890 after 21038 ms)` |
| TCP 直连宿主 `127.0.0.1:7890` | ❌ `timeout` |
| **连沙箱内自己起的 HTTP 服务** | ✅ **成功**（`http_200`）← 关键对照 |
| **宿主局域网地址**（服务监听 `0.0.0.0`） | ❌ **timeout** ← 排除"改 LAN 代理"这条绕法 |
| **外部 IP 裸 TCP**（`140.82.121.4:443`） | ❌ `timeout`（而同一次运行里 `gh` 成功） |

**⚠️ 探针方法的一次校正：** 早期用 `Invoke-WebRequest` 做探针时，PowerShell 5.1 会读取
系统代理设置（本机 `ProxyEnable=1 → 127.0.0.1:7890`），导致**外部对照也失败**——读数被污染。
改用**裸 TCP + `gh`（不经系统代理）**后才得到自洽结果。
**凡是用 `Invoke-WebRequest` 得出的网段可达性结论，应以此表为准。**

**⚠️ 关键对照推翻了"loopback 被整体拦掉"这一解释（2026-09-21 二次校正）。**

同一次运行内，用同一个 `127.0.0.1`，结果相反：

| 探针 | 结果 |
|---|---|
| A. 连沙箱内自己起的服务（`127.0.0.1:18111`） | ✅ `connected` |
| B. 连宿主上的代理（`127.0.0.1:7890`） | ❌ `timeout` |

**⇒ 沙箱并未禁止 loopback，而是拥有自己独立的回环接口。**

| 谁 | `127.0.0.1` 指向 |
|---|---|
| 宿主上的代理软件 | **宿主的**回环接口 |
| 沙箱内运行的进程 | **沙箱自己的**回环接口 |

**⇒ 沙箱能连自己起的服务，但连不到宿主回环上的服务。**
这也是 MXC 的字段名为 `network.ingress.**hostLoopback**`（宿主回环）的原因 ——
**它存在的意义正是打通"沙箱回环 ↔ 宿主回环"这一对。**

（附带解释一个早先的异常：沙箱内 `netstat` 看不到监听行但连接成功，
正因为沙箱看到的是自己的网络栈。）

**根因（三层，逐层收窄）：**

**第一层 · Windows 的回环隔离（Loopback Restriction）**

Windows 对 AppContainer 类沙箱进程**默认禁止访问 `127.0.0.1`**。设计初衷是防逃逸：
若允许沙箱随意访问本机端口，它可能利用本机其他本地服务的漏洞反向攻击宿主。
（这也是"装了本地代理后某些商店应用无法联网"的同一根因，代理软件里常见的
"UWP 回环豁免"小工具就是为此。）

**佐证：** 本机 `CheckNetIsolation.exe LoopbackExempt -s` 的回环豁免列表**原本为空**，
正好解释"代理对普通程序有效、对沙箱进程无效"。

**第二层 · 官方豁免工具对 MXC 不适用**

以管理员身份实测：

```
CheckNetIsolation.exe LoopbackExempt -a -n=<containerId>
→ exit=0「完成」，列表出现 SID: S-1-15-2-4083123861-...-3367644864
→ 但加豁免后沙箱仍连不上宿主（回环 / LAN 均 timeout）
```

原因：`CheckNetIsolation` 面向**打包应用（按包名查表）**，
而 MXC 的 base-container tier 用的是 **PSEC 身份**（`base_container_runner.rs:822`
里 `identity = "<process-security-environment>"`），且 profile **用完即删**
（`sandbox_tracking.rs:186`）—— 文件系统与注册表均查不到，故列表显示 "AppContainer NOT FOUND"。

| | UWP / 商店应用 | MXC 沙箱 |
|---|---|---|
| 身份 | 打包应用（有包名） | 进程安全环境（PSEC） |
| 配置生命周期 | 常驻 | **用完即删** |
| `CheckNetIsolation` | ✅ 可用 | ❌ **不适用** |

**第三层 · MXC 自己的等价开关在本机不可用**

MXC 提供 `network.ingress.hostLoopback`（宿主回环），**即回环豁免的官方等价物**，
但本机系统不提供该能力位：

```
network.ingress.hostLoopback='allow' requires Process Security Environment
contract version 1.1 with ingress support
```

与 `--probe` 的 `baseContainerSupportsIngressHostLoopbackAllow: false` **完全一致**。

**⇒ 结论：回环隔离的两条正统解法（`CheckNetIsolation` / `hostLoopback`）均已实测排除，**
**不存在靠改配置绕过的路径 —— 这是 Windows 的隔离设计使然，不是接线问题。**

**代码级依据：** 该档由 `query_psec_ingress_support()` 独立门控，
与 `egress` 无关 —— `base_container_runner.rs:446-449`：

```rust
pub fn supports_ingress_host_loopback_allow() -> bool {
    Self::is_process_security_environment_usable()
        && Self::query_psec_ingress_support().unwrap_or(false)   // PSE_SUPPORT_NETWORK_INGRESS = 0x0008
}
```

**影响面（需产品侧知悉）：**

| 场景 | 受影响 |
|---|---|
| 用本地代理（Clash / v2ray 等，国内常见）拉依赖 | ❌ **完全不可用** |
| `git fetch/push`（继承 `http.proxy`） | ❌ 同上 |
| `bun install` | ❌ 大概率同上 |
| 直连外网的工具（如 `gh`） | ✅ 可用 |

**⚠️ 定性校正：这不是"我们的开发流程问题"，而是"用户机器配置导致 Agent 运行失败"。**

准确的因果链：

| 环节 | 说明 |
|---|---|
| **用户机器上装了代理软件** | 国内开发者的普遍配置，不是我们的特殊情况 |
| 代理监听在本机端口 | 例如 `127.0.0.1:7890` |
| 工具继承了代理设置 | git `http.proxy`、系统 WinINET 等常见来源 |
| Agent 在沙箱内执行联网命令 | 沙箱禁止访问本机自己的端口 |
| **结果** | **命令失败 → Agent 任务失败** |

**失败形态的三个要点（决定严重性）：**

1. **不是降级，是失败** —— **用户什么都没做错**，只是机器上装了代理
2. **与 §四 的错误误导叠加** —— Agent 看到"拒绝访问"，会以为是文件权限问题，
   转而反复重试或改错方向，而不会报告"网络不通"
3. **用户难以自查** —— 报错里没有"代理"字样

**⇒ 沙箱能否工作部分取决于用户机器配置，而我们无法列举这些配置。**
这类失败在用户侧难以归因，也难以让用户自行解决。

**产品侧可考虑的应对（唯一可行项）：**

- 沙箱启动前**检测本机代理配置**，命中时给出明确提示
- 或**明确适用范围**：需要联网拉依赖的场景不走沙箱
- 即 §五 提到的"**沙箱负责编译与测试，依赖安装留在沙箱外**"形态

**绕法清单 —— 五条里四条已实测排除：**

| # | 绕法 | 状态 |
|---|---|---|
| 1 | 改用**局域网可达**的上游代理（不用 loopback） | ❌ **已实测排除**：沙箱连宿主 LAN 地址同样 `timeout` |
| 2 | 用 `CheckNetIsolation` 给 MXC 加**回环豁免** | ❌ **已实测排除**：该工具面向打包应用，对 MXC 的 PSEC 身份不适用 |
| 3 | 打开 MXC 自己的 `hostLoopback` | ❌ **本机系统不支持**该能力位（PSEC 1.1 未落地） |
| 4 | 把代理链去掉、让工具直连 | ⚠️ 需用户改机器配置，我们控制不了 |
| 5 | 等 MXC/系统支持宿主回环 | ⚠️ 时序不可控 |

**⇒ 前三条都是"看起来可行但实测不通"**：根因是 Windows 的 AppContainer 回环隔离，
**不存在靠改配置绕过的路径**。**因此实际可用的只有产品侧检测 + 提示（表中 4/5 亦不可依赖）。**

**一处未查清（不作为结论）：** 宿主**局域网地址**为何同样被拦，我们没有查出确切原因
（早先"缺 privateNetworkClientServer 能力"的推测不成立，因为回环隔离是独立机制）。
不排除是 MXC 自身的策略限制。

**这是本次验证里最实际的阻断**：它意味着
**"能联网的沙箱"在用户装了本地代理时等于"不能拉依赖"，并且以 Agent 失败的形式暴露。**

### 2.6 ⚠️ U2 实测结论：被 U1 阻断，未进入编译阶段

```
L1 (cargo check --workspace):  exit=101  15.2s
L1_first_error: error: failed to download from
                `https://rsproxy.cn/api/v1/crates/rmcp/2.1.0/download`
L1_last: [7] Could not connect to server (Failed to connect to 127.0.0.1 port 7890 ...)

L2 (cargo build -p Flowy):     exit=101  13.0s   同上
L2_exe: NONE
```

**⇒ 失败原因是 U1（代理不可达），不是构建规模。**
且注意一个细节：**缓存的依赖能过，缺的那个（`rmcp`）过不去** ——
说明断网沙箱只能支撑"依赖已全部预热"的构建。

**⇒ U2（规模上限 / 栈压力）在本机网络环境下无法验证**，
需先解决 U1 或改为"预热全部依赖后离线构建"。
（注：宿主对照基线见验证记录。）

---

## 三、架构层面的风险（接入本身完全未动）

**这才是"接入"的真实工作量 —— 一行接入代码都还没写。**

| # | 风险 / 缺口 | 说明 |
|---|---|---|
| **A1** | **P2 未实跑** | state-aware 会话一次都没跑过（本机 `Containers-DisposableClientVM` = Disabled，启用需重启） |
| **A2** | **接入设计不存在** | `SandboxPolicy` 新变体、`enforce_sandbox` 扩展（现只有 3 个静态分支）、`ProcessSupervisor` 集成、错误通道——全部待设计 |
| **A3** | **审批面会瞬间变紧** | 沙箱一强制，"不在策略内"的动作**真的**被拦。`[approvals]` 默认口径与无人值守超时（D-A / D-B 待决项）从"可选"变成"必须"，否则 `cargo check` 都会撞审批门 |
| **A4** | **上游成熟度** | README 自陈「当前**任何 MXC profile 都不应被视为安全边界**」+「已知存在 SDK 生成策略过于宽松的情况」。版本/schema 兼容矩阵要进 CI |
| **A5** | **策略必须按用途分级** | 已验证：编译/测试一类在**离线**可行；依赖安装一类**必须出网**。混用一份策略会让审批门失效 |

---

## 四、与"拒绝不可归因"的关系（本清单的枢纽）

D-D 已判定：**MXC 的拒绝无法支撑 E 组的 `RetryDecision`**。本轮又添两例：

| # | 实例 | 表现 | 性质 |
|---|---|---|---|
| 1 | 被策略拦下的**文件写入** | 不进 `captureDenials`（10 条 denial 中 `write` 为 0） | 信息**缺失** |
| 2 | **网络**被策略拦下 | `os error 5` + **无任何网络字样**（R1） | 信息**误导** |
| 3 | **冷缓存缺依赖** | 与 2 完全同形 | 与 2 不可区分 |

**⇒ 三类后果（策略拒写 / 策略拒网 / 缺依赖）在子进程侧呈现为同一句话。**
这使 E 组（类型化拒绝）从"完成率优化"升级为**接入前置**：
没有它，agent 会把"网络被禁"读成"文件没权限"并开始瞎改路径。

**机制原因（已验证）：** `captureDenials` 只覆盖 ETW 学习模式能观测到的访问检查
（capability / registry / ui），而**真正的文件与网络策略拦截走另一条路径**
（Tier 1 PSEC 文件规则 + AppContainer capability），两者不共享计数。

---

## 五、结论与建议顺序

> **可行性验证已完成，它证明的是"这条路能走通"。**
> 本清单登记的是**走通之后**的已知风险：**6 项未验证技术风险 + 5 项架构缺口**，
> 外加**两项已测且已定性的阻断**（U1 代理链、U2 被其阻断）。
> 其中 A3（审批面）与 E 组是**接入的硬前置**；U1 决定沙箱能覆盖哪些开发动作。

**本轮新增的两条已定性结论：**

| 结论 | 性质 | 影响 |
|---|---|---|
| **U1：本地代理链被沙箱切断** | 🔴 实际阻断，且平台不支持打开（PSEC 1.1 未落地） | **用户机器装了本地代理时，拉依赖 / `git push` / `bun install` 不可用，并以 Agent 任务失败的形式暴露** |
| **U2：完整构建未进入编译阶段** | ⚠️ 被 U1 阻断（非规模问题） | "最大构建能否扛住"仍未知；宿主基线 82 crate / 4m02s / 0 error |

**⇒ 一个可能的产品形态结论**：在用户存在本地代理配置的场景下，
沙箱的适用范围可能是「**编译与测试在沙箱内，依赖安装在沙箱外**」。

**建议的推进顺序：**

| 序 | 事项 | 依据 |
|---|---|---|
| **1** | **E 组（类型化拒绝）** —— 现有**三个**独立实例证明拒绝不可归因（§四） | 接入前置；且与网络无关，必须做 |
| **2** | **U1：解决代理链**（局域网可达代理，或验证"预热依赖后离线构建"） | 决定沙箱能覆盖哪些开发动作 |
| **3** | **U6 / R3 / R4：子进程与并发**（`cargo test` 场景） | 影响测试能力，而测试是本仓库的主要验证手段 |
| **4** | **U4：cargo 只读 registry 是否够用** | 决定策略能否收紧（当前策略可读整盘） |
| **5** | **U2 复测**（U1 解决后） | 决定"沙箱内能否做发布构建" |
| **6** | 再谈 A2（接入设计）与 A1（P2 实跑） | 前面几项会改变设计输入 |

**明确不建议：**

- ❌ 把本文的"可行性通过"读作"可以接入"
- ❌ 在 U1 解决前承诺"沙箱内能装依赖"
- ❌ 在未定 A3（`[approvals]` 口径 / 无人值守超时）之前接入 —— 会立刻撞审批门
- ❌ 在未做 E 组之前接入 —— 拒绝不可归因会直接伤完成率
- ❌ 把 MXC 当作当前可依赖的安全边界（A4）

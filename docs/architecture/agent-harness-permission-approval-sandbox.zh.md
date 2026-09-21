# Flowy Agent Harness 权限、审批与沙箱（Permission, Approval & Sandbox）

> **最后维护：** 2026-09-21 · 核对基准：工作区 HEAD（`docs/agent-store/README.md` 记为 2026-09-21，迁移编号 059）
> 文档性质：安全模块**分层契约**（现行正文，未发版故非冻结）；现状证据为 2026-09-21 时点快照
> 主审查对象：本仓库（`crates/agent/**`、`crates/shared/nomi-process-runtime`、`crates/backend/**`）。
> 外部项目仅作概念对照，不替代本仓库源码证据。
>
> **证据分级（本文所有引用按此标注）：**
> - **【源】** 本仓库源码或测试，已逐条核实到文件:行
> - **【官】** 一手官方文档，已直接读取
> - **【转】** 第三方整理或社区文档，未独立核实 → 只作方向参照，不作判据
>
> 模块命名对齐行业共识：Anthropic 官方文档导航为 `Permissions and sandboxing`【官】，
> LangChain Deep Agents 把 `Permissions` / `Sandboxes` / `Human-in-the-loop` 分列两处【官】，
> 社区综述以 `Permission, Approval & Sandbox` 命名该模块，定位为 Agent 的"免疫系统"【转】。

---

## 一、结论摘要

Flowy 已经有一个**真实存在的授权链**，而且其中一段（审批）达到生产强度：三路 CAS、`argument_digest`
绑定、`expires_at`、幂等与过期语义、能力位派生自 runtime。工具准入层（工具可见性 / 技能权限 /
调用者能力 / 浏览器风险分级）也已成型且顺序正确——`deny` 优先于 `auto_approve`，
单类别授权不是整体旁路。【源】

但它有**一处结构性裂缝和五处能力缺口**：

1. **裂缝（层 0）**：策略文件对 Agent 的 `Bash` 完全敞开。`write_root` 只约束
   Write / Edit / ApplyPatch 且默认关闭，Windows / Linux 又没有进程沙箱，于是
   `~/.agent-store/config.toml`、`%APPDATA%/nomi/config.toml`、`~/.cargo/config.toml`
   都在敞口里——**这是全链唯一"下层能放宽上层"的通路**：Agent 可以自己把
   `auto_approve` 改成 `true`。【源】

2. **执行面沙箱在主力平台缺席**（沿用 `2026-09-16-agent-harness-architecture-review.zh.md` H2）：
   唯一真实隔离 `MacSeatbelt` 是 Unix-only，Windows 上直接 `CapabilityDenied`。【源】

3. **拒绝未分型**：`ProcessError::CapabilityDenied` 有 `path` / `reason` 与稳定分类码
   `capability_denied`，但**没有 `hint`**，也不区分 `Denied`（可换方案重试）与
   `Abort`（停下等人）。Browser 侧已有更好的形状可对照。【源】

4. **判官层不存在**：两个 LLM judge 都判完成度，没有一个判"该不该问人"；
   且缺少"判官只能变严、不能放行"的类型级约束。

5. **闭环层是真空**：批准历史不回流成策略，于是同一操作第 50 次仍要问人。

**贯穿全链的一条硬约束**（本文的规范核心）：

> **下层不能放宽上层。** 策略 → 规则 → 沙箱 → 拒绝 → 判官 → 审批，逐层只能收紧或转呈；
> 唯一有权放宽的是人，且必须是**有作用域的**放宽（限定对象、限定参数、限定时间）。

Flowy 目前有**三处**独立实现了这条约束（工具权限、审批作用域、浏览器红线），
但**层 0 是例外**——它是唯一的反例。

**代价已被行业量化**：默认模式下用户会批准 **93%** 的权限弹窗【转】；
而 OS 级沙箱能安全地减少 **84%** 的权限提示【转】。即——**沙箱不是拿完成率换安全，
是把"逐条审批"换成"预定义安全域"**。这是接 MXC 的正当性所在，不是"更安全所以要更慢"。

---

## 二、模块的行业共识形状

社区综述给出的四层防御模型【转】，与 Flowy 的现状逐层可对照：

| 行业层 | 它回答的问题 | 行业实现要点 | Flowy 对应 |
|---|---|---|---|
| 第 1 层 Tool 级权限 | 工具是否可见？参数是否合法？ | 装配期名单、schema 校验 | 层 1（**已成**） |
| 第 2 层 规则级权限 | 是否命中 allow / deny / ask？ | 内容级匹配 `Bash(git commit:*)`，非仅工具名 | 层 0（**缺写保护**）+ 层 3 |
| 第 3 层 交互式审批 | `ask` → 谁来决定？ | 多路来源竞争：UI / Hook / Classifier / Guardian / 父 Agent | 层 4（**缺**）+ 层 5（**已成**） |
| 第 4 层 OS 级 Sandbox | 进程实际能碰哪些文件与网络？ | 文件与网络**独立**管理；沙箱优先执行 | 层 2（**Windows/Linux 缺**） |

社区模型的两个要点值得照抄：

- **sandbox 优先执行**：每次工具调用先在最小权限环境尝试，大量动作不需要先问；
  只有 sandbox 真正挡住，才走审批升级。【转】
- **文件与网络必须分开管**：没有网络隔离，被攻破的 Agent 能外泄 SSH key；
  没有文件隔离，它能逃逸沙箱重获网络。两者缺一不可。【转】

Flowy 的现状是**第 4 层缺失导致第 3 层承担全部压力**——这正是 93% 那个数字的成因。

本文的层号（0–8）是社区四层模型的**细分**，不是替代。多出的两层是
**层 0（策略定义与签名）** 与 **层 8（批准 → 策略升级）**：社区模型把前者当作隐含前提
（规则来源枚举），把后者作为收尾原则（"执行后把经验沉淀回规则层"）【转】，但都没有独立成契约。

---

## 三、Flowy 现状：逐层映射

### 3.0 全链一处裂缝（先读这条）

层 0 与层 2 之间没有约束，导致**层 2 的能力可以改写层 0 的规则**：

```
Agent 的 Bash（不受 write_root 约束、Windows/Linux 无进程沙箱）
  → 改写 ~/.agent-store/config.toml 或 %APPDATA%/nomi/config.toml
  → auto_approve = true
  → 层 1 的 deny 之外的准入全部自动放行
```

`nomi-tools/src/path_guard.rs:11-13` 自己的注释就是这条裂缝的准确描述：

> This stops *accidental or buggy* out-of-workspace writes … It is **not** a security sandbox
> against a determined agent: **the same agent has `Bash`** …

即：**API 面有写入白名单，进程面没有。** 这是本文 §五 的第一优先项。

---

### 层 0 · 策略定义与签名 —— 谁能写策略

**已实现：**

| 机制 | 位置 | 说明 |
|---|---|---|
| 写入白名单 | `nomifun-app-server/src/agent_store.rs:1112` | 文件头注明 "The **write whitelist** for `~/.agent-store/config.toml` (`config/set`)" |
| 未知键即拒 | 同上 + `nomifun-api-types` | `AgentStoreConfigPatch` 是 `#[serde(deny_unknown_fields)]`；当前只允许 `default_model`；`api_key` / `base_url` / 路径字段一律 `invalid_request`（**不是静默忽略**） |
| 路径由宿主决定 | `lib.rs:1129-1136`、`:4393-4394` | "no request can name a path"；测试 `lib.rs:10953` 用 `{"path": "C:/elsewhere/config.toml"}` 打这个断言 |
| 最小改动写 | `lib.rs:4764` | `toml_edit` 只重写目标键、注释与其余键逐字节保留；同目录临时文件 + `rename` 原子替换 |
| 读口径不伪装 | `docs/agent-store/16-…zh.md` R16 | 文件缺失 = `exists:false` + 显式 `null`；读不动 = `config_unavailable`——**不拿默认值把"文件坏了"伪装成"没写"** |
| 策略类型化形式 | `nomifun-api-types/src/tool_policy.rs:1` | "Host-owned Nomi tool policy — the typed form of `~/.agent-store/config.toml`" |

**策略来源（多处叠加）：**

- `~/.agent-store/config.toml` —— 宿主策略（provider / marketplace / memory / import）
- `%APPDATA%/nomi/config.toml` + 项目级 —— 工具策略
- `nomi-config/src/config.rs:374-419` 的 `ToolsConfig`：`auto_approve`（默认 `false`）、
  `allow_list`（`default_allow_list()`）、`skills`（deny / allow）、`write_root`（默认空）、
  `browser` / `computer` / `web`、seatbelt 开关

**缺失：**

- ❌ **策略文件无进程级写保护**（§3.0）。行业共识是"**可工作，但不能影响后续执行权限**"：
  Agent 能改项目源码，但碰不到 `.git/hooks`、settings、skills 目录、`~/.ssh`、`~/.aws`
  等**控制面路径**；最终可动范围收敛为「当前项目源码 + 临时目录」【转】。
  Flowy 的 `write_root` 只有**正向白名单**，没有控制面**反向保护**
- ❌ **策略变更无事件**。`config/set` 做到"响应 = 写后重读"（落点即磁盘内容，不做乐观回显），
  但没有"谁在何时把 `auto_approve` 从 false 改成 true"的留痕
- ⚠️ **`[approvals]` 段不存在**。`docs/agent-store/21-open-decisions.zh.md` D3=B 已拍板
  「策略由 `~/.agent-store/config.toml [approvals]` 声明」，但**尚未实装**；
  实装的是 `[tools].auto_approve` / `[tools].allow_list` / `[tools].skills`。
  读文档的人容易误以为 `[approvals]` 已在用

---

### 层 1 · 工具准入 —— 工具 / schema / 调用者权限

**这层最完整，基本不需改动。**

**① 装配期名单（三层求交）** — `nomi-config/src/config.rs`

```rust
pub enabled_tools: Option<Vec<String>>,          // :98  服务器级工具白名单
/// Per-server tool blocklist, applied **after** `enabled_tools`.   // :99
```

叠加顺序见 `nomifun-ai-agent/src/manager/nomi/agent.rs:923-960`：
`Config::resolve` 读 `%APPDATA%\nomi\config.toml` × 会话白名单（工厂算好的受限角色名单）× 宿主 `[tools].enabled`。

**② 技能级权限（五步链，顺序本身是安全属性）** — `nomi-skills/src/permissions.rs:45-108`

```
1. deny 规则        → Deny   （always enforced, even when auto_approve = true）
2. allow 规则       → Allow
3. safe-properties  → Allow  （无 hooks && 无 allowed_tools && 无内嵌 shell）
4. auto_approve     → Allow  （Ask → Allow，但不越过 Deny）
5. 兜底             → Ask { reason }
```

第 3 步的判定（`:87-95`）已经识别出**技能正文里的内嵌 shell 等于 hook 级特权**
（`has_shell_body`），并区分了 `LoadedFrom::Mcp`（运行时从不执行内嵌 shell，故豁免）。
断言见 `integration_tests.rs:787`（"Deny should not be overridden by auto_approve"）
与 `:770`（auto_approve 转换 Ask → Allow）。

> 注：`2026-09-16-…review.zh.md` H3 曾把此处记为漏洞，现已由 `has_shell_body` 收口。

**③ 调用者能力** — `CapabilityPolicy`（`nomi-process-runtime/src/capability.rs`）

```rust
pub struct CapabilityPolicy {
    pub cwd_roots: Vec<PathBuf>,
    pub sandbox: SandboxPolicy,
}
```

子代理继承路径**逐层收窄**（`nomi-agent/src/local_agent_invocation.rs`）：

- `:429` 源码工作区必须在继承的 capability roots 内
- `:441` `child.process_capability.cwd_roots = vec![cwd.clone()]`
- `:962` `&mut capability.sandbox`（逐子引擎收窄 sandbox）

`agent-harness-modes-review.zh.md:709` 记录其意图：「逐子引擎独立 `ProcessSupervisor`，
并把 capability/Seatbelt 收窄到子 cwd；**继承的拒绝不可降级**」。

**④ 风险分级（浏览器专属，但形状最好）** — `nomi-browser/src/redline.rs`

- `ApprovalTier`：`Info / Edit / Exec / Irreversible`（`:41-51`）
- `classify_action(action, ctx) -> ApprovalTier`（`:189`）**纯函数**，
  运行时危险信号全部走 `ActionContext` 入参（`:71-90`：元素 accname/role、是否 submit 控件、
  跨域 POST、Enter 落 form、POST 页 reload）
- 中英不可逆词表 + 误命中消解（`:98-168`，处理 `display` / `replay` 含 `pay` 子串）

**缺失：**

- ❌ **`deniedPaths` 类反向名单**。只有"正向白名单"一种形态，表达不了
  "整盘可读但屏蔽 `~/.ssh`"
- ⚠️ **能力位诚实性**已有先例（`docs/agent-store/24-…zh.md:53`「不广告必然失败的方法」），
  需推广到策略面：策略未覆盖的路径不应被 advertise 成可用

---

### 层 2 · 内核沙箱 —— 文件系统 / 网络 / UI / 令牌 / 环境

**现状：**

```rust
// nomi-process-runtime/src/capability.rs:4-8
pub enum SandboxPolicy {
    UnrestrictedLocalOwner,                        // 默认，零隔离
    MacSeatbelt { write_roots: Vec<PathBuf> },     // 唯一真实实现
    DenySpawn,
}
```

`MacSeatbelt` 的实现质量很高（`platform/unix.rs:3119-3192` 的 `seatbelt_profile`）：

- 逐个 canonicalize write root、验证是目录、**验证在 normalized capability roots 之内**
- 拒绝含控制字符的路径字面量（防 profile 注入）
- 专门的信任处理：`trusted_macos_user_temp`，**Seatbelt 下必须保留可信 `TMPDIR`，
  不接受用户覆盖**（测试 `:4275`）

**Windows 是空的**（`platform/windows.rs:2877-2884` 两条 `CapabilityDenied`，
一条给 `DenySpawn`，一条给 `MacSeatbelt`："macOS Seatbelt cannot authorize Windows process"）。

**MXC 接入要点（Microsoft eXecution Container，MIT，2026-06 Build 开源）【官】：**

MXC 是**进程级**的，与既有 `ProcessSupervisor` 同构，因此接入成本低：

- 形态：一个原生二进制（`wxc-exec.exe` / `lxc-exec` / `mxc-exec-mac`）+ 版本化 JSON 配置
  + TypeScript SDK `@microsoft/mxc-sdk`
- 调用：`wxc-exec.exe config.json`（或 `--config-base64`），
  尾参 `-- <cmd>` 可覆盖 `process.commandLine`
- 默认后端跨平台原生：Windows `processcontainer`（AppContainer / BaseContainer）、
  Linux `bubblewrap`、macOS `seatbelt`

**策略面是四类（不止"路径 + 出网"）【官】：**

| 类 | 字段 | 默认 |
|---|---|---|
| 文件系统 | `readwritePaths` / `readonlyPaths` / `deniedPaths` / `clearPolicyOnExit` | **省略即拒绝** |
| 网络 | `egress.default` + CIDR/端口级 `allow`/`deny`；`ingress.default` + `hostLoopback` | 全部 `deny` |
| UI | `allowWindows` / `clipboard: none\|read\|write\|all` / `allowInputInjection` | 全 `false` / `none` |
| 执行 | `timeoutMs` | 无超时 |

**另有三类不在 JSON 里但创建进程时就被改掉：** 令牌身份（AppContainer 包 SID，
`least_privilege_mode` 进一步降到 `ALL RESTRICTED APPLICATION PACKAGES`）、
权限集（`capabilities` 列表）、**进程环境**（`CreateEnvironmentBlock(..., bInherit = FALSE)`，
父进程环境变量一个都不泄漏）。

**接入必须同时处理的四件事（否则是净负）：**

1. **UI 策略默认全关 = 能力归零。** `allowWindows:false` + `allowInputInjection:false`
   + `clipboard:"none"` 会让 `nomi-computer` / `nomi-browser` / `nomi-a11y`
   （含 Windows actor：`crates/agent/nomi-a11y/src/windows/`）**结构性失效**。
   Win32k lockdown 在内核态、子进程执行任何用户态代码之前生效，无竞态窗口——**确定性失败，不是概率失败**
2. **`hostLoopback` 默认 `deny`**，会撞死"起本地服务再连它"的模式（dev server / LSP / 本地代理）
3. **`deniedPaths` 在 Windows 上未实现**【官】（README 明载）。即 Windows 上**表达不了**
   "整盘可读但屏蔽 `~/.ssh`"——只能做白名单
4. **跨后端不可移植**：源码有硬规则「backends that cannot enforce a requested ingress
   combination **reject it rather than weakening the policy**」【官】——fail-closed 是对的，
   但策略必须按后端生成

**MXC 的四个实现级风险【官/转】：**

| 风险 | 后果 |
|---|---|
| **Tier 2（BFS broker）默认不编译**（Cargo feature `tier2_bfs`；`find_bfscfg_exe` 碰盘前即 `return None`） | 探测逻辑从 Tier 1 直落 Tier 3。Tier 2 是**唯一不碰宿主 security descriptor** 的降级目标，却是最可能拿不到的那层 |
| **Tier 3 直接改宿主 NTFS ACL** | `SetNamedSecurityInfoW` 写真实文件对象 → 必须靠 `DaclManager` 反向重放 + per-ACE 状态文件 + 启动时清扫孤儿。强杀/掉电会残留**不可见的 ACE**（AppContainer SID 按名字稳定派生，长期有效） |
| **`windows_sandbox` 拆卸用 `taskkill /F /IM`，且 VM 跨次复用** | 按映像名杀进程，**作用域是整机**；VM 复用使上一脚本的副作用（文件/注册表/包/后台进程）留给下一脚本——把不可信负载放进同一信任域 |
| **`isolation_session` 靠用户态文本过滤兜底**（`protected_paths_filter.rs`，MXC issue #330） | OS 的 `ShareFolderBatchAsync` **带子树继承**，共享 `C:\Users\alice` 会连 `.ssh` 与浏览器凭据一起授出。兜底是纯字符串 deny-set，源码自列绕过：8.3 短名、symlink/junction（"no `canonicalize` disk access"）、UNC、`CommonProgramFiles`。注释自陈"The proper fix belongs in the OS API" |

**README 的定性必须原样带过来：**

> 这是早期预览……**已知存在 SDK 生成策略过于宽松的情况**……当前
> **任何 MXC profile 都不应被视为安全边界**。【官】

**接入前置条件（顺序不可换）：** 先用 `--audit`（宽松学习模式，注入 `permissiveLearningMode`）
在**隔离的开发机 / CI** 上跑真实任务，收集 `denials.json` / `denials.verbose.json` / ETL，
生成 `Adjusted_*.json` 反哺策略。**`--audit` 期间 AppContainer 限制不被强制，绝不可在生产机跑。**【官】

**启动时机：** 层 2 必须在 §层 7 的"平台残留物清理"与 §层 3 的"拒绝分型"**同时**上线，
否则要么在宿主 ACL 上留孤儿，要么让拒绝信息不可读。

---

### 层 3 · 类型化拒绝 —— 喂回 Agent 自愈循环

**现状（比预期好：契约已存在且带稳定分类码）：**

```rust
// nomi-process-runtime/src/request.rs
:115  #[error("process capability denied for {path:?}: {reason}")]
:116  CapabilityDenied { path: PathBuf, reason: String },
:152  Self::CapabilityDenied { .. } => "capability_denied",   // 稳定分类码
```

调用侧已在消费这个形状：`local_agent_invocation.rs:156` 包成
`"Delegated Agent capability denied: {error}"`；`:1602` 测试断言
`result.content.contains("capability_denied")`。

**cwd_roots 越界、Seatbelt root 非法、TMPDIR 覆盖、含控制字符的路径——全部走同一个变体，
分类码一致。** 这是好设计。

**浏览器侧有更强的先例可对照：**

| 形状 | 位置 | 价值 |
|---|---|---|
| `Blocked { reason }` | `nomi-browser-engine` | 不可逆拦截、工作区逃逸（`actions.rs:2823-2869` 的 canonicalize 检查）、遮挡误点 |
| `Unsupported { capability, hint }` | `engine.rs:184-185` | **默认 OFF 的能力，明确带 `hint`**。`evaluate.rs:79-80` 决策表 + `:112` 的 `evaluate_off_error()` 注释写明「讲清为何 off + 怎么开」 |
| `RetryDecision::{Retryable, Fatal}` | `actions.rs:867-891` | **重试语义显式区分**，`classify_browser_err` / `classify_editable_check_err` 做映射 |

**缺失：**

- ❌ **`hint` 字段**。`CapabilityDenied` 有 `reason` 但没有结构化的"替代方案"。
  对比 `BrowserError::Unsupported { capability, hint }`——进程侧缺 `hint`
- ❌ **`RetryDecision` 等价物**。MXC 的拒绝恰好分两类：**策略拒绝**（换个路径就能过）
  vs **后端不支持**（怎么试都不行）。这两类必须映射到不同的重试决策，
  否则 Agent 会对注定失败的操作反复重试
- ❌ **`Denied` 与 `Abort` 未区分**。行业共识：生产系统必须区分
  `Denied`（拒绝，可重试其他方案）与 `Abort`（中止，等新指令），
  "否则一旦 UI 关闭或连接中断，就会被误当成『安全拒绝』"【转】。
  **这与本仓库既有旧账是同一类错误的两个方向**：
  `agent-harness-modes-review.zh.md:68` 记着「`approval_manager` 退化成 stdin 交互，
  **EOF 即视作批准**」——EOF 是"连接断了"，它既不是批准，也不是拒绝

---

### 层 4 · 判官分诊 —— ask 之后谁来决定

**现状：两个 LLM judge，都不判安全。**

| judge | 位置 | 判什么 | 关键参数 |
|---|---|---|---|
| 目标完成度 | `nomi-agent/src/goal/judge.rs:405` `judge_goal()` | 三值裁决 done / continue / wait | `JUDGE_MAX_TOKENS = 4096`、`JUDGE_TIMEOUT_SECS = 30`、输入全部**截断**（2000/2500/4000 字符，`:35-37`）、有契约时 `contract` 权威（`:183`）、`parse_failed` 是**独立返回位**（`:492`） |
| 浏览器动作 | `nomi-browser/src/redline.rs` + `nomifun-ai-agent/…/browser_approval.rs` | **规则判定，不是 LLM** | `ActionContext` 结构化信号 |

**已有正确的 fail-closed 模式**：`nomifun-agent-execution/src/control_steps.rs:456`
`judge_fails_closed_when_any_declared_ballot_is_invalid()`——多重投票任一票非法即 fail-closed。

**行业参照【转】：**

- **Codex 的 Guardian**：把审批的审查者从"用户"换成子 Agent，但不是简单点"允许"，
  而是**重建压缩上下文、在锁死环境（只读 sandbox、自身不允许再申请审批、关闭非必要能力）
  中做风险评估**；关键安全属性是 **fail-closed：超时、解析失败、review session 执行失败
  一律按高风险拒绝**
- **Claude Code 的两阶段 Classifier**：Stage 1 单 token 快速过滤，Stage 2 只对 flag 的动作
  做 CoT 推理，误判率 **8.5% → 0.4%**；
  且**刻意不看 assistant 思考与工具执行结果**——"既防 agent 说服 classifier，
  也防 prompt injection payload 通过 tool output 传播"
- **先确定性后模型化**：便宜的确定性快路径消化绝大多数动作，只把剩余高风险动作交给模型
- **熔断**：连续 deny 超阈值退回人工，headless 模式下超限直接 abort

**缺失（这一层要新建）：**

- ❌ **"只准变严"的类型级约束**。分诊判官的输出空间必须是
  `enum Triage { Reject(reason), Escalate(card) }`——**类型系统层面就没有"放行"这个值**，
  而不是靠提示词要求它不要放行
- ❌ **降级目标是 `Escalate`，不是 `Reject`**。分诊场景的 `parse_failed` / timeout
  应退化为"交给人看"（保守但可用），**永远不是"拒绝"**，
  更**永远不是"放行"**。**唯一不可原谅的失败模式是解析不出来时说 yes**
- ❌ **熔断**。介于层 3 与层 5 之间，防止 Agent 在拒绝循环里打转（同时是完成率保护）
- ❌ **判官不得窥探生成器上下文**。若为求准而读文件内容，就把不可信数据读进了决策上下文；
  若不读，就活在"文本 ≠ 行为"的无知里。**正解是双管：分诊判官既不看 assistant 思考，
  也不把 tool output 纳入判据**

**为什么判官不能当授权器（四条，逐条可复核）：**

1. **它判文本，不判行为。** 危险分布在运行时展开里
   （`subprocess.run(['rm','-rf', open('/tmp/p').read().strip()])` 的危险取决于 `/tmp/p` 的内容，
   文本里没有）
2. **命令文本是攻击者控制的数据。** prompt injection 的载荷是"看起来完全正常的字符串"
3. **可能与生成器共享盲点**（同族模型自评）。`judge_goal` 在**完成度**上能接受（错了好修），
   在**安全**上不能接受（错了不可逆）
4. **它产出"看起来像经过审查"的审计记录。** 带理由的批准进了日志后，
   在审计面上与"人工批准过"不可区分——**消灭了"这里本该有人看一眼"的信号**

---

### 层 5 · 人工审批 —— 唯一有权放宽者

**现状（全链最成熟的一段）：**

协议层（`docs/agent-store/10-public-contracts.md:144-156`、`05` §7）已定完：

```
approval.required = 事实事件
approval/request  = Server Request
approval/respond  = 客户端 Command
三者共享 request_id、approval_id、run_id、step_id、attempt_id
请求还必须绑定 tool_call_id、argument_digest、expires_at
重复响应幂等，过期响应返回 approval_expired
```

服务端职责写得很硬（`05:1421`）：

> 服务端必须在 Runtime 层**再次校验**审批对应的资源、工具、参数摘要和过期时间；
> **不能只相信客户端提供的 approval_id**

实现侧：

- `run/answer-decision`（WS + HTTP `POST /api/app-server/run/{run_id}/answer-decision`），
  params = `run_id` + `step_id` + `attempt_id` + `answer` + **三个 CAS 版本**，
  两处结构均 `deny_unknown_fields`
- CAS 语义：「读到之后任何并发移动 → `Conflict`，**绝不静默覆盖**」
- 能力位**派生自 runtime**（`lib.rs:541` `approvals: availability.runtime`）——
  「无 runtime 的连接不宣告一个只会回 `runtime_unavailable` 的能力」
- WebUI：`web/src/components/ApprovalCard.tsx`；
  `web/src/lib/approvals.ts` 的 `pendingApproval` / `pendingDecision` 从事件流纯投影
- **无 approve-all 是有意为之**：`16` R8 记录明确 `AnswerDecisionInput` +
  `RunEvent` 投影字段「含三个 CAS、**无 `always_allow`**」

**三个作用域分级** — `nomi-protocol`：

```rust
// commands.rs:70
SessionMode::Yolo      → 全类别 auto-approve（唯一 wholesale bypass）
SessionMode::AutoEdit  → 只 info + edit
SessionMode::Default   → 无

// commands.rs:62
ApprovalScope::Always  → 单类别 scoped grant（add_auto_approve(category)）
```

**必须继承的既有区分**（`nomi-protocol/src/lib.rs:156-182`，
这是全仓最接近"层级不能互相放宽"的实现）：

> Per-category user "always" approvals are **intentionally NOT consulted**:
> a manual `add_auto_approve("exec")` is a **scoped grant, not a wholesale approval bypass**,
> so it must not arm the facade redline gate against irreversible actions.

即精确区分了 **scoped grant**（单类别，仍受红线约束）与 **wholesale bypass**（yolo，
触发额外的独立门）。`session_bypasses_approval()`（`:177-182`）是其唯一判定点。

**浏览器红线：整个仓库最完整的"独立门"实现** — `nomi-browser/src/redline.rs:5-28`

模块头列出三条**旁路** approval pipeline 的路径（`auto_approve`、`SessionMode::Yolo`、
companion 强制 yolo），并说明为何必须有一道**不经 approval pipeline** 的强制门
`enforce_redline`：yolo 下一切自动批准，若 IRREVERSIBLE 动作只靠普通审批闸，
会**静默自动执行**——这是红线事故。故 yolo/companion 会话下 IRREVERSIBLE 一律
**hard-deny `BrowserError::Blocked`**。

**缺失：**

- ❌ **`[approvals]` 段未实装**（见层 0）。`21-open-decisions.zh.md` D3 已拍板方向，
  D3 的配套问题（`:82`）仍在问「默认策略是全部放行还是只放行只读」
- ❌ **无"临时、限定对象"的完整闭环**。`argument_digest` 与三 CAS 解决了**绑定**，
  但缺**时限的作用域**（"这一条路径，这一次，10 分钟"）
- ❌ **无人值守下的行为未定义**。`docs/guides/terminal.md:131`：
  「a turn that hits an interactive approval prompt will block until it times out」。
  AutoWork / Full Auto 打到审批门就是**挂到超时**——这是必须显式拍板的取舍，
  不能让它以超时形式默认发生

---

### 层 6 · 审计与溯源

**已有：**

- `argument_digest` + `tool_call_id` + 三个 CAS 版本 → **动作与其审批的绑定可验证**
- `approval.required` 是**可重放事实事件**；`run/events` 与 WS 实时推送**共用同一投影**（`16` R8 ③）
- **快照与事件互补**，这个区分已写进文档：「事件给『发生过什么』，快照给『现在是什么』」（`16` R10）
- R14 已把逐轮 token 持久化（迁移 `058` → `app_server_context_usage.last_turn_input_tokens` / `last_turn_output_tokens`）

**缺失：**

- ❌ **策略变更审计**。`config/set` 做到"写后重读"，但没有"谁在何时改了什么"的事件——
  这是层 0 的直接依赖
- ❌ **三层判定的原因必须可区分**。判官层加入后要有第三种信号，
  且**不能混进 approval 的审计里**（否则判官就"洗成"了人类批准）
- ❌ **结构化拒绝归档无落点**。MXC 的 `MXC.VerboseDenials`（sanitized provider/event id、
  closed outcome reason、occurrence count）是学习素材，链路里没有对应位置

---

### 层 7 · 回滚与爆炸半径

**已有：**

- **技能回收站**：dsh-skill-hub 插件实现的「上游删除 → 移入回收站可恢复，
  恢复后保留来源与场景归属」——**这正是本层要的形态**：破坏性操作产出可恢复的中间态
- **git worktree 隔离**：`nomi-tools/src/worktree.rs:147`「This is the authority hand-off
  used by child-agent capability setup」——子代理跑在独立 worktree 里，爆炸半径被 git 边界限制
- **原子写**：`config.toml` 的临时文件 + `rename`（`lib.rs:4764`）；
  测试断言 `config.toml.tmp` 不残留（`:10732` 等多处）

**对照：MXC Tier 3 是这一层的反面教材。** 它必须落 per-ACE 状态文件、启动时清扫孤儿 ACE、
把清理接到 panic unwinding 与 Ctrl-C handler 上——**因为它的中间态在宿主安全描述符上，
不在可回滚的原地**。Flowy 现有的原子写模式不需要这些，正是因为它把中间态留在了原地。

**缺失：**

- ❌ **通用的"可撤销动作"契约**。技能回收站是**点状实现**，未抽成
  "任何破坏性工具都应产出可恢复中间态"的协议级约定
- ❌ **平台侧残留物清理**。若接 MXC，Tier 3 会在宿主 ACL 留孤儿 ACE。
  链路里没有"平台残留物"的清理位置——**必须在层 2 之前补上**
- ❌ **审批授予的撤销**。`add_auto_approve(category)` 是**进程内 `HashSet`**，
  无持久化、无到期。`ApprovalScope::Always` 的 scope 语义在**协议层**（`expires_at`）
  与 **agent 引擎层**（内存 HashSet）之间**对不上**

---

### 层 8 · 批准 → 策略升级（闭环）

**现状：真空。** 全仓没有"审批历史回流成策略"的机制。三个相关碎片都不构成闭环：

1. `ApprovalScope::Always`（`nomi-protocol/src/lib.rs:100-113`）**是运行时、进程内的**：
   `approve(call_id, Always)` → `auto_approved.lock().insert(category)`。进程重启即丢
2. `session_mode`（Default / AutoEdit / Yolo）是**用户显式切的**，不是从历史推出来的
3. `ToolApprovalManager` 明确**拒绝**把 per-category grant 当 wholesale bypass 用——正确，
   但也意味着现状只有"越批越松"或"越批越紧"两种手动手势

**为什么这层必须有：** 它是**漏斗可持续的唯一理由**。缺了它，同一操作第 50 次被批准时
系统不知道它已是常规操作，人反复被问同一件事，而每次人工批准作为"策略不完整"的证据
无处回流。最终结果就是 93% 那个数字——**审批门被人关掉**。

**闭环路径（产物是层 0 的输入，不是层 4 的输入）：**

```
审批历史（approval.required / approval.respond 事件流，已可重放）
  → 按 (tool, argument_digest 的规范化形状) 聚合
  → 超阈值的重复项 → 生成策略条目候选
  → 走 config/set 白名单校验（deny_unknown_fields + 未知键即拒）
  → 写入 config.toml + 产生策略变更事件（层 6）
  → 该操作此后落在层 1 / 层 2 内自动放行，判官与人都不再介入
```

**三条不可让步的约束：**

1. **只能升级到"策略"，不能升级到"绕过判官"。** 闭环的产物是层 0 / 层 1 的输入，
   **不是层 4 的输入**——判官不应有一条"这些我都批过了"的后门
2. **自动生成的策略条目必须走同一套白名单校验。** 未知键即拒这条防线不能因为
   "是系统生成的"就跳过
3. **策略自动变宽是最需要留痕的操作**（层 6 + 层 7），因为它把一次性例外变成永久能力

---

## 四、行业↔本仓词汇对照

| 本文用语 | 行业用语 | 本仓库现状词汇 |
|---|---|---|
| 授权漏斗 / 权限链 | permission pipeline | `权限链`（`2026-09-16-…review.zh.md` H3/M5/§7） |
| 三层裁决 | allow / deny / ask | `SkillPermission::{Allow, Deny, Ask}`（`nomifun-skills/src/permissions.rs:36-43`） |
| 审批等级 | approval tier | `ApprovalTier::{Info, Edit, Exec, Irreversible}` |
| 会话模式预设 | permission mode | `SessionMode::{Default, AutoEdit, Yolo}` |
| 作用域授权 | scoped grant | `ApprovalScope::Always` + `add_auto_approve` |
| 独立强制门 | non-bypassable gate | `enforce_redline`（`nomi-browser/src/redline.rs:261`） |
| 判官分诊 | classifier / Guardian | **无**（`goal/judge.rs` 判完成度，非安全） |
| 控制面路径 | control plane paths | **无**（`write_root` 只有正向白名单） |
| 熔断 | circuit breaker | **无**（相邻者：`loop_guard` / `StagnationGuard`） |

---

## 五、实施顺序与验收条目

顺序依据：**先封裂缝，再补可读性，最后接沙箱**——因为沙箱的失败模式在信息不清晰时会误导 Agent。

| 序 | 层 | 工作 | 验收条目 |
|---|---|---|---|
| 1 | **0** | 策略文件进程级写保护 + 控制面路径反向名单；策略变更事件 | Agent 无法通过任何工具改写 `~/.agent-store/config.toml` / `%APPDATA%/nomi/config.toml` / `~/.cargo/config.toml`；变更产生可重放事件 |
| 2 | **3** | `CapabilityDenied` 补 `hint`；加 `RetryDecision`；区分 `Denied` / `Abort` | 策略拒绝 → `Retryable(替代方案)`；后端不支持 → `Fatal`；连接中断 → `Abort`（**不得**映射为批准或拒绝） |
| 3 | **7** | 平台侧残留物清理契约（MXC Tier 3 的 ACE 回滚/孤儿清扫） | 强杀进程后宿主 ACL 无残留；启动时自检 |
| 4 | **2** | MXC 接入（新 `SandboxPolicy` 变体）+ `--audit` 策略学习 | Windows 上 Bash 具备内核级文件/网络约束；后端不可用**回落**而非静默放宽；UI 策略显式声明后 `nomi-computer` / `nomi-browser` 仍可用 |
| 5 | **4** | 判官分诊（输出空间二元、降级到 `Escalate`、不窥探生成器上下文）+ 熔断 | 分诊判官**无法**产出放行；`parse_failed` / timeout → `Escalate`；连续 deny 达阈值 → 升级人工 |
| 6 | **8** | 批准 → 策略升级闭环 | 重复批准超阈值产出策略候选且**经白名单校验**；判官无后门 |
| 并行 | **6** | 策略变更事件（与 1 同批） | 三层判定原因在审计面上**可区分** |

### 5.1 MXC 接入的工作分解（6 组）

上面的序 4（层 2）不是"加一个 `SandboxPolicy` 变体"，实际是 6 组工作。

**⚠️ D 组存在已定位的硬阻断点**，详见
[`agent-harness-mxc-process-wrapper-feasibility.zh.md`](agent-harness-mxc-process-wrapper-feasibility.zh.md)。
**D 组的可行性结论必须先出，否则 C / F 的投入可能全部作废。**

#### A · 前置决策（3 条，纯拍板）

| # | 要定什么 | 为什么是前置 |
|---|---|---|
| A1 | **`[approvals]` 默认口径**（§六 D-B） | 接上沙箱后 `cargo check` / `bun install` / `git fetch` / `gh pr create` **全部变成越界动作**。没有策略 = 每个构建都弹审批，比现在更糟，且会直接把沙箱关掉 |
| A2 | **无人值守撞审批门的行为**（§六 D-A） | `docs/guides/terminal.md:131`「block until it times out」。有沙箱后审批成为**唯一**越界通路，挂到超时 = AutoWork 彻底不能构建和测试 |
| A3 | **MXC 定位：实验性后端还是目标后端**（§六 D-E） | 决定 C/D/F 的投入深度。README 自陈「任何 MXC profile 都不应被视为安全边界」【官】 |

#### B · 策略学习（零依赖，可与 A 并行开工）

| # | 工作 | 产出 |
|---|---|---|
| B1 | 探测可用性：`wxc-exec.exe --version` / `--probe` | 后端可用矩阵（含 OS 版本，`processcontainer` 最低 Win11 24H2 / 26100） |
| B2 | 写基线 `policy.json`（按 0.8 方向性 schema） | 起始策略 |
| B3 | 隔离开发机 / CI 上跑 `--audit`：`cargo check` / `bun install` / `git fetch` / `gh pr create` | `denials.json` + `denials.verbose.json` + ETL |
| B4 | 从 denials 归纳**工作区外路径清单** | 层 0 反向名单 + 层 2 白名单的**共同输入** |

#### C · 策略作者层

| # | 工作 | 落点 |
|---|---|---|
| C1 | `SandboxPolicy` 新增变体，字段对应 MXC 四类（文件系统 / 网络 / UI / 超时） | `nomi-process-runtime/src/capability.rs:4-8` |
| C2 | `config.toml` → `CapabilityPolicy` 映射：新增 `[sandbox]` 段 + 序列化器 | 现仅 `nomi-config/src/config.rs:417-419` 的 seatbelt 开关 |
| C3 | **UI 策略显式声明** | 默认值会打死 `nomi-computer` / `nomi-browser` / `nomi-a11y`（含 `crates/agent/nomi-a11y/src/windows/`） |
| C4 | **`hostLoopback` 显式声明** | 默认 `deny`，会撞死 dev server / LSP / 本地代理 |
| C5 | **schema 版本钉死 + 配置走 `--config-base64`** | 策略含绝对路径，不落临时文件；0.8 与 0.6/0.7 网络字段不兼容 |
| C6 | **按后端生成策略** | 「reject it rather than weakening the policy」【官】——一份策略走不通所有后端 |
| C7 | 扩展 `enforce_sandbox` | `platform/windows.rs:2874-2886`，现只有 3 个静态分支 |

#### D · 进程劫持与生命周期 ← **硬阻断点在此**

现状链：`CommandBuilder` → `spawn_child_process` → `platform/windows.rs` 的 **Job Object + `ExactProcessIdentity`**，
同时支撑 `recovery.rs` 的孤儿检测、`ChildProcessCleanup` 的"进程树清理已证明"语义
（`command_builder.rs:518-531`）、`ManagedChildProcess` 的单权威所有权。

**已核实的阻断**（详见可行性清单）：

- MXC 在 `appcontainer_runner.rs:1186-1206` 创建自己的 Job Object 并设 UI 限制（`:1192`），
  然后把**仍挂起**的子进程 `AssignProcessToJobObject` 进去（`:1193`）；
  失败即 `TerminateProcess` 并返回（`:1200-1205`）
- MXC 的 `UiJobObject::new` 用 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 且**调用 `set_ui_limits`**
  （`job_object.rs:235-287`）
- DSH 侧 `platform/windows.rs:1677-1695` 的 `arm_process_job` **只设** `KILL_ON_JOB_CLOSE`，
  **未设任何 breakaway 限制**
- 官方文档：嵌套作业"by default **if** the system can form a valid job hierarchy **and neither job sets UI limits**"
  【官】——MXC 的 job 设了 UI 限制，故**不能**与 DSH 的 job 形成嵌套

**后果**：若 DSH 把 `wxc-exec.exe` 放进自己的 Job，MXC 对沙箱子进程的 `AssignProcessToJobObject`
将失败 → MXC 主动终结并返回错误 → **`processcontainer` 后端每次 spawn 都失败**。

| # | 工作 | 风险 |
|---|---|---|
| D1 | 验证"沙箱进程能否加入调用方 Job 层次"（可行性清单 D-A） | **决定方案形态，必须先做** |
| D2 | 确认 `ExactProcessIdentity` 捕获的是沙箱内真实进程而非 wrapper | 否则孤儿检测与"精确清理证明"**静默失真**，测试可能仍绿 |
| D3 | 证明 Job 的 `KILL_ON_JOB_CLOSE` 覆盖面从 wrapper 延伸到沙箱内进程树 | wrapper 被杀时真实进程树可能存活 |
| D4 | 运行时拒绝的**新错误通道** | 现有 `enforce_sandbox` 只能做 spawn 前的静态拒绝 |

#### E · 拒绝分型接线

| # | 工作 | 落点 |
|---|---|---|
| E1 | `CapabilityDenied` 补 `hint`（结构化替代方案） | `request.rs:115-116`；对照 `BrowserError::Unsupported { capability, hint }`（`nomi-browser-engine/src/engine.rs:184-185`） |
| E2 | 加 `RetryDecision`：策略拒绝 → `Retryable`；后端不支持 → `Fatal` | 对照 `actions.rs:867-891` |
| E3 | 区分 `Denied` 与 `Abort` | 现有旧账「EOF 即视作批准」（`agent-harness-modes-review.zh.md:68`）的正解 |
| E4 | 新增后端不可用错误，**不回落**成 `UnrestrictedLocalOwner` | `ProcessError::code()` 是 const match（`request.rs:149-164`），加变体需同步 |
| E5 | 类型化拒绝贯通到工具返回值 | `local_agent_invocation.rs:156` 目前在拼字符串 |

#### F · 平台残留物治理

| # | 工作 |
|---|---|
| F1 | Tier 3 宿主 NTFS ACL 回滚：per-ACE 状态文件 + 启动时孤儿清扫 + panic unwinding + Ctrl-C handler |
| F2 | 强杀后的 ACL 自检（残留 ACE 在资源管理器里不可见，`icacls` 才看得到） |
| F3 | `windows_sandbox` 后端的状态语义备案（原转引的 `taskkill /F /IM` 在当前 main 已被
PID-reuse-safe 的 scoped teardown 取代；VM 复用现由 state-aware 生命周期的调用方负责——**接入前需按当时版本重新核实**） |
| F4 | 探测走绝对路径 / `LOAD_LIBRARY_SEARCH_SYSTEM32` 等反降级措施 |

#### G · 验证与发布

| # | 工作 |
|---|---|
| G1 | Windows 侧 MXC 契约测试（现仅 macOS Seatbelt：`tests/pty_contract.rs:336`、`tests/process_contract.rs:251`） |
| G2 | **反向验证**：允许的路径确实能写、拒绝的路径确实 `EPERM` 且带 `hint`——不只测"能跑通" |
| G3 | 网络隔离真实测试（`bun install` / `cargo fetch` / `gh` 都要出网） |
| G4 | `wxc-exec.exe` 打进 Windows Tauri 包 + 版本/schema 兼容矩阵进 CI |
| G5 | **`--audit` 期间 AppContainer 限制不被强制**——只能在隔离开发机 / CI 跑，此约束写进 CI 与文档 |

#### 5.1.1 顺序

```text
B（学习，零依赖）──┐
                  ├─→ C（策略作者层）─→ D（⚠️ 先验证）─→ F ─→ G
A（拍板，零依赖）─┘         ↑                  ↑
                    E ──────┴──────────────────┘
         （E 定错误契约，D 定生命周期契约，两者共同决定 C 的形状）
```

- **可立即并行开工**：A（拍板）+ B（`--audit` 学习）。两者都不碰生产代码，
  且 B 的产出同时喂 C、E、F
- **必须在 C 之前出结论**：D 组的 job 归属验证。**若不可行且退路不成立，
  强制形态的接入方案要重做**，C / F 的投入即作废

**与既有文档的接线：**

- `docs/agent-store/21-open-decisions.zh.md` D3=B（`[approvals]` 落点）= 本文层 0 + 层 5 的接口；
  **层 0 与层 8** 建议作为新待决项挂进同一决策体系
- `2026-09-16-agent-harness-architecture-review.zh.md` H2（无进程沙箱）/ H3（权限链）/
  M5（MCP 无 per-tool 闸门）= 本文层 2 / 层 1 与层 3 的前置证据
- `docs/agent-store/16-…zh.md` §7 是公共契约决策记录的规范落点（"公共契约变更先写 `16` §7，
  再改基线文档"）

---

## 六、待决项

| # | 问题 | 建议默认 | 阻塞谁 |
|---|---|---|---|
| D-A | **无人值守（AutoWork / Full Auto）打到审批门时怎么办**：放宽沙箱策略、还是让判官授权、还是挂到超时？ | 放宽沙箱策略（保持判官不可授权）；超时必须是显式失败而非静默 | 层 4、层 5 |
| D-B | **`[approvals]` 默认口径**：全部放行（只把显式列出的当审批）还是只放行只读？ | 前者（对现有行为零影响，未配置即完全不变）——沿用 `21` `:82` 的建议 | 层 0、层 5 |
| D-C | **判官读不读工具输出**：读（更准，但把不可信数据读进决策上下文）还是不读（更安全，但活在无知里）？ | **都不读**，与 Claude Code 的两阶段 Classifier 同款口径 | 层 4 |
| D-D | **策略升级的阈值与人工确认点**：N 次批准后自动写策略，还是产出候选让人点？ | 产出候选 + 人点（自动变宽是最危险的自动化） | 层 8 |
| D-E | **MXC 的定位**：实验性后端（默认关）还是目标后端？ | 实验性（README 自陈"不应被视为安全边界"），同时盯三个转正信号：Windows `deniedPaths` 落地、Tier 2 进入默认构建、`isolation_session` 从 Insider 转正 | 层 2 |

---

## 参考文献

| # | 来源 | 分级 |
|---|---|---|
| 1 | 本仓库源码与测试（逐条标注文件:行） | 【源】 |
| 2 | [Configure the sandboxed Bash tool — Claude Code Docs](https://code.claude.com/docs/en/sandboxing) | 【官】 |
| 3 | [Configure permissions / Choose a permission mode — Claude Code Docs](https://code.claude.com/docs/en/permissions) | 【官】 |
| 4 | [Human-in-the-loop — LangChain Deep Agents Docs](https://docs.langchain.com/oss/python/deepagents/human-in-the-loop) | 【官】 |
| 5 | [Microsoft eXecution Container (MXC) — README](https://github.com/microsoft/mxc/blob/main/README.md) | 【官】 |
| 6 | [MXC Sandbox Policy Spec v0.8.0](https://github.com/microsoft/mxc/blob/main/docs/sandbox-policy/0.8.0/policy.md) | 【官】 |
| 7 | [MXC — Windows OS-version policy support](https://github.com/microsoft/mxc/blob/main/docs/process-container/os-version-support.md) | 【官】 |
| 8 | [MXC Internals: How Microsoft's eXecution Containers Actually Isolate Agent Code](https://www.originhq.com/research/mxc-execution-containers-internals) | 【转】 |
| 9 | [AI Agent 实现原理与实践（五）：Permission, Approval & Sandbox](https://skyan.github.io/posts/agents-arch-5/) | 【转】 |
| 10 | John Hughes, "How we built Claude Code auto mode"（93% / classifier 8.5%→0.4% / 熔断数据出处） | 【转】 |
| 11 | David Dworken & Oliver Weller-Davies, "Beyond permission prompts"（84% / 文件与网络隔离缺一不可的出处） | 【转】 |

> 说明：`【转】` 分级下的 MXC 白皮书《Internals》与社区综述均非一手材料；
> 其中 Codex / Guardian / pi / nanobot 的实现细节**未独立核实**，
> 只作方向参照。写入决策前应回到上游一手文档或源码。

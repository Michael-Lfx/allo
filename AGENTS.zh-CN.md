# AGENTS.zh-CN.md

> 英文原版请参阅 [AGENTS.md](AGENTS.md)。

Flowy 是一套基于 Rust + Tauri + React 的本地优先（local-first）自动化平台。它通过单一的 axum 后端驱动 Shell 命令行、文件系统、浏览器、桌面应用、Agent、MCP 服务器及远程能力 API，支持两种宿主模式（桌面端与 Web 端）并配套一个 React 19 单页应用（SPA）。

## 技术栈

- **后端：** Rust（2024 edition，resolver 3）、axum、SQLite（sqlx）、Tauri 2
- **前端：** React 19 + TypeScript + Vite 6 + Arco Design + UnoCSS
- **包管理器：** Bun（>= 1.3.13）——严禁使用 pnpm 或 npm
- **工作区架构：** 一个 Bun 工作区（`ui/`），一个 Cargo 工作区（`crates/`）

## 目录路由与职责划分

| 路径 | 职责范围 |
| --- | --- |
| `apps/web/` | 独立 `nomifun-web` 服务器（包含 API + SPA）。 |
| `apps/desktop/` | 内嵌后端的 Tauri 桌面端外壳。 |
| `crates/agent/` | 独立的 AI Agent 引擎（包含 24 个 crate：23 个 `nomi-*` + `flowy-web`；包含用于会话日志的 `nomi-agent-trace`）。高度内聚自洽；对后端基础工具 crate 仅有少量明确文档记录的依赖。 |
| `crates/backend/` | 45 个 `nomifun-*` crate：负责 HTTP/WS 服务、数据存储、鉴权与各项业务特性。 |
| `crates/shared/` | 5 个跨层通用工具 crate（`flowy-ssh`、`nomi-process-runtime`、`nomi-redact`、`nomifun-models-dev`、`nomifun-net`）。新增 shared crate 须极为审慎。 |
| `ui/src/common/` | 跨宿主共享代码：API 客户端、公共类型、适配器与通用工具。 |
| `ui/src/platform/` | 宿主桥接层：运行时桥接与主题设计变量（tokens）。严禁在渲染层直接引入 Tauri。 |
| `ui/src/renderer/` | 页面视图、业务组件、React Hooks、服务与样式。 |
| `docs/` | 用户手册、架构文档、贡献指南与技术规范。 |
| `scripts/` | 构建辅助脚本、质量门禁检查器与发版工具链。 |

**核心架构边界：** 后端业务特性代码归属 `crates/backend/`；Agent 引擎代码归属 `crates/agent/`。后端调用 Agent 引擎必须统一经过 `nomifun-ai-agent`（唯一桥接入口）。严禁在没有 feature gate 与明确架构文档依据的情况下，让后端 crate 直接依赖 `nomi-*`。

## 常用命令

| 命令 | 使用场景 |
| --- | --- |
| `bun install` | 安装前端 JavaScript/TypeScript 依赖。 |
| `bun run dev` | 启动桌面端 / Tauri 开发模式（包含内嵌后端）。 |
| `bun run dev:web` | 浏览器 + 后端协同开发模式（禁用鉴权，仅监听 localhost）。 |
| `bun run dev:ui` | 纯前端 Vite 开发迭代（不启动后端）。 |
| `bun run build` | 构建当前操作系统的桌面端安装包。 |
| `bun run build:ui` | 构建 React SPA 前端产物到 `ui/dist/`。 |
| `bun run test` | 运行 Rust 全量测试套件（`cargo test`）。 |
| `bun run test:fast` | 运行快速测试套件（`cargo nextest`）。 |
| `bun run check` | 全量质量门禁：前端检查（类型检查 · i18n · 主题 · 按钮布局 · 图标 · 废弃CSS · CodeMirror）+ 仓库级门禁（错误暴露契约 · 支持面契约 · 进程运行时边界 · 浏览器平台边界 · Windows控制台隐藏）+ Agent Store 门禁（市场清单 · 协议指纹 · 跨仓库发版同步）+ 脚本注册表校验。 |
| `bun run typecheck` | 针对 `ui/` 前端执行 TypeScript 类型检查。 |
| `bun run fmt` | 格式化 Rust 代码（`cargo fmt`）。 |
| `bun run clean` | 深度清理构建缓存与磁盘空间。 |
| `cargo check --workspace` | 校验整个 Cargo 工作区中所有 Rust crate 是否正常编译。 |
| `cargo test -p <crate>` | 针对指定 crate 执行针对性的 Rust 单元测试。 |

自诊断命令：`cargo run -p nomifun-app --bin nomicore -- doctor` 可探测系统已安装的各种 Agent CLI 并输出诊断表格。

## 验证梯子（Verification Ladder）

始终遵循“运行覆盖你改动的最小必要检查”原则。完整梯子表格请参阅 [CONTRIBUTING.zh-CN.md](CONTRIBUTING.zh-CN.md)（或 [EN](CONTRIBUTING.md)）。

| 变更类型 | 最小必要检查 |
| --- | --- |
| 前端 TypeScript（`ui/`） | `bun run typecheck` |
| Agent Store 前端（`web/`） | `cd web && bun run typecheck && bun run test` |
| 前端各专项检查（`ui/`） | 包含在 `bun run check` 中（`check:i18n` / `check:theme` / `check:icons` / …） |
| Rust 编译正确性 | `cargo check -p <crate>` |
| Rust 行为与功能逻辑 | `cargo test -p <crate>` |
| 数据库迁移变更 | 运行迁移测试 + `cargo test -p nomifun-db` |
| 根目录脚本与注册表 | `bun run help --check` |

发起 PR 前的通用检查：`cargo check --workspace && bun run check`

> `bun run check` 是全量聚合门禁。它包含了 `ui/` 前端校验（typecheck · i18n · theme · button-layout · icons · dead-css · codemirror）、仓库级门禁（error-surface contract · support-surface contract · process runtime boundary · browser platform boundary · windows-console-hide）、Agent Store 门禁（market manifest · protocol fingerprint · cross-repo release sync），最后校验脚本注册表。注意：Agent Store 前端（`web/`）**不**在该检查链路内——涉及 `web/` 的变更请手动执行 `cd web && bun run typecheck && bun run test`。

## 高风险敏感区域

触碰以下区域前必须提前沟通确认：

- **数据库迁移**：位于 `crates/backend/nomifun-db/migrations/` 的只增（append-only）SQL。修改时必须同步更新 Models 数据模型、Repositories 仓储层以及数据库迁移测试。
- **认证与安全**：`crates/backend/nomifun-auth/`（JWT、CSRF、限流、bcrypt）。发现安全漏洞请根据 [SECURITY.md](SECURITY.md) 流程报告，严禁提公开 Issue。
- **进程运行时边界**：由 `scripts/check-process-runtime-boundary.mjs` 强制守护。严禁绕过交接白名单。
- **Agent 词汇与协作模型**：（已退役）之前由 `scripts/check-agent-vocabulary.mjs` 守护；现有协作模型正随着 Multi-Agent V2 自然演进。
- **打包资源与第三方代码**：引入新依赖或静态资源前须严格核实开源许可证兼容性。详见 CONTRIBUTING.md § Dependencies, Assets, And Licenses。
- **版本发布、签名与更新机制**：参见 [RELEASING.zh-CN.md](RELEASING.zh-CN.md)（或 [EN](RELEASING.md)）与 [BUILD_RELEASE.zh-CN.md](BUILD_RELEASE.zh-CN.md)。
- **严禁在生产代码中混入 Mock 假数据或伪造额度**：切勿在生产环境上下文（`*Context.tsx`）、Hooks 或运行时状态中硬编码假余额、虚假账户状态、绕过用 Token 或测试默认值。

## 编码规范

- 优先复用既有设计模式，避免引入多余抽象。
- Rust：提交前务必执行 `cargo fmt`。依赖统一使用根目录 `Cargo.toml` 声明的 workspace 依赖。
- 前端：使用路径别名（`@/`、`@common/`、`@renderer/`）。所有面向用户的文本必须经过 i18n 国际化（同时支持 `zh-CN` 与 `en-US`）。主题相关改动必须通过 `bun run check:theme`。
- HTTP DTO 数据传输对象统一归属于 `nomifun-api-types`。
- Commit 规范：严格采用 Conventional Commits 风格（`feat:`、`fix:`、`docs:` 等）。
- **技术方案与设计文档**：必须遵循 [docs/contributing/technical-solution-standard.zh.md](docs/contributing/technical-solution-standard.zh.md) 规范（采用双层架构模型：前半部为清晰精炼的架构，后半部在附录中完整逐字归档历史技术依据与上下文；Mermaid 语法必须防报错；严禁在 `git diff` 中丢失既有关键信息）。

## 前端质量红线

完整的历史复盘、问题剖析与防范边界详见 [docs/contributing/frontend-quality-red-lines.zh.md](docs/contributing/frontend-quality-red-lines.zh.md)（[英文版](docs/contributing/frontend-quality-red-lines.md)）。

1. **生产运行时绝对零 Mock 污染**：
   - 严禁在生产环境 Context（`*Context.tsx`）、自定义 Hooks 或运行时状态中硬编码假余额、虚构账户状态、Bypass 凭据或测试回退值。UI 预览必须使用完全隔离的 Sandbox 文件或测试专属页面，绝不侵入生产主路径。
   - 对计费、积分余额及用户鉴权 Context 的变更必须放入极小、独立的 PR 中，严禁与常规 UI 样式或业务功能 PR 混杂。
2. **完整的 i18n 覆盖率与零回退泄漏**：
   - 任何面向用户的文本（按钮、Pill 标签、Tooltip 提示、Popover 浮窗、Badge 徽标、aria-label 属性、弹窗标题、Toast 错误提示）必须对称声明在 `zh-CN` 和 `en-US` 两套词典中（`ui/src/renderer/services/i18n/locales/`）。
   - 校验流程：运行 `bun run gen:i18n` 更新 `i18n-keys.d.ts`，运行 `bun run check:i18n` 校验，并在配套单测中增加双语言断言。
3. **双主题（明亮与暗色）深度兼容与 CSS 规范完整性**：
   - 所有 UI 组件必须使用语义化设计 Token（如 `text-t-primary`、`bg-fill-1`、`var(--border-base)`、`var(--flowy-attention)`）优雅适配 Light 和 Dark 模式。
   - 单方向的边框宽度类（如 `border-t`、`border-b`）必须同时指定显式的边框样式类（例如 `border-t-solid`）。
   - 校验流程：在明暗双主题下肉眼确认视觉效果，执行 `bun run check:theme`，并确保 `bun run check:dead-css` 无任何警告。

## Git 工作流：基于 `origin/main` 切分支，Rebase 后提 PR

完整的规范背景与避免踩坑指南请参阅 [docs/contributing/git-workflow.zh.md](docs/contributing/git-workflow.zh.md)（[英文版](docs/contributing/git-workflow.md)）。

**切勿直接提交到 `main` 分支。** 无论是本地提交还是直接 push 均被严格禁止。每一次改动必须走特性分支与 Pull Request。

1. **从远端拉取最新基线，切出分支**：`git fetch origin && git checkout -b <branch> origin/main`。
2. **在分支上开发并提交**：遵循 Conventional Commits 规范，且严格归属于人类作者。
3. **提交 PR 前通过 Rebase 同步远端**：`git fetch origin && git rebase origin/main`，保持提交历史绝对线性。
4. **推送分支并创建 PR，等待 CI 通过后合并**：`git push -u origin <branch>`。CI 标红或未完成前严禁合并。
5. **合并后同步本地**：`git checkout main && git pull --ff-only`。

严禁通过直接 merge 产生菱形分支分叉，未经仓库所有者明确许可严禁对 `main` 进行强推（force-push）。

## Git 提交署名必须唯一识别真实人类

每一次 Git 提交都必须明确将贡献归属到负责的真实人类开发者。AI 工具可以辅助编写，但**绝对不能**以 author、committer、co-author 或任何鸣谢贡献者的身份出现在提交记录中。

- 严禁在 Git author / committer 中使用任何 AI 模型、AI 厂商、机器人或代理身份（包括 Claude、Codex、GPT、ChatGPT、Gemini、Copilot、OpenAI、Anthropic 等）。
- 严禁在 Commit message 中添加任何 AI 鸣谢或协作尾注（如 `Co-authored-by`、`Generated-by`、`Assisted-by`）。
- 克隆仓库后务必运行 `bun run setup:git-hooks` 以启用本地提交署名门禁。严禁使用 `--no-verify` 绕过钩子检查。
- 在对历史记录进行 amend 时，必须保留原已知的人类作者与提交者。若无法确定责任人，统一使用 `RiKa0-0 <2206491416@qq.com>` 作为保底 author 与 committer。
- 每次 push 前，务必通过 `git log -n 5` 自行审查提交元数据，确保完全合规。

## 深度技术文档索引

- [CONTRIBUTING.zh-CN.md](CONTRIBUTING.zh-CN.md)（[EN](CONTRIBUTING.md)）—— 完整贡献约定与 PR 检查清单
- [docs/contributing/project-structure.md](docs/contributing/project-structure.md) —— 权威仓库全景图
- [docs/contributing/technical-solution-standard.zh.md](docs/contributing/technical-solution-standard.zh.md)（[EN](docs/contributing/technical-solution-standard.md)）—— 技术方案编写规范与 Mermaid 规范
- [docs/contributing/frontend-quality-red-lines.zh.md](docs/contributing/frontend-quality-red-lines.zh.md)（[EN](docs/contributing/frontend-quality-red-lines.md)）—— 前端质量红线与复盘分析
- [docs/contributing/git-workflow.zh.md](docs/contributing/git-workflow.zh.md)（[EN](docs/contributing/git-workflow.md)）—— Git 分支、Rebase 与署名规范指南
- [docs/architecture/overview.md](docs/architecture/overview.md) —— 双宿主架构模型与请求流转全景
- [docs/architecture/backend-crates.md](docs/architecture/backend-crates.md) —— 后端各 crate 职责归属
- [docs/architecture/agent-engine.md](docs/architecture/agent-engine.md) —— Agent 引擎各 crate 职责
- [docs/architecture/frontend.md](docs/architecture/frontend.md) —— React SPA 路由划分与平台适配器
- [docs/architecture/agent-observability-and-eval.zh.md](docs/architecture/agent-observability-and-eval.zh.md) —— Session 会话日志与 Agent 评估套件
- 业务领域架构文档：[多媒体创作](docs/architecture/media-creation.zh.md)、[云服务与计费](docs/architecture/cloud-billing.zh.md)、[学习系统](docs/architecture/learning.zh.md)、[POI 与洞察](docs/architecture/poi-insights.zh.md)、[智能客服](docs/architecture/customer-service.zh.md)、[机器人网关](docs/architecture/robot-gateway.zh.md)、[SSH 会话管理](docs/architecture/ssh-sessions.zh.md)
- [docs/contributing/development.md](docs/contributing/development.md) —— 开发调试闭环、数据目录与命令行工具
- [docs/contributing/building-and-packaging.md](docs/contributing/building-and-packaging.md) —— 构建发版产物指南
- [docs/reference/configuration.md](docs/reference/configuration.md) —— 环境变量与配置项详解
- [docs/reference/troubleshooting.md](docs/reference/troubleshooting.md) —— 常见问题排查手册

## Agent 技能与规范（Agent Skills）

### Issue 追踪器
Issue 均托管于 GitHub Issues，使用 `gh` 命令行工具操作。详见 `docs/agents/issue-tracker.md`。

### 分流标签（Triage labels）
五种标准规范标签：`needs-triage`、`needs-info`、`ready-for-agent`、`ready-for-human`、`wontfix`。详见 `docs/agents/triage-labels.md`。

### 业务领域文档布局
采用单一上下文结构：仓库根目录维护一个 `CONTEXT.md`，架构决策记录集中存放在 `docs/adr/`。详见 `docs/agents/domain.md`。

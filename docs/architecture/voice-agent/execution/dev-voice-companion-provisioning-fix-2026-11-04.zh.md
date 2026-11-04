# 开发环境语音组件缺失：修复方案（2026-11-04）

> 状态：已实施（解析器 / ensure 脚本 / dev 文案 / 文档；验收见 §7）
> 分支：`fix/voice-missing-service-component`（自 `origin/main` 拉出）
> 症状：源码检出里启用语音时提示「当前安装包缺少语音服务组件」。

## 1. 一句话结论

**这不是发布包问题，是开发环境问题**：`bun run dev` 从不构建、也不放置 `voice-companion.exe`；而宿主解析器只认几个固定目录，即使开发者手工构建（尤其按打包惯例带 `--target`）也可能落不到解析器查的位置。干净检出必现。

## 2. 干净状态复现（已实测）

本机已删除全部历史产物（`target/debug/ai-mate.exe` 10-26、`target/debug/voice-companion.exe` 11-03、
`target/x86_64-pc-windows-msvc/release/voice-companion.exe` 10-29、`build.noindex/debug/bin/` 与
`apps/desktop/resources/bin/` 里的副本 10-31），此后：

```bash
# 解析器等价探针（候选顺序与 Rust 实现一致）
$ bun build.noindex/tmp/resolve-probe.mjs
RESOLVER RESULT: none -> backend returns voice_companion_missing
exit=1

# 标准 dev 命令
$ bun run dev
# 启动正常、窗口出现，但 150s 内不产生任何 voice-companion 可执行文件
# 日志：Voice companion autostart is enabled, but acoustic models are not yet ready; standing by
```

结论：干净 main 上必现；本机此前能用，是因为残留的手工构建产物恰好命中候选路径。

## 3. 根因（两层）

### 3.1 dev 流程不产守护进程

- `bun run dev` → `tauri dev` → `cargo run -p nomifun-app`，只构建 Flowy 本体；
- `voice-companion` 在 `crates/backend/nomifun-app/Cargo.toml` 中是 **`[dev-dependencies]`**（仅测试用 lib），`cargo run` 不会构建它的 bin；
- `tauri.dev.conf.json` 的 `beforeDevCommand` 为空、`bundle.resources` 被覆盖为 `[]`，dev 也不复制资源；
- 解析器候选只有 `target/{debug,release}`、`build.noindex/{debug,release}`，以及 `NOMIFUN_BUNDLED_BIN_DIR`/exe 同目录。

### 3.2 解析器不认识真实产物布局

| 构建方式 | 产物路径 | 解析器是否认识 |
| --- | --- | --- |
| `cargo build -p voice-companion`（不带 `--target`） | `target/debug/voice-companion.exe` | ✅ |
| `cargo build --target x86_64-pc-windows-msvc`（打包/CI 惯例） | `target/<triple>/debug/…` 或 `build.noindex/<triple>/debug/…` | ❌ |
| `prepare-voice-companion.mjs` 暂存 | `apps/desktop/resources/bin/voice-companion.exe` | ❌（dev 不复制资源） |
| debug `tauri build` 复制 | `build.noindex/debug/bin/voice-companion.exe` | ⚠️ 仅当 dev 的 exe 恰好在该目录时 |

## 4. 边界问题清单

| 编号 | 边界 | 状态 | 影响 |
| --- | --- | --- | --- |
| B1 | 全新检出无任何守护进程 | 已实测 | 必现缺失错误 |
| B2 | `--target <triple>` 布局不被解析 | 已实测（产物存在仍报缺失） | 「编了也找不到」 |
| B3 | 旧命名 `ai-mate(.exe)` 仍被接受（names 数组 + `resolve_ai_mate_binary` 别名） | 已证实（代码 + 本机 10-26 残留命中） | 静默运行旧协议守护进程；违背命名规范 |
| B4 | 解析顺序 `bundled_bin` 优先于 exe 同目录且不看 mtime | 已证实（代码顺序） | 陈旧副本压制新构建 |
| B5 | dev 从不刷新守护进程 | 已证实 | 宿主与守护进程协议漂移 |
| B6 | `CARGO_MANIFEST_DIR` 在 `cargo run` 子进程里存在（实测 `Ok(...)`），但直接双击/IDE 运行已构建 exe 时不存在 | 已实测一半 | 非 cargo run 启动时 cwd 相对候选失效 |
| B7 | 没有文件名之外的校验（版本/哈希） | 已证实 | 过期二进制只能靠后续握手报错暴露 |
| B8 | `dev:web` 宿主未挂语音路由 | 已证实（apps/web 无 voice 引用） | web 模式无语音（需在文档写明） |
| B9 | 升级安装不清旧 `bin/ai-mate.exe` | 推理（NSIS 不删旧文件） | 目录长期残留旧命名 |

## 5. 修复设计

### R1 解析器（`crates/backend/nomifun-app/src/voice_companion_manager.rs`）

1. **命名唯一化**：候选名只保留 `voice-companion` / `voice-companion.exe`；删除 `resolve_ai_mate_binary` 别名与测试里的旧命名用例。
2. **候选目录扩展**（按优先级）：
   - `VOICE_COMPANION_BIN`（仅当是文件）
   - `NOMIFUN_BUNDLED_BIN_DIR`
   - 当前 exe 同目录
   - 仓库根推导（依次：cwd → `CARGO_MANIFEST_DIR` 祖先 → `current_exe` 祖先中最近的 `Cargo.toml`/`.git`）下：
     `target/{debug,release}`、`build.noindex/{debug,release}`、
     `target/<triple>/{debug,release}`、`build.noindex/<triple>/{debug,release}`、
     `apps/desktop/resources/bin`
   - 保留现有 `target/`、`build.noindex/` 直接候选
3. **选择规则**：把所有存在候选按 mtime 取最新（消除 B4 的陈旧优先），返回规范化路径。
4. 抽取纯函数 `voice_companion_candidate_paths(root, exe, env) -> Vec<PathBuf>` 与
   `select_newest(paths) -> Option<PathBuf>` 以便单测。

### R2 dev 预置脚本（`scripts/ensure-voice-companion.mjs`）

- **行为**：按 R1 同一候选表检查；缺失或比 `apps/voice-companion/src` 最新 mtime 旧 → 执行
  `cargo build -p voice-companion --bin voice-companion --no-default-features`（debug、不带 `--target`），
  成功后确认 `target/debug/voice-companion(.exe)` 存在。
- **开关**：`NOMIFUN_SKIP_VOICE_COMPANION=1` 跳过；`--check` 只检查不构建（验收用，退出码 0/1）。
- **清理**：发现 `target/debug/ai-mate(.exe)` 等旧命名产物时删除并打印提示。
- **接线**：`package.json` 的 `dev` 在 `run-dev.mjs` 之前调用；新增 npm script 需同步
  `scripts/scripts.json` 并跑 `bun run help --readme`（README 生成表）。失败只告警，不阻断 dev。
- 单测：`scripts/ensure-voice-companion.test.mjs`（已提前写好，见 §6）。

### R3 dev 专属提示（i18n）

- 后端在 `cfg!(debug_assertions)` 下缺失时返回错误码 `voice_companion_missing_dev`；
- `ui/src/renderer/pages/voiceSession/VoiceErrorStatus.tsx` 映射到新 key
  `voice.errors.componentMissingDev`；
- `ui/src/renderer/services/i18n/locales/{zh-CN,en-US}/voice.json` 双语新增：
  zh：「开发环境未构建语音组件：运行 `bun run dev` 会自动构建，或执行 `bun run ensure:voice-companion`」；
  en 对应文案。`voiceLocales.test.ts` 增加对称性断言。

### R4 文档

- `docs/contributing/development.md(.zh.md)`：dev 语音前置说明（首次 `bun run dev` 会构建守护进程、
  可用 skip 变量、旧产物清理）；
- `docs/reference/troubleshooting.md(.zh.md)`：新增「开发环境提示缺少语音服务组件」条目（区分安装包缺失）；
- 当前文档里的旧命名统一为 voice-companion（历史执行档案保持原样）。

## 6. 测试集（先写测试，后实现）

| 测试 | 位置 | 断言 |
| --- | --- | --- |
| 解析候选包含 triple 布局 | `voice_companion_manager/tests.rs` | 构造临时目录 + `target/x86_64-pc-windows-msvc/release/voice-companion.exe` → 仍能解析到 |
| 只认 voice-companion | 同上 | 只有 `ai-mate.exe` 时必须返回 None |
| 陈旧副本不压制新构建 | 同上 | bundled 旧副本 + exe 同目录新文件 → 返回新文件 |
| ensure 脚本缺失即构建 | `scripts/ensure-voice-companion.test.mjs` | 注入假 spawn/fs：缺失/过期 → 触发 cargo build；新鲜 → 跳过 |
| ensure 不阻断 | 同上 | cargo 失败 → 进程退出码 0 且打印告警 |
| `--check` 语义 | 同上 | 未就绪 exit 1；就绪 exit 0 |
| 旧命名清理 | 同上 | 存在 `target/debug/ai-mate.exe` → 删除并提示 |
| i18n 对称 | `ui/.../voiceLocales.test.ts` | 新 key 中英文都存在 |

## 7. 验收标准（不打包完整安装包）

1. `bun scripts/ensure-voice-companion.mjs --check`：删除产物后应 **exit 1**；运行 `bun scripts/ensure-voice-companion.mjs` 后 **exit 0**。
2. `target/debug/voice-companion.exe` 存在且 mtime 晚于 `apps/voice-companion/src` 最新文件。
3. `cargo test -p nomifun-app voice_companion`（仅相关测试）全绿。
4. `bun test scripts/ensure-voice-companion.test.mjs` 全绿。
5. `bun run typecheck` + `bun run check:i18n` 通过。
6. **复测复现**：杀掉全部 Flowy/webview2/vite 进程、删除 `target/debug/voice-companion.exe` 与
   `apps/desktop/resources/bin/voice-companion.exe` 后重跑 `bun run dev`：脚本自动重建守护进程，
   日志不再出现缺失错误；再删一次并 `NOMIFUN_SKIP_VOICE_COMPANION=1` 跑 dev，应打印 dev 专属提示而非「重装安装包」。
7. `bun run check`（仓库门禁）通过；`cargo check -p nomifun-app` 通过。
8. 不要求：构建完整 Flowy 安装包、`verify:voice-package`、发布渠道验证（本次不涉及发布链路）。

## 8. 风险与回滚

- **首启变慢**：首次 `bun run dev` 增加约 2 分钟（`cargo build -p voice-companion`，增量后很快）；
  可用 `NOMIFUN_SKIP_VOICE_COMPANION=1` 跳过。
- **删除旧命名产物**：仅删除构建输出目录下的 `ai-mate*`，不动源码与用户数据。
- **解析器新增候选**：只增不减（除旧命名回退），回滚按文件还原即可。
- **旧安装包兼容**：R1 删除 `ai-mate` 回退后，仍依赖 `bin/ai-mate.exe` 的超旧安装包会退回缺失提示；
  打包侧自 #385 起只产出 `voice-companion.exe`，视为合理收敛（文档注明）。

## 9. 我替领导拍的板（默认决策，可被否决）

1. `ensure-voice-companion` **挂进 `bun run dev`**（自动构建），带 skip 开关 —— 理由：同事的代价是一整天，2 分钟可接受。
2. 旧命名 **彻底移除**（解析器不再认 `ai-mate`），构建输出目录中的旧产物由脚本清理 —— 依据领导明确要求「应该叫 voice-companion」。
3. 本次 **只修 dev 链路**；release arm64 CRT / Windows 渠道发布问题不在本 PR 范围（另记 issue）。
4. 验收 **不构建完整安装包**，按 §7 的 dev 级验收执行。

## 10. 参考文件

- 触发点：`crates/backend/nomifun-app/src/voice_companion_manager.rs:2862`（解析）、`:3088`（缺失错误）
- 现有测试：`crates/backend/nomifun-app/src/voice_companion_manager/tests.rs:1804`
- UI 映射：`ui/src/renderer/pages/voiceSession/VoiceErrorStatus.tsx:7`
- dev 入口：`package.json` `dev`、`scripts/run-dev.mjs`、`apps/desktop/tauri.dev.conf.json`
- 打包先例：`scripts/prepare-voice-companion.mjs`（debug profile 支持）、`scripts/ensure-ui-dist.mjs`
- 诊断探针（临时，gitignored）：`build.noindex/tmp/resolve-probe.mjs`

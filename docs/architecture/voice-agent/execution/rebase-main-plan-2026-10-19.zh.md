# feat/xiaozhi-voice Rebase origin/main 实施与冲突解决计划（2026-10-19）

## 1. 目标与范围

将 `feat/xiaozhi-voice` 分支当前 40 个提交及正在开发的 61 个文件变更，安全、线性地变基到最新的 `origin/main`（当前 HEAD 为 `b2afbcfb3`，包含 `v1.6.3` 及 PR #330 ~ #345），确保历史严格线性、无分叉、解决所有潜在语义冲突并通过全量质量门禁。

## 2. 事实基线与证据

- **当前分叉点（Merge Base）**：`3b32a4c73010064bf0e3abdbf94eb1569db9b642`
- **提交差异**：当前分支 40 commits vs `origin/main` 45 commits
- **已提交重叠文件（10个）**：经 `git merge-tree` 模拟检测，提交历史层面的重叠改动大多正交，可干净自动合入（Auto-merging）。
- **工作区未提交文件（61个）**：存在 `+9102 / -1350` 行的未提交代码，包含与 main 冲突的高危热点文件。

## 3. 高危冲突点与解决策略

| 冲突文件 | origin/main 改动 (PR) | 本分支工作区改动 | 解决与对齐策略 |
| :--- | :--- | :--- | :--- |
| `crates/backend/nomifun-conversation/src/service.rs` | PR #344：会话目录 Skill 自动发现与加载逻辑 (+228行) | 语音会话状态绑定与投影同步 (+314行) | 保留 main 的 Skill 自动扫描装配，将语音运行时与会话事件发射机制并入新的会话流管道中，确保两者互不覆盖。 |
| `crates/backend/nomifun-ai-agent/src/types.rs` 与 `factory/nomi.rs` | 引入 thinking 配置与 tool_calls 严格配对 (+240行) | 工具调用策略与 protocol sink | 采用 main 的新类型结构，把语音相关的工具白名单策略重新映射至新类型定义中。 |
| `Cargo.lock` | 更新依赖至 v1.6.3 (PR #345 等) | 引入 sherpa-onnx-sys 等语音依赖 | Rebase 期间若 lock 冲突，使用 `git checkout --theirs Cargo.lock`（或保留 main 版本），解完代码冲突后执行 `cargo check -p nomifun-app` 让 Cargo 自动刷新生成一致的 Lockfile。 |
| `ui/src/renderer/services/i18n/i18n-keys.d.ts` | 增加连接器与市场文案 key | 增加语音配置与反馈 key (+218行) | 忽略两边 diff，冲突时保留任意版本，随后在前端执行 `bun run gen:i18n` 自动重建。 |

## 4. 分步执行步骤（待用户开发完毕后启动）

### Step 1: 现场保护与工作区归整
```powershell
# 1. 创建备份分支（安全底线，防止 rebase 异常中断导致工作区丢失）
git branch feat/xiaozhi-voice-backup-20261019

# 2. 将当前 61 个修改文件形成明确的阶段性提交（或 git stash）
git add .
git commit -m "feat(voice): WIP companion live audio, session binding and diagnostic handoff"
```

### Step 2: 拉取并执行变基
```powershell
# 1. 抓取最新远端引用
git fetch origin

# 2. 变基到 origin/main
git rebase origin/main
```

### Step 3: 冲突解决与工具链刷新
1. 若停在 `nomifun-conversation/src/service.rs`：手工比对合入会话状态与技能加载。
2. 若停在 `Cargo.lock`：`git checkout --ours/theirs Cargo.lock`，继续 `git rebase --continue`。
3. 变基完成后重新生成并校验依赖与词条：
   ```powershell
   cargo check -p nomifun-app
   bun run gen:i18n
   ```

### Step 4: 质量门禁校验
```powershell
# 前端类型检查与国际化校验
bun run typecheck
bun run check:i18n

# 后端核心语音与会话测试
cargo test -p nomifun-conversation
cargo test -p nomifun-voice-terminal
```

## 5. 验收标准（Acceptance Criteria）

1. [ ] `git status` 显示位于 `feat/xiaozhi-voice`，且 `git log -n 1 origin/main` 包含在当前分支历史中（严格线性，无多余 merge commit）。
2. [ ] `cargo check -p nomifun-app` 无报错。
3. [ ] `bun run typecheck` 无类型错误。
4. [ ] 会话服务测试 `cargo test -p nomifun-conversation` 全部绿色通过。
5. [ ] 语音伴侣与前端设置页面正常构建加载。

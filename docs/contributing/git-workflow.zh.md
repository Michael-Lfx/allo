# Git 工作流与人类贡献者归属规范 (Git Workflow & Human Attribution Guide)

> **定位**：Flowy 仓库（`nomifun-tauri`）Git 分支规范、Rebase 要求与人类署名硬性门禁。  
> **核心铁律**：**“严禁直推 main，必须基于 origin/main 建分支并 Rebase；严禁 AI 署名，每一行提交必须归属于人类开发者。”**

---

## 1. 核心流程：基于 `origin/main` 建分支、Rebase 后提 PR

**严禁直接向 `main` 分支提交代码。** 不允许在本地 `main` 提交，更不允许直接 `git push origin main`。仓库内任何改动——哪怕是一行文档错别字修复——也必须走特性分支与 Pull Request。

### 1.1 标准五步走流水线
1. **以远端最新为起点，绝不用陈旧的本地 `main`**：
   ```bash
   git fetch origin
   git checkout -b <branch-name> origin/main
   ```
   *背景*：本地 `main` 滞后是导致后续合入产生冲突或多余 Merge Commit 的最常见原因。
2. **在分支上开发并提交**：
   - 遵循 Conventional Commits 格式（`feat:`, `fix:`, `docs:` 等）；
   - 遵守人类真实身份署名规范（详见第 2 节）。
3. **在发起 PR 前，通过 Rebase 追平最新代码**：
   ```bash
   git fetch origin
   git rebase origin/main
   ```
   *原则*：保持提交历史呈严格线性，杜绝产生 `Merge remote-tracking branch 'origin/main'` 节点。若 PR 开启期间 `main` 发生推进，合并前必须再次 Rebase。
4. **推送分支、发起 PR、等待 CI 并合入**：
   ```bash
   git push -u origin <branch-name>
   ```
   任何必需 CI 门禁为红色或处理中状态时，严禁合并 PR。
5. **合入后同步本地主干**：
   ```bash
   git checkout main && git pull --ff-only
   ```

### 1.2 保护机制所预防的三大历史事故 (Lessons Learned)
本规范是基于仓库早期经历的实际痛点建立的：
- **消除嵌套 Merge Commit**：基于旧 main 建分支后若使用 `git merge origin/main`，合入主干时会产生混乱的双重合并节点；严格使用 Rebase 则历史纯净；
- **防止误推未审核半成品**：直接向 `main` 推送会导致本地未经验证或他人未完工的改动意外发布，完全绕过 CI 门禁；
- **防止孤立提交丢失**：若本地 `main` 存在未推送提交，应主动确认提交人意图，严禁擅自发布或强制覆盖。

---

## 2. 核心铁律：Git 归属必须且仅能识别人类开发者 (Human Attribution)

这是 `nomifun-tauri` 仓库的本地强制策略，不会修改任何开发者的全局 Git 配置。

**每一次 Git Commit 必须将工作明确归属于负责的人类工程师。AI 工具可以辅助编写代码，但绝对不能作为 Author、Committer、Co-Author 或以任何形式出现在贡献名单中。**

### 2.1 归属红线与禁止清单
1. **严禁 AI 身份署名**：严禁在 Git Author 或 Committer 的姓名与邮箱中使用任何 AI 模型、AI 产品、供应商或机器人身份。禁止出现的标识包括但不限于：`Claude`, `Codex`, `GPT`, `ChatGPT`, `Gemini`, `Copilot`, `OpenAI`, `Anthropic`。
2. **严禁添加 AI 致谢尾注**：提交信息中严禁添加 `Co-authored-by`、`Generated-by`、`Assisted-by` 等 AI 赞誉标签。（技术说明中提及模型名称作为功能描述除外）。
3. **本地 Hook 守护机制**：克隆仓库后，必须运行：
   ```bash
   bun run setup:git-hooks
   ```
   该脚本会启用仓库级的归属拦截钩子。**严禁在提交时使用 `--no-verify` 绕过检查**。
4. **追溯与重写历史时的保全规则**：在 Amend 或重写历史时，必须保留原有已知人类作者；如果确实无法确定具体责任人，统一使用保底人类身份：
   ```text
   Author / Committer: RiKa0-0 <2206491416@qq.com>
   ```
5. **自检要求**：在推送前，运行 `git log -n 5` 自行检查 Author、Committer 和 Commit Message，确保 100% 符合人类归属规范。

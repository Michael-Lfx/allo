# Git Workflow & Human Attribution Guide

> **Scope**: Branching models, linear rebase rules, and human attribution enforcement for the `nomifun-tauri` repository.  
> **Golden Principle**: **"Never commit to main, branch off origin/main and rebase; attribution strictly belongs to human developers."**

---

## 1. Core Workflow: Branch Off `origin/main`, Rebase, Then PR

**Never commit to `main`.** Not on the local branch, and not by pushing it. Every change — including a one-line documentation fix — goes through a branch and a pull request.

### 1.1 Five-Step Standard Pipeline
1. **Start from the remote, not from stale local `main`**:
   ```bash
   git fetch origin
   git checkout -b <branch-name> origin/main
   ```
2. **Commit on the branch**:
   - Follow Conventional Commits format (`feat:`, `fix:`, `docs:`, etc.);
   - Comply with human attribution rules (see Section 2).
3. **Catch up by rebasing before opening the PR**:
   ```bash
   git fetch origin
   git rebase origin/main
   ```
   Keep the branch linear without merge commits. Rebase again if `main` moves while the PR is open.
4. **Push the branch, open the PR, wait for CI, merge**:
   ```bash
   git push -u origin <branch-name>
   ```
   Do not merge a PR whose required checks are red or pending.
5. **After the merge, sync local main**:
   ```bash
   git checkout main && git pull --ff-only
   ```

### 1.2 Historical Accidents Prevented (Lessons Learned)
- Eliminates messy `Merge remote-tracking branch 'origin/main'` commits;
- Prevents accidental publishing of unreviewed local work on `main` without PR or CI gates;
- Prevents overwriting stranded commits on unpushed local branches.

---

## 2. Git Attribution Must Identify a Human

This is a repository-local rule for `nomifun-tauri`. It does not modify global Git configuration.

**Every commit must attribute the work to the responsible human developer. AI tools may assist with a change, but they must never appear as the author, committer, co-author, or other credited contributor.**

### 2.1 Attribution Rules
- Never use an AI model, AI product, vendor, bot, or agent identity in the Git author or committer name/email (e.g. Claude, Codex, GPT, ChatGPT, Gemini, Copilot, OpenAI, Anthropic).
- Never add AI-credit trailers or equivalent attribution to a commit message (`Co-authored-by`, `Generated-by`, `Assisted-by`).
- Run `bun run setup:git-hooks` to enable repository-local attribution checks. Never bypass those checks with `--no-verify`.
- Preserve the known human author and committer when amending or rewriting history. If the responsible human cannot be determined, use `RiKa0-0 <2206491416@qq.com>` as both author and committer.
- Inspect affected commits (`git log -n 5`) before pushing to verify compliance.

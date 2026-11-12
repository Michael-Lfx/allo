# Agent Store 发版操作手册 (Release Runbook) · 技术方案

> **状态**：✅ 现行操作规范（Beta 发版执行依据，覆盖 `0.1.0-beta.N`）
> **最近核对**：2026-11-12（以 `0.1.0-beta.15` 三出口全流程实证为基线）
> **适用范围**：Agent Store npm 四包 + 站点仓 GitHub Release 资产 + 官网双语文档站上线
> **前置阅读**：[`09-release-readiness.md`](file:///c:/workspace/allo/docs/agent-store/09-release-readiness.md)、[`12-sdk-packaging.md`](file:///c:/workspace/allo/docs/agent-store/12-sdk-packaging.md)、[`18-marketplace-spec.zh.md`](file:///c:/workspace/allo/docs/agent-store/18-marketplace-spec.zh.md)
> **一句话原则**：**一次发版同时覆盖 npm 四包、GitHub Release 与站点三个出口；严守 7 条不变量与 S0~S8 顺序流水线，用同一份 EXE 与版本锁步杜绝破损发布。**
>
> **正文与附录**：正文回答「今天怎么发」；文末附录逐字保留重构前底稿（含旧版本号与已退役做法），仅用于追溯，不随正文更新。两者冲突时以正文为准。

---

## 1. 背景与核心痛点

Agent Store 由 Rust 引擎可执行程序、四个互相依赖的 npm 包、双语文档站与 GitHub Release 下载包组成，跨两个仓库协同发布。以下四类事故决定了流程必须机械化：

1. **二义性二进制**：npm runtime 包内的 EXE 与 Release ZIP 内的 EXE 若分别构建，用户会拿到行为不同的底层。判据是两者 SHA-256 完全相同。
2. **先宣称上线、后上传资产**：站点 `release.json` 驱动首页下载直链；资产未就绪就发站，首页立即 404。
3. **发布顺序与时间窗**：`sdk` 依赖 `runtime-win32-x64`；先发 sdk 或中途失败，`npm i` 会静默装成「无二进制」。
4. **注册表 CDN 滞后**：发布后 `npm view` 可能返回旧值或假 404，误判会导致重复发布与版本污染。

### 1.1 旧做法 vs 现行做法

| 环节 | 旧做法（附录底稿口径） | 现行做法（本手册） |
|---|---|---|
| 推送 | 直接 `git push origin main` | 从 `origin/main` 起分支 → PR → 合并；`main` 永不直接提交 |
| 发布 CI | 期待 GitHub Actions 全自动 | **无发布 CI**；本地逐环门禁 + 人工执行，CI 只做常规检查 |
| 站点构建 | 依赖 GitHub Webhook 自动触发 | EdgeOne API `CreatePagesDeployment` 指定 sha 手动触发 |
| Release 回验 | 必须下载回算 sha256 | 下载回验优先；网络挂住时用资产 `digest: sha256:` 与本地 zip 比对等价确认 |
| 市场内容 | 站点产物整树镜像 | 官方源在 ModelScope；`/source/**` 仅保留头像，`_files.txt` 必须 404 |

---

## 2. 方案全景与三大出口

### 2.1 端到端拓扑

```mermaid
flowchart TD
    subgraph BuildEngine ["1. 统一构建源头（本仓）"]
        SRC["Rust 代码 + WebUI 静态包"]
        EXE["唯一二进制: target/release/agent-store.exe<br/>记录 SHA-256"]
    end

    subgraph OutNPM ["出口一：npm 官方注册表"]
        N1["protocol"]
        N2["client"]
        N3["runtime-win32-x64（内嵌 EXE）"]
        N4["sdk（集成套件）"]
    end

    subgraph OutRelease ["出口二：GitHub Release（站点仓）"]
        ZIP["flowy-agent-store-v版本-windows-x86_64.zip"]
        SUM["SHA256SUMS.txt"]
        NOTES["RELEASE_NOTES.md"]
    end

    subgraph OutSite ["出口三：官网与双语文档站"]
        SITE["EdgeOne 双语站点"]
        DOCS["API 文档 / changelog / upgrade"]
    end

    SRC --> EXE
    EXE -->|"装配 runtime 包"| N3
    N1 --> N2 --> N3 --> N4
    EXE -->|"打包并核对哈希"| ZIP
    EXE -->|"生成摘要"| SUM
    ZIP & SUM & NOTES -->|"发布 prerelease"| OutRelease
    OutRelease -->|"资产就绪后部署"| OutSite
```

### 2.2 三个出口的交付定义

| 出口 | 产物 | 执行 | 成功判据 |
|---|---|---|---|
| **npm** | 四个同版本包，全部挂 `beta` | `web/scripts/publish-packages.ts` | canonical packument（`registry.npmjs.org/@flowy-agent-store%2Fsdk`）的 `dist-tags.beta`、`versions`、`time` 已转正；tarball 可下载可晚到 |
| **GitHub Release** | ZIP + `SHA256SUMS.txt` + `RELEASE_NOTES.md`，标 prerelease | 站点仓 `scripts/release.mjs`（`release:pack` / `release:publish` / `release:status`） | `release:status` 显示 `draft=false`、`prerelease=true`；资产体积与哈希核对通过；资产 HEAD 200 |
| **站点上线** | 双语文档站 + 下载页 | EdgeOne Makers API | 部署 `Status=Success` 且 `UsedInProd=true`；生产页 200 |

### 2.3 非目标

- 桌面端 Flowy 发版：见仓库根 `RELEASING.md` / `BUILD_RELEASE.zh-CN.md`。
- 产品准入判定：见 `09-release-readiness.md`（本文只回答「怎么发」）。
- 市场树刷新：见站点仓 `docs/market-maintenance.md`；只有本次发布确实改市场树才做。
- 二进制瘦身验证：见 `14-binary-size-trimming.zh.md`。

---

## 3. 发版 7 大核心不变量

任一不成立直接终止发版。

| # | 不变量 | 违背后的后果 | 机械判据 |
|---|---|---|---|
| **1** | 协议指纹全仓一致（本次基线 `fp-21`；权威值是 `PROTOCOL_VERSION` / `APP_SERVER_PROTOCOL_VERSION`，文档不复制其值作判据） | 握手严格相等失败，SDK 拒连 | `bun run check:fingerprint` exit 0 |
| **2** | 版本锁步：四个 `package.json` + SDK 依赖 pin + 五个平台 runtime pin + 站点 `content/release.json` 同值 | 半发布，或站点下载链接指向不存在的版本 | `bun run check:release-sync` exit 0 |
| **3** | 文档公开的路由拆分与真实路由表一致（现行 `56 / 83`：mapped / total；源为 `http-transport.test.ts` 的 `DOCUMENTED_ROUTE_SPLIT`） | 对消费者的覆盖面承诺失真 | `bun run check:release-sync` exit 0 |
| **4** | 同一份 EXE 服务两个出口：npm runtime 包内的 EXE 与 Release ZIP 内的 EXE SHA-256 相同 | 两类用户拿到不同底层 | `release:pack --expect-sha256 <vendor exe 哈希>` |
| **5** | 站点双语文档结构镜像：标题层级 / 代码块 / 表格列数 / 内链一致 | 单语信息脱落 | 站点 `pnpm run check:docs-sync` exit 0 |
| **6** | 先有 Release 资产，后有站点宣告 | 首页下载 404 | 顺序约束：S6 完成 → S7 |
| **7** | 历史发布事实永不改写：版本号、发布时间、`changelog` / `upgrade` 条目 | 历史记录变成假记录 | 人工遵守 D10=A；写错只追加「更正（日期）」 |

---

## 4. 标准有序发版流水线 (S0 ~ S8)

> 下列命令中的 `0.1.0-beta.15` 是本次实证基线，实际执行时替换为目标版本；通用示例见 §7。

```mermaid
sequenceDiagram
    autonumber
    participant Dev as 开发者 / 发布机
    participant Repo as 本仓 allo
    participant Site as 站点仓 agent-store-site
    participant NPM as npm 官方源
    participant GH as GitHub Releases
    participant Edge as EdgeOne 静态站

    Dev->>Repo: S0 检查两仓工作树干净
    Dev->>Repo: S1 构建带 static-webui 的唯一 EXE
    Dev->>Repo: S2 版本锁步 dry-run
    Dev->>Site: S3 改 release.json 与双语文档
    Dev->>Repo: S4 两仓门禁逐环校验
    Dev->>NPM: S5 顺序发布四包 (TAG=beta)
    Dev->>Site: S6 打包并发布 GitHub Release
    Site->>GH: 资产就绪并完成哈希回验
    Dev->>Site: S7 分支+PR 合并到 main
    Dev->>Edge: S7 用 main sha 触发部署
    Dev->>Repo: S8 台账登记与 changelog 转正
```

### S0 前置：环境与工作树

```powershell
git status --short                          # 本仓必须干净
git -C ..\agent-store-site status --short    # 站点仓必须干净
bun install
cd ..\agent-store-site; pnpm install; cd ..\allo
```

工作树必须干净：S1 会基于当前源码构建，未提交改动会被烧进已发布 EXE，且「已发布二进制 == 某个提交」不再可复核。若确有并发改动，等它提交，或在干净 HEAD 的独立 worktree 里只做 S1。

### S1 构建唯一的 EXE

```powershell
bun run agent-store:build     # web build → cargo build --release -p agent-store --features static-webui → 复制到 cli/
Get-FileHash target\release\agent-store.exe -Algorithm SHA256
(Get-Item target\release\agent-store.exe).Length
```

- **必须带 `--features static-webui`**（`agent-store:build` 已带）。不带它二进制只提供 API，SPA 兜底 404；`publish-packages.ts` 的兜底自动构建恰恰不带该 feature。也可用 `AGENT_STORE_BIN` 指向已明确构建的文件。
- 本机链接若报预编译 `sherpa`（MT）与 `ort`（MD）的 ABI 冲突，改用带匹配 MD 库的发布机构建；EXE 是唯一进入 npm 与 ZIP 的产物，Rust 源码不必搬过去。
- 记录权威哈希与字节数。S6 的 `--expect-sha256` 用 `web/packages/runtime/vendor/flowy-agent-store.exe` 的哈希，即 npm 里真正发布的字节。

### S2 版本锁步预演 (dry-run)

```powershell
$env:DRY_RUN="1"; $env:VERSION="0.1.0-beta.15"; $env:TAG="beta"
bun web/scripts/publish-packages.ts
Remove-Item Env:DRY_RUN,Env:VERSION,Env:TAG

git diff --stat                  # 只应出现版本号与依赖 pin 的变化
bun web/scripts/verify-published-sdk.ts web/packages/runtime/vendor/flowy-agent-store.exe
```

- `VERSION` 每次显式给；不带会写回脚本内置默认版本，打坏版本锁步。
- dry-run 不联网，但会改四个 `package.json` 的版本并把二进制落进 `web/packages/runtime/vendor/`（已 gitignore）；它跳过 `verify-published-sdk.ts`，要手工补跑，期望输出 `VERIFY-OK`。
- 结果就是版本锁步终态。提交走分支 + PR（示例：beta.15 的 PR #440），不直接提交 `main`。

### S3 站点版本与文档

1. 站点 `content/release.json` 的 `version` 改为同一版本。
2. 按 §5 清单同步中英双语文档；未发布台账（`changelog` §4、`upgrade` §8）此时仍保留，S8 再转正。
3. 提交走站点仓分支 + PR（示例：beta.15 的 PR #19 / #20）。

### S4 两仓门禁逐环校验

```powershell
bun run check:fingerprint
bun run check:release-sync
cd web; bun run typecheck; bun run test; cd ..
cargo test -p nomifun-app-server --lib
cd ..\agent-store-site; pnpm run check:release; cd ..\allo
```

- `bun run release:check` 的第 1 环是 `ui/` 全量 typecheck，历史基线恒红且与 Agent Store 无关。因此按环执行并**如实登记读数**；ui 环未绿时不得声称 `release:check` 通过。
- 站点 `check:release` 覆盖 docs-sync、market 与 typecheck。
- beta.15 的完整读数见 §8。

### S5 npm 顺序发布

```powershell
$env:VERSION="0.1.0-beta.15"; $env:TAG="beta"
bun web/scripts/publish-packages.ts
Remove-Item Env:VERSION,Env:TAG

npm view @flowy-agent-store/sdk dist-tags versions --json
```

- 内部顺序固定：`protocol` → `client` → `runtime` → `sdk`。runtime 必须先于 sdk 上线。
- `TAG=beta` 必须显式给，预发布绝不落 `latest`。
- 认证用 Granular Access Token（`@flowy-agent-store` 读写 + Bypass 2FA），通过临时 `npm_config_userconfig` 注入；不要改全局 `~/.npmrc`，否则 `npm whoami` 会因 registry 指向 npmmirror 报 `ENEEDAUTH`。
- 读取侧 CDN 滞后：`npm view` 可能给旧值或假 404；以 canonical packument 的 `versions` / `time` / `dist-tags` 转正为准，tarball HEAD 与 `npm i` 可能再晚 10 分钟以上。runtime 大包异步约 7 分钟可见；中途 409「previously staged」属正常，不要重复发布。
- 发布前确认 `bun run dev` 不被打死：写入 vendor EXE 时 Windows 会锁文件，`web/vite.config.ts` 的 `watch.ignored` 已排除该目录。

### S6 GitHub Release 资产发布（站点仓）

```powershell
cd ..\agent-store-site
bun run release:pack -- --exe C:\workspace\allo\target\release\agent-store.exe --expect-sha256 <vendor exe 的 sha256>
bun run release:publish
bun run release:status
```

- `--expect-sha256` 填 `web/packages/runtime/vendor/flowy-agent-store.exe` 的哈希，不是 `target/release` 的哈希；两者应一致，这一步就是挡「同版本两个二进制」。
- `release:publish` 流程：建草稿 → 上传 → 体积回读 → 下载回算 sha256 → 转正式 prerelease。慢点在**下载回验**（国内网络可能挂住）。
- 下载回验挂住时：用 `gh api repos/szStarWave/agent-store-site/releases/tags/v<version>` 的资产 `digest: sha256:<hex>` 与本地 zip 的 sha256 比对；一致即可视为等价验证，再执行 `gh release edit v<version> --repo szStarWave/agent-store-site --draft=false --prerelease` 转正。
- 要人工检查时用 `--keep-draft` 停在草稿；资产传错且仍在草稿用 `--replace-existing` 重传。

### S7 站点上线（分支 → PR → 合并 → 部署）

1. 站点仓从 `origin/main` 起分支，提交并开 PR，合并到 `main`（示例：beta.15 的 site PR #19 / #20）。
2. 合并后取 `main` 上的 40 位 sha：`git -C ..\agent-store-site rev-parse main`。
3. 用 EdgeOne API `CreatePagesDeployment`（`Provider=Github`、`RepoBranch=main`、`RepoCommitHash=<sha>`）触发生产构建；轮询到 `Status=Success` 且 `UsedInProd=true` 才算上线。接口与脚本见站点仓 `docs/deploy-trigger.md`。
4. 部署后自检（顺序即用户体验顺序）：
   1. 中英 `/<lang>/docs/typescript-sdk`、`changelog`、`upgrade` 有预渲染正文（非 SPA 空壳），且含目标版本标记。
   2. 构建日志无 `File count exceeds project limit` / `File size limit exceeded`（总文件 ≤ 20,000，单文件 ≤ 25 MiB）。
   3. `/source/<market>/_files.txt` 与旧目录返回 404；目录页头像 200 且大小写一致。
   4. 三个官方市场源 `wb-market/<market>/index.json` 均 200，且与 `wb-market/state.json` 的 `index_sha256` 一致（WMC 侧 `uv run wbcollect pipeline verify`）。
   5. 首页下载直链能真实下载 `v<version>` 的 ZIP。

### S8 台账与变更转正

1. 站点 `changelog` §2 追加本次「已发布事实」（版本号 + 发布时间 UTC + 改了什么）；把 §4 的未发布条目转成 §2 正式条目；`upgrade` §2 / §3 / §8 同步，破坏性发布新增 §6.x 升级小节。版本号与发布时间永不改写。
2. 本仓 `docs/agent-store/README.md` 增加「本轮（日期 发布 `<version>`）」记录：PR、npm 时间、Release 资产哈希、部署 ID、门禁读数、已知边界。
3. 记录 dist-tag 决策：预发布只动 `beta`，`latest` 保持不动（beta.15 仍为 `0.1.0-beta.2`）。

---

## 5. 站点文档同步核对清单

| 站点文件 | 何时改 | 判据 |
|---|---|---|
| `content/docs/{zh-CN,en-US}/typescript-sdk.md` §2 常量行 | 协议指纹 bump | `bun run check:fingerprint` |
| 同页 §5.3「HTTP 绑定」的 `**56 / 83**` 与路由表外方法名单 | 路由表拆分变化 | `bun run check:release-sync` |
| 同页 §1 的「版本状态」注（当前版本号 + `latest` / `beta` 指向） | 每次发版 | 人工 |
| 同页 §5/§7 的方法签名、子客户端、错误码 | 新增/删除 wire 方法或错误码 | 人工 |
| 同页 §8 MCP 接入指南 | 工具面 / allowlist / 声明文件语义变化 | 人工 |
| `content/docs/{zh-CN,en-US}/changelog.md` §2（已发布事实 + JSON 块）与 §4 台账 | 每次发版：§4 清零并转正 | 人工（S8） |
| `content/docs/{zh-CN,en-US}/upgrade.md` §2 发布序列 + §3 dist-tag/JSON | 每次发版 | 人工（S8） |
| 同页 §6 逐版本升级步骤 | 任何破坏性发布必须新增一节 | 人工（S8） |
| 同页 §8「已发布产物的差异与自查方法」 | 未发布项清零时同步改写 | 人工（S8） |
| `content/docs/{zh-CN,en-US}/examples-sdk.md` | 对外用法变化 | 人工 |
| `content/docs/{zh-CN,en-US}/compatibility.md` | 已发布平台变化 | 人工（与站点 `src/lib/platform.ts` 的 `RELEASED_PLATFORMS` 同改） |
| `content/docs/{zh-CN,en-US}/{configuration,plugins-market}.md` | 市场源类型或官方默认源地址变化 | 人工（两页同改） |
| 市场内容（`wb-market/<market>/index.json`） | 市场内容变化 | WMC 生成上传；核对 `wb-market/state.json` 的 `index_sha256`，不要手改 |
| `content/release.json` | 每次发版 | `bun run check:release-sync` |
| 所有页的双语结构 | 任何时候 | 站点 `pnpm run check:docs-sync` |

---

## 6. 异常处置与失败回退

| 故障场景 | 表现 | 处置与回退 |
|---|---|---|
| npm 半发布（runtime 已上、sdk 未上） | 中间状态 | 不用回滚：新版 sdk 还不存在，旧 sdk 仍指向旧 runtime；修好后只补发缺失包 |
| npm 已全量但二进制有缺陷 | 注册表已有公开版本 | npm 不能删版本、不能改已发布内容；`npm deprecate` 标注 + 立刻发 `beta.N+1`，不复用版本号 |
| Release 资产传错 | 下载哈希不符 | 草稿态用 `release:publish --replace-existing` 重传；已正式发布只补缺资产用 `gh release upload --clobber`；要换二进制则发下一版 |
| Release 下载回验挂住 | `release:publish` 长时间停在下载 | 用资产 `digest: sha256:` 与本地 zip 比对；一致后 `gh release edit ... --draft=false --prerelease` |
| 站点文档发布后 404 / 空壳 | 路由缺失或预渲染失败 | 新页补进站点 `src/lib/docOrder.ts` / 预渲染配置，再重新部署 |
| 构建期漏了市场树 / 头像 | 线上头像变字母占位 | 必须 `pnpm run build` 而不是 `docusaurus build`，确保 `copy-market-tree` 执行 |
| npm CDN 短时 404 | 发布后 `npm i` 报 404 | 正常物理延迟；以 canonical packument 已转正为判据，不重复发布 |
| GitHub Actions Cargo job 卡住 | `apt-get` 安装阶段长时间无进展 | 视为基础设施问题；在本地完成等价门禁并如实登记，CI 未绿不得声称绿灯；确需合并时按仓库权限走 admin 合并并记录 |
| `ui/` typecheck 恒红 | `release:check` 第 1 环失败 | 与 Agent Store 无关的既有基线；按环跑并记录，修 ui 是独立一轮 |
| WMC 发布守卫拒绝发布 | 同 version 内容变化被拒 | 按守卫要求提升条目 version 后再发布；不要绕过守卫强推 |

---

## 7. 一页速查

```powershell
# ── 本仓 C:\workspace\allo ──────────────────────────────────────────
git status --short
bun run agent-store:build
Get-FileHash target\release\agent-store.exe -Algorithm SHA256
$env:DRY_RUN="1"; $env:VERSION="<version>"; $env:TAG="beta"
bun web/scripts/publish-packages.ts
Remove-Item Env:DRY_RUN,Env:VERSION,Env:TAG
bun web/scripts/verify-published-sdk.ts web/packages/runtime/vendor/flowy-agent-store.exe
# 改站点 content/release.json + 站点文档（§5 清单）
bun run check:fingerprint; bun run check:release-sync
cd web; bun run typecheck; bun run test; cd ..
cargo test -p nomifun-app-server --lib
$env:VERSION="<version>"; $env:TAG="beta"
bun web/scripts/publish-packages.ts
Remove-Item Env:VERSION,Env:TAG
npm view @flowy-agent-store/sdk dist-tags versions --json

# ── 站点仓 C:\workspace\agent-store-site ────────────────────────────
pnpm run check:release
bun run release:pack -- --exe C:\workspace\allo\target\release\agent-store.exe --expect-sha256 <哈希>
bun run release:publish
bun run release:status
# 分支 + PR 合并 main 后：
pwsh ./deploy.ps1 -CommitSha (git -C . rev-parse main)
# 部署后自检（S7）→ changelog §2 / README「本轮」台账（S8）
```

---

## 8. 验证矩阵与 beta.15 实证读数

### 8.1 门禁读数（2026-11-11 ~ 2026-11-12）

| 检查项 | 结果 |
|---|---|
| `bun run check:fingerprint` | ✅ `fp-21` |
| `bun run check:release-sync` | ✅ `0.1.0-beta.15` 四包 + 依赖 pin + 站点 `release.json` |
| `cd web && bun run typecheck` | ✅ exit 0 |
| `cd web && bun run test` | ✅ **598 passed / 3 skipped** |
| `cargo test -p nomifun-app-server --lib` | ✅ **192 passed** |
| 站点 `pnpm run check:release` | ✅ docs-sync + market + typecheck |
| `bun run check:market` | ✅ **24 / 24** |

### 8.2 三出口产物与上线事实

| 项 | 实测值 |
|---|---|
| EXE | `EE09D98EFC65EA71D1B6EF18BD766C535E67BB3877E1C329281125F5150CD51C`（222,450,176 B） |
| Release ZIP | `flowy-agent-store-v0.1.0-beta.15-windows-x86_64.zip`，86,350,301 B，sha256 `58D0119740B69C1E9AA2C4E360EC6AA4ED31A5B8A3DB0965B70E559114C8DAAA` |
| npm 发布时间（UTC） | sdk `2026-11-11T16:29:42.189Z`；protocol `16:30:15.177Z`；client `16:30:51.757Z`；runtime `16:32:36.698Z` |
| npm dist-tags | 四包 `beta = 0.1.0-beta.15`；`latest` 保持 `0.1.0-beta.2` |
| GitHub Release | `v0.1.0-beta.15` prerelease（`draft=false`），资产 HEAD 200 |
| 站点部署 | EdgeOne deployment `dpmkm01p86dg`，`Success` / `UsedInProd=true`，生产域 `agent-store.flowyaipc.cn` |
| 发布 PR | allo #439 / #440 / #441；site #19 / #20 |

---

## 9. 已知边界（登记，不在本流程修复）

1. **runtime 包只有 Windows x64 一条真实路径**：`web/packages/runtime/package.json` 的 `name` / `os` / `cpu` 固定 `win32-x64`，`publish-packages.ts` 只改版本、不改包名。
2. **两仓都没有发布 CI**：本流程全部动作手工执行，本手册即为此而写。
3. **平台字面量两处**：站点 `src/lib/platform.ts` 的 `RELEASED_PLATFORMS` / `assetName()` 与 `scripts/release.mjs` 的 `PLATFORM = "windows-x86_64"` 需人工同改；`check:release-sync` 只守版本号与路由拆分。
4. **`web/scripts/verify-published-sdk.ts` 的 `VERSION` 是遗留字面量**（仅作客户端自称名，不影响判据）。
5. **历史 stray 包**：`runtime-win32-x64@0.1.0-beta.13` 仍在版本历史、无 dist-tag，不影响安装；不要复用它。
6. 更早的细节与历史处置（S0 并发写入者、CI 恒红、tarball 404 时长等）见文末附录 §5 / §7。

---

## 附录：原始技术底稿与历史归档 (Historical & Technical Reference Archive)

> **归档说明**：以下完整保留重构前的原始技术底稿、历次讨论与历史记录全文，供历史追溯、协议字段详细对照与技术审计。

---

# Agent Store 发布流程（npm + agent-store-site）

> 定位：**发一次版的操作手册**——回答「怎么发」，不回答「能不能发」。产品准入（P0 门禁与发布结论等级）见 `09-release-readiness.md`；桌面端 Flowy 的发版见仓库根 `RELEASING.md`；市场树刷新见站点仓 `docs/market-maintenance.md`。三者互不替代。
> 最后核对：2026-09-17
> 适用范围：Agent Store 预览版（`0.1.0-beta.N`）——**npm 四个包** + **站点仓的 GitHub Release 资产** + **站点上线**
> 前置阅读：`web/AGENTS.md` §5（协议指纹与跨仓同步）· `12-sdk-packaging.md` §6（发行形态）· `18-marketplace-spec.zh.md` §8（市场发布门禁）

## 1. 一次发布有三个出口

| 出口 | 产物 | 由谁执行 | 判据 |
|---|---|---|---|
| npm | `@flowy-agent-store/protocol` / `client` / `sdk` / `runtime-win32-x64`，**同一个版本号** | 本仓 `web/scripts/publish-packages.ts` | `npm view @flowy-agent-store/sdk dist-tags versions --json` |
| GitHub Release | `flowy-agent-store-v<版本>-windows-x86_64.zip` + `SHA256SUMS.txt` + `RELEASE_NOTES.md`，**一律标 prerelease** | 站点仓 `scripts/release.mjs` | `bun run release:status` |
| 站点上线 | 官网 + 双语文档站；**GitHub 自动触发自 2026-09-17 起失效，推送后需按站点仓 `docs/deploy-trigger.md` 手动触发一次构建**；三个市场源自 doc 30 起迁至 ModelScope 归档，**不在站点产物里** | 站点仓 | 线上页面 + 产物文件数不超限（自检见 S7） |

> **发布刚完成时不要信 `npm view`**（实测 2026-09-16）：注册表读取侧有 CDN 缓存——四个包都打印 `✓ published` 之后的一两分钟里，`npm view` 仍可能返回旧值，甚至对新版本返回**假 404**。复核用 canonical packument URL：`https://registry.npmjs.org/@flowy-agent-store%2Fsdk`——**不要**给它加查询串（某些缓存节点会对带查询串的请求直接 404）；某个版本的 tarball 是否真的在，用 `HEAD .../-/<pkg>-<version>.tgz`，并先用一个**不存在的版本**确认这个探针确实会返回 404。

**不属于本流程**：桌面端 Flowy 的发版（`RELEASING.md`）；P0 准入判定（`09`）；市场树刷新（`bun run sync`，站点 `docs/market-maintenance.md`——`16` R6② 已延后，只有本次发布确实改了市场树才做）；`14-binary-size-trimming.zh.md` 的瘦身验证。

## 2. 不变量（发布前必须同时成立）

| # | 不变量 | 为什么 | 判据 |
|---|---|---|---|
| 1 | 协议指纹处处一致 | 握手与 SDK 对它做**严格相等**校验，漏一处那个夹具/mock/SDK 就直接连不上。**别把当前值抄进文档**：它是计数器，权威只有 `nomifun-app-server::PROTOCOL_VERSION` 与 `protocol.ts` 的 `APP_SERVER_PROTOCOL_VERSION` | `bun run check:fingerprint` |
| 2 | 版本锁步：四个 `package.json` + sdk 的 2 个依赖 pin + 5 个平台 runtime pin + 站点 `content/release.json` 全部同值 | `publish-packages.ts` 用一个 `VERSION` 发布四个包，站点用 `release.json` 生成下载链接 | `bun run check:release-sync` |
| 3 | 站点两语言文档引用的方法计数与实际路由表一致（现行 `48 / 71`） | 这是对消费者承诺的覆盖面 | `bun run check:release-sync` |
| 4 | **同一份 exe 服务两个出口**（npm runtime 包内的二进制 = Release zip 内的二进制） | 同版本号对应两个不同二进制，等于两类用户拿到不同的东西 | `release:pack --expect-sha256` |
| 5 | 站点双语文档结构一致 | 站点硬约束（标题层级 / 代码块 / 表格列数 / 内链） | 站点 `bun run check:docs-sync` |
| 6 | **先有 Release 资产，后有站点宣告** | 站点 `release.json` 驱动首页下载直链；先推站点就会出现指向不存在资产的 404 | 顺序约束（§3 S6 → S7） |
| 7 | 已发布事实永不改写：站点 `changelog` §2 与 `upgrade` §2/§3 的版本表 | D10=A：版本号与发布时间是历史；写错了追加「更正（日期）」 | 人工 |

`check:release-sync` 与 `check:fingerprint` 都同时看本仓与站点仓；站点不在场时**跳过并提示**（用 `AGENT_STORE_SITE_DIR` 指定别的 checkout）。

## 3. 有序清单

### S0 前置

```powershell
git -C . status --short                          # 干净
git -C ..\agent-store-site status --short         # 干净
bun install ; bun install --cwd ..\agent-store-site   # 两仓各自装一次
```

站点仓默认在同级 `..\agent-store-site`（与 `check:release-sync` 的默认一致）。放别处就用 `AGENT_STORE_SITE_DIR` 指过去，并手工在站点目录里跑它那半边的门禁。

### S1 构建唯一的 exe

```powershell
bun run agent-store:build        # web build → cargo build --release -p agent-store --features static-webui → 复制到 cli/
Get-FileHash target\release\agent-store.exe -Algorithm SHA256
```

> **必须带 `--features static-webui`**（`bun run agent-store:build` 已经带上）。该 feature 默认关闭，不带它二进制只提供 API、SPA 兜底直接 404；而 `publish-packages.ts` 在找不到现成二进制时的**兜底自动构建恰恰不带这个 feature**。要么先按本步构建好，要么用 `AGENT_STORE_BIN` 指一个明确构建过的文件。

### S2 版本锁步预演（dry-run）

```powershell
$env:DRY_RUN="1"; $env:VERSION="0.1.0-beta.5"; $env:TAG="beta"
bun web/scripts/publish-packages.ts
Remove-Item Env:DRY_RUN,Env:VERSION,Env:TAG

git diff --stat                  # 只应出现版本号与依赖 pin 的变化
bun web/scripts/verify-published-sdk.ts web/packages/runtime/vendor/flowy-agent-store.exe
```

dry-run **不上网**，但照常改写 4 个 `package.json` 的版本、照常把二进制落进 `web/packages/runtime/vendor/`（该目录已 gitignore）。它**会跳过** `verify-published-sdk.ts`，所以上面这条要手工补跑，期望输出 `VERIFY-OK`。这一步的结果就是「版本锁步」的终态，单独提交。

### S3 站点侧版本与文档

1. 站点仓 `content/release.json` 的 `version` 改成同一版本；
2. 按 **§4 同步清单**逐项过一遍站点文档（`changelog` §4 与 `upgrade` §8 的「未发布」条目在发版后要转成已发布事实，见 S8）。

### S4 门禁（两个半边都要绿）

```powershell
bun run release:check            # 本仓：仓级门禁 + web typecheck/test + nomifun-app-server 协议测试
bun run release:check:site       # 站点半边：docs-sync + market + typecheck（需要同级 checkout）
```

### S5 npm 发布

```powershell
$env:VERSION="0.1.0-beta.5"; $env:TAG="beta"
bun web/scripts/publish-packages.ts
Remove-Item Env:VERSION,Env:TAG

npm view @flowy-agent-store/sdk dist-tags versions --json
```

- 包内顺序由脚本固定：`protocol` → `client` → **`runtime`** → `sdk`。runtime 必须先于 sdk 上线，否则 `npm i` 落在窗口期会静默装成「没有二进制」（可选依赖失败只警告）。
- `TAG` **必须显式给**：预发布只挂 `beta`，绝不能落到 `latest`（脚本注释里已写明这条的理由）。
- **不带 `VERSION` 直接跑会写回脚本内置的默认版本**——那会同时打坏版本锁步（`check:release-sync` 会红）。所以每次都显式给 `VERSION`。
- 发布失败或中断：见 §5。
- **复核时注意读取侧 CDN 滞后**（见 §1 的提示）：刚发完 `npm view` 可能给旧值或假 404，用 canonical packument URL 复核，别据此判断发布失败。
- **发布前先确认 `bun run dev` 不会被打死**（2026-09-17 实测）：本步会把约 181 MiB 的二进制写进 `web/packages/runtime/vendor/`。Windows 在写入期间锁住该文件，Vite 的 watcher 抛 `EBUSY: resource busy or locked, watch '…/runtime/vendor/…exe'`，而这是 FSWatcher 的未捕获 error —— **整个 dev server 连同 Vite 一起退出**，看起来像发布把开发环境搞崩了。该目录已加进 `web/vite.config.ts` 的 `watch.ignored`；换 checkout 或回退那份配置后会复现。
- **认证用 granular token，别依赖全局 `~/.npmrc`**（§7 第 6 条的落地做法）：本机全局 `~/.npmrc` 的 `registry` 指 npmmirror，而 `//registry.npmjs.org/:_authToken` 只对 npmjs 生效，所以裸 `npm whoami` 会报 `ENEEDAUTH`——那是**注册表不匹配**，不是没凭据。用 `npm_config_userconfig` 指一个只含 `registry=https://registry.npmjs.org/` 与该 token 的临时文件即可（`whoami` 应能打印用户名）。

### S6 GitHub Release（站点仓）

```powershell
cd ..\agent-store-site
bun run release:pack -- --exe C:\workspace\allo\target\release\agent-store.exe --expect-sha256 <S1 的 sha256>
bun run release:publish          # 建草稿 Release → 上传资产 → 回读体积 → 下载回验 sha256 → 转正式（prerelease）
bun run release:status
```

`--expect-sha256` 请填 **`web/packages/runtime/vendor/flowy-agent-store.exe` 的哈希**：那才是 npm 里真正发布出去的那份字节。想先人工核对，就加 `--keep-draft` 停在草稿态。

> **慢点在「下载回验」而不是上传**（实测 2026-09-16）：上传 69 MB 资产只用 59 秒，但 `release:publish` 最后要把资产从 GitHub **下载回来**算 sha256——国内网络会在这里长时间挂住。给 `gh` 配 `HTTPS_PROXY=http://127.0.0.1:7890` 再跑；也可以先用 GitHub API 里该资产的 `digest`（服务端算的 sha256）与本地文件比对确认完整性，再决定是否重跑回验。

### S7 站点上线

```powershell
git add content/docs content/release.json
git commit -m "docs(release): 发布 0.1.0-beta.5"
git push origin main
```

推送后**不会**自动上线：EdgeOne Makers 的 GitHub 自动触发**自 2026-09-17 起失效**，且本站项目是
`Github` 型（`edgeone makers deploy` 的上传通道对它不可用）。所以推送后要**手动触发一次构建**——
接口形状、实测与失败定位见站点仓 `docs/deploy-trigger.md`。触发的提交必须是刚推上去的那个 sha。
然后做**部署后自检**（顺序即用户体验顺序）：

1. `/<lang>/docs/typescript-sdk` 中英两页都有正文（不是 SPA 空壳——空壳说明该页没进 `react-router.config.ts` 的 `prerender()`）；
2. **站点产物文件数**：EdgeOne Makers 构建日志里没有 `File count exceeds project limit`。上限是 20,000 个文件——三个市场自 doc 30 起整树**不再进产物**（现在都是索引型镜像，`market-source/` 约 1.7k 文件），产物里只有站点自身约 94 个文件 + 目录页头像约 1,708 个（本地 `bun run build` 后数 `build/client` 即可预检）。⚠️ **`/source/<market>/…` 与 `/source/<market>/_files.txt` 已退役**，不要再把它们当作部署自检项；
3. **市场索引可达且摘要一致**：三个官方源的 `wb-market/<market>/index.json` 均 200，且与 `wb-market/state.json` 的 `index_sha256` 一致（WMC 侧 `uv run wbcollect pipeline verify`）。`is_official_source` 按地址判定，源在 ModelScope，**不在本站产物里**。站点仓 `bun run publish:market -- --verify-only` 只针对冻结的旧 `{market}.zip`，不再作为发布门禁；
4. 首页/兼容性页的下载直链真能下到 zip：`https://github.com/szStarWave/agent-store-site/releases/download/v<版本>/flowy-agent-store-v<版本>-windows-x86_64.zip`。

### S8 发布后台账

1. 站点 `changelog` §2 追加本次「已发布事实」（版本号 + 发布时间 + 改了什么；改了什么只引已成文的差异结论，不编条目）；版本号与发布时间**永不改写**；本次的「未发布」条目（原 §4 台账）从 §4 转成 §2 的正式条目，`upgrade.md` §8 同步。
2. 本仓 `docs/agent-store/README.md` 加一条「本轮（日期）」记录（受影响的文档 + 跨仓事项）。
3. 记下 dist-tag 决策（是否有意移动 `latest`）——预发布不应移动它。

## 4. 站点文档同步清单

改动落到站点时**逐项对照**；「判据」一列的机械项已有门禁，人工项必须自己看一遍。

| 站点文件 | 何时改 | 判据 |
|---|---|---|
| `content/docs/{zh-CN,en-US}/typescript-sdk.md` §2 常量行 | 协议指纹 bump（任何 wire 增量） | `bun run check:fingerprint` |
| 同页 §5.3「HTTP 绑定」的 `**48 / 71**` 与路由表外方法名单 | 路由表映射数变化 | `bun run check:release-sync` |
| 同页 §1 的「版本状态」注（当前版本号 + `latest` / `beta` 指向） | **每次发版** | 人工 |
| 同页 §5/§7 的方法签名、子客户端、错误码 | 新增/删除 wire 方法或错误码 | 人工 |
| 同页 §8 MCP 接入指南 | 工具面 / allowlist / 声明文件语义变化 | 人工 |
| `content/docs/{zh-CN,en-US}/changelog.md` §2（已发布事实 + JSON 块）与 §4 台账 | 每次发版：§4 清零并把条目转正；JSON 块用注册表输出逐字更新 | 人工（S8） |
| `content/docs/{zh-CN,en-US}/upgrade.md` §2 发布序列 + §3 dist-tag/JSON | 每次发版 | 人工（S8） |
| 同页 §6 逐版本升级步骤（**带破坏性变更的发布必须新增一节**） | 任何破坏性发布 | 人工（S8） |
| 同页 §8「已发布产物的差异与自查方法」 | 未发布项清零时同步改写 | 人工（S8） |
| `content/docs/{zh-CN,en-US}/examples-sdk.md` | 对外用法（新子客户端、错误处理）变化 | 人工 |
| `content/docs/{zh-CN,en-US}/compatibility.md` | 已发布平台变化 | 人工（与站点 `app/lib/platform.ts` 的 `RELEASED_PLATFORMS` 同改） |
| `content/docs/{zh-CN,en-US}/{configuration,plugins-market}.md` | 市场源**类型**（`source_kind` 取值表）或**官方默认源地址**变化 | 人工（两页都写这两件事，必须同改；`30` §7 的迁移口径也在此） |
| 市场内容（`wb-market/<market>/index.json` + payload/entries） | 市场内容变化 | `workbuddy-market-collect` 生成上传；核对 `wb-market/state.json` 的 `index_sha256`（**不要手改**）。站点仓 `content/market-hosts.json` + `dist-market/*.zip` 仅为**冻结的旧 `{market}.zip` 台账**，不随索引发布更新 |
| `content/release.json` | 每次发版 | `bun run check:release-sync` |
| 所有页的双语结构 | 任何时候 | 站点 `bun run check:docs-sync` |

## 5. 失败与回退

| 情况 | 处置 |
|---|---|
| npm 半发布（`runtime` 已上、`sdk` 未上） | 不用慌：新版 `sdk` 还不存在，旧版 `sdk` 仍指向旧 runtime，用户侧无感。修好后只补发 `sdk`。 |
| npm 已全上但二进制有问题 | npm **不能删版本、不能改已发布内容**。`npm deprecate` 标注 + 立刻发下一个版本（`beta.N+1`），**不要复用版本号**。 |
| Release 资产传错 | 仍在草稿态 → `release:publish --replace-existing` 重传；已正式发布 → 只补缺的资产用 `gh release upload --clobber`，需要换二进制则发下一版（同版本两个二进制正是 `--expect-sha256` 要挡的事）。 |
| 站点文档已发布的数字写错 | 不改写，追加「更正（日期）」（D10=A）。 |
| `check:fingerprint` / `check:release-sync` 红 | 改**落点**（常量、站点文档、`release.json`），绝不改门禁；门禁模式失配也要修模式而不是删检查。 |
| 部署后页面是空壳 | 新页没进 `react-router.config.ts` 的 `prerender()`（站点 `AGENTS.md` 硬约束），补上再推。 |
| 构建期拷贝漏了市场树 | 站点 `bun run build` 才带 `copy-market-tree.mjs`；直接 `react-router build` 会让线上 `/source/**` 全 404。 |

## 6. 一页速查

```powershell
# ── 本仓 C:\workspace\allo ──────────────────────────────────────────
bun run agent-store:build
$env:DRY_RUN="1"; $env:VERSION="0.1.0-beta.5"; $env:TAG="beta"
bun web/scripts/publish-packages.ts
Remove-Item Env:DRY_RUN,Env:VERSION,Env:TAG
bun web/scripts/verify-published-sdk.ts web/packages/runtime/vendor/flowy-agent-store.exe
# 改站点 content/release.json + 站点文档（§4 清单）
bun run release:check
bun run release:check:site
$env:VERSION="0.1.0-beta.5"; $env:TAG="beta"
bun web/scripts/publish-packages.ts
Remove-Item Env:VERSION,Env:TAG
npm view @flowy-agent-store/sdk dist-tags versions --json

# ── 站点仓 C:\workspace\agent-store-site ────────────────────────────
bun run release:pack -- --exe C:\workspace\allo\target\release\agent-store.exe --expect-sha256 <哈希>
bun run release:publish
bun run release:status
git add content/docs content/release.json
git commit -m "docs(release): 发布 0.1.0-beta.5"
git push origin main
# 部署后自检（§3 S7）→ changelog §2 / README「本轮」台账（§3 S8）
```

## 7. 已知缺口（登记，不在本流程修复）

1. **runtime 包只有 Windows x64 这一条真实路径**：`web/packages/runtime/package.json` 的 `name` / `os` / `cpu` 固定 `win32-x64`，而 `publish-packages.ts` 只改版本、**不改包名**，脚本头注释里「linux/darwin 由 CI 走同一脚本」并没有对应 CI。换平台前先解决包名与 `os`/`cpu` 的推导。
2. **两仓都没有发布 CI**：`.github/workflows` 只有 `release-modelscope.yml`；本流程全部动作手工执行（本手册就是为此而写）。
3. **资产名与已发布平台是两处字面量**：站点 `app/lib/platform.ts` 的 `RELEASED_PLATFORMS` / `assetName()` 与站点 `scripts/release.mjs` 的 `PLATFORM = "windows-x86_64"` 必须人工保持一致，换平台时两处同改（`check:release-sync` 只守版本号，不守这个）。
4. **站点域名与 HTTPS 未定**：EdgeOne 预览域名带签名 `eo_token` 会过期，不能长期作为对外公布的市场源地址（站点 `README.md` §部署「待定」）。
5. **`web/scripts/verify-published-sdk.ts` 里的 `VERSION = "0.1.0-beta.1"` 是遗留字面量**（只作为客户端的自称名，不影响判据），发版时别被它误导。
6. **npm 发布需要「带 bypass 2FA」的凭据**：账号开了 2FA 写保护时，`npm login` 得到的 token 会在**第一个包**上失败（`E403 … Two-factor authentication or granular access token with bypass 2fa enabled is required`）——脚本在第一个包就中止，所以**注册表未被改动**（2026-09-16 实测）。改用 **Granular Access Token**（Scope 选 `@flowy-agent-store` 的 Read and write、勾 **Bypass 2FA**），用一个临时 `npm_config_userconfig` 指向的文件注入即可，不必改全局 `~/.npmrc`。
7. **S0 的「工作树干净」与并发写入者冲突，而 S1 恰恰基于工作树构建**（2026-09-18 发布 `0.1.0-beta.6` 时实测）：当时工作树里另有 546 行**未提交**的 provider 改动（`agent_store.rs` +148 / `lib.rs` +377 / `routes.rs` +7），`cargo check` 能过，但 `lib.rs` 留了一条 `unused import: AppServerModelSummary`，说明它没写完。它们**会被烧进已发布的 exe**，且「已发布二进制 == 某个提交」这条不再可复核。两条出路：**等它们提交**（S0 成立后再发），或在**干净 HEAD 的独立 worktree** 里只做 S1 那一步（Rust 源码不进 npm 包，只有 exe 进，所以不必把整条流程搬过去）。本次用户知情后选择了仍在工作树构建——**这不是默认做法，是显式决定**。
8. **`release:check` 的第 1 环恒红，S4 只能逐环跑**（2026-09-18 实测）：`release:check` = `bun run check && …`，而 `check` 的第 1 环是 `bun run typecheck`（`ui/`），那里有 **73 个既有错误 / 28 个文件**（TS2339 ×38、TS7006 ×11、TS2322 ×7、TS2345 ×6、TS2554 ×3…）——与 Agent Store 无关，且**全在已提交的代码里**（`ui/` 工作树是干净的）。所以「两仓门禁全绿」当前只能在**逐环**层面成立：本仓跑 `check:fingerprint`、`check:release-sync`、`cd web && bun run typecheck && bun run test`、`cargo test -p nomifun-app-server`，跳过 ui 那一环并**如实登记**（不得声称 `release:check` 通过）；站点半边 `release:check:site` 不受影响。修 ui 那批是独立一轮（其中 `updateTelemetry.ts` 的重复 import 只是 2 个错误）。
9. **npm 读取侧的 tarball 404 可能持续十分钟以上，别据此判定发布失败**（2026-09-20 发布 `0.1.0-beta.7` 时实测）：四个包都打印 `✓ published` 之后，canonical packument 约 1 分钟就转正（`dist-tags.beta` 与 `time` 都是新值、`versions` 里有新版本、`npm view <pkg>@<version> version` 也能打印），但 **`.../-/<pkg>-<version>.tgz` 的 `HEAD` 与 `npm i` 继续 404 / `ETARGET` 达 10 分钟以上**，之后自行恢复。判据是 **manifest 已转正**（版本出现在 `versions`、`time` 有该版本、`dist-tags` 指向它），而不是 tarball 可下载；`npm i` 装回来复核这一步要等它恢复后再做。§1 的告警只说了「一两分钟」与「假 404」，这次的**时长**是新的。
10. **发布链与「不许直接提交 `main`」的关系**（2026-09-20 记录）：`AGENTS.md` 的 §Git Workflow（从 `origin/main` 起分支、rebase、PR，禁止直接提交 `main`）是 **2026-09-20 的 PR #246 才合并**的，而 `0.1.0-beta.4`/`beta.5`/`beta.6` 三次发布都在此之前，走的是**直接提交 `main`**。规则生效后，本手册的 S2/S8（版本锁步提交、索引台账提交）与站点 S7（`git push origin main`）都应改走**分支 + PR**；`main` 当前的分支保护只有 `non_fast_forward` + `no deletion`（**没有**强制 PR），所以直接提交在技术上仍可行——那是**规则**与**门禁**的不一致，不是权限错误。站点仓 `main` 没有保护规则。

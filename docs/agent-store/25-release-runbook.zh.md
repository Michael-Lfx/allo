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
2. **站点产物文件数**：EdgeOne Makers 构建日志里没有 `File count exceeds project limit`。上限是 20,000 个文件——`market-source/` 三个市场合计 22,612 个，所以自 doc 30 起整树**不再进产物**，产物里只应有站点自身约 94 个文件 + 目录页头像约 648 个（本地 `bun run build` 后数 `build/client` 即可预检）。⚠️ **`/source/<market>/…` 与 `/source/<market>/_files.txt` 已退役**，不要再把它们当作部署自检项；
3. **市场归档可达且摘要一致**：`bun run publish:market -- --verify-only` 三个市场全绿（它 `HEAD` 每个稳定 URL，比对 `X-Linked-Etag` 与 `content/market-hosts.json` 记录的 sha256）。归档在 ModelScope，**不在本站产物里**；
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
| `content/market-hosts.json` + `dist-market/*.zip` | 市场内容变化 | `bun run pack:market` 产出、`bun run publish:market` 上传并回验（**不要手改**） |
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

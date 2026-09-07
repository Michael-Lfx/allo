# Skill 市场下载与安装方案

> **文档状态：** 方案、现状分析与首期实现记录；已补充 2026-09-07 公开源 demo、完整解耦方案与边界审查
>
> **撰写日期：** 2026-09-07
>
> **适用范围：** Skill 市场中的普通 Skill 安装；MCP Server 与插件另行处理

## 结论先行

“由 Flowy 负责下载，并落到应用数据目录”适合作为 Skill 安装的统一产品模型，且安装阶段不应依赖用户机器上的
`openclaw`、`npx`、`node`、`uv` 等外部命令。后端可以通过 HTTP 下载、在临时目录安全解压、校验 `SKILL.md`，再原子提交到：

```text
<data_dir>/skills/<skill-name>/
```

这里的 `<data_dir>` 由应用数据目录和 `NOMIFUN_DATA_DIR` 决定。该目录已经是当前用户 Skill 的 canonical root；Skill
catalog 从这里发现 Skill，对话运行前再把它投影到工作区中的 Agent 原生 Skill 目录。

但“所有市场条目使用同一个下载器”并不成立：

- SkillHub 单 Skill 和 SkillHub 专家包已经具备或接近可复用的归一化下载流程；
- LoopHub demo 已确认排行榜给出的 URL 当前能返回 ZIP，但仍需按包内 manifest 作为最终名称；
- ClawHub 当前只有 `openclaw skills install ...` 命令映射，没有 Flowy 内部的归档下载实现，不能仅凭详情页 URL 推断下载接口；
- MCP 与 ClawHub Plugin 不是普通 Skill，不应写入 `<data_dir>/skills/`。

因此建议统一的是**安装生命周期和落盘契约**，而不是强行统一每个市场的传输协议：

```text
市场条目
  -> 来源适配器解析 artifact
  -> 后端 HTTP 下载
  -> 临时目录安全解压
  -> Skill manifest 校验
  -> <data_dir>/skills/<name>/ 原子提交
  -> catalog 发现
  -> 对话按需链接到 Agent 原生目录
```

## 1. 当前实现基线

### 1.1 目录与对话使用

`SkillPaths` 在 `crates/backend/nomifun-extension/src/skill_service.rs` 中定义了以下边界：

- `user_skills_dir = data_dir/skills`：用户 Skill 的持久化目录；
- `builtin_skills_dir`：应用启动时物化的内置 Skill；
- `preset_skills_dir`：Preset 资源，不等同于用户市场 Skill；
- `cron_skills_dir`：定时任务专用 Skill。

`list_available_skills` 扫描用户 Skill 根目录并让用户 Skill 覆盖同名内置 Skill。`materialize_skills_for_agent` 按用户
Skill、内置 Skill、自动注入 Skill、cron Skill 的顺序解析来源，返回绝对源路径；调用方再将 Skill 链接到工作区的 Agent
原生目录。后端不再为每个会话复制一份 Skill，也不应把会话目录当作安装位置。

因此，市场下载完成后只需要保证：

```text
<data_dir>/skills/<skill-name>/SKILL.md
```

对话侧会自然复用现有 catalog 和 materialize 流程。

### 1.2 现在的市场“安装”是什么

当前普通市场 UI 的 Add 动作会创建一个待发送的对话草稿，最终由 Agent 执行市场命令；它不是应用托管的下载任务。
命令生成与白名单位于 `ui/src/renderer/pages/settings/skill/skillMarket.ts`，内置说明位于
`crates/backend/nomifun-app/assets/builtin-skills/auto-inject/nomifun-skills/SKILL.md`。

后端已有一个较完整的 SkillHub 专家包安装路径：

- `crates/backend/nomifun-extension/src/market/package.rs` 创建 `.market-import` 临时目录；
- 通过 SkillHub API 下载子 Skill ZIP；
- 使用安全解压和严格的 `SKILL.md` 校验；
- 全部子 Skill 成功后再原子提交；
- 发生本地提交或 Preset 提交失败时可回滚新建目录。

这条路径是新方案的最佳实现基线，但当前专家包市场 UI 已被移除，普通市场也没有调用
`POST /api/skills/market/package/install` 的入口。后续需要把“已存在的后端能力”与用户可见安装入口重新接通。

## 2. 是否适用于 Skill 市场的所有安装

### 2.1 按市场条目分类

| 条目 | 当前安装方式 | 是否适用统一落盘方案 | 判断 |
| --- | --- | --- | --- |
| SkillHub 单 Skill | Flowy 后端托管下载 | 是 | 服务端按 canonical slug 重新确认并下载 ZIP，安装本身不需要 `npx`、Node、uv 或 OpenClaw。 |
| SkillHub 专家包 | `skillhub package add ...` | 是 | 已有 staging、逐项校验、原子提交和回滚；可复用。包本身是多个 Skill 的编排资源，Preset 提交仍应是独立事务边界。 |
| LoopHub 单 Skill | `loophub skill download <download_url>` | 可纳入，需 manifest 驱动 | demo 中 `download_url` 返回 `application/zip`，归档含唯一 `SKILL.md`；排行榜显示名与 manifest 名可能不同，必须以 manifest 名提交。 |
| ClawHub 单 Skill | `openclaw skills install @owner/slug` | 不纳入当前托管安装市场 | 当前解析器只有页面地址和命令，没有 Flowy 内置的下载 artifact。不能把 ClawHub 详情页当作 ZIP，也不应在用户机上静默执行 `openclaw`。 |
| SkillHub MCP | `mcp market add skillhub:<slug>` | 否 | 这是 MCP Server 配置/连接资源，目标应是 MCP 注册表或配置存储，不是 `skills/<name>`。 |
| MCPWorld | `mcp market add mcpworld:<id>` | 否 | 同上，属于 MCP 安装生命周期。 |
| ClawHub Plugin | `openclaw plugins install ...` | 否 | 插件可能包含宿主扩展、权限和运行时代码，不能降级为一个 `SKILL.md` 目录。 |
| 本地目录/ZIP 导入 | 现有 Import API | 是 | 已有用户 Skill 根目录、ZIP 安全解压和导入校验，可与市场安装共享提交原语。 |

结论是：

> 如果“所有 Skill 安装”指所有**普通 Skill 内容**，统一的落盘和对话使用方案可覆盖；如果指市场页面上的所有条目，则不能覆盖，MCP 和 Plugin 必须排除。

### 2.2.1 现场 demo 结果（2026-09-07，首期实现前的只读快照）

demo 只访问公开 HTTP 接口，在内存中读取响应和 ZIP 目录，不执行包内脚本、不调用 `openclaw`/`npx`，也没有写入用户
`<data_dir>/skills/`：

| 检查项 | 结果 | 说明 |
| --- | --- | --- |
| SkillHub ranking | HTTP 200，`application/json` | 当前响应结构为 `data.skills[]`；抽样 `tencent-docs`。 |
| SkillHub artifact | HTTP 200，`application/zip`，378,994 bytes | ZIP magic 为 `PK`；恰好 1 个 `SKILL.md`，声明名为 `tencent-docs`，与 slug 一致。 |
| LoopHub ranking | HTTP 200，`application/json` | 抽样条目 `id=12277`，提供 `dl.cocoloop.cn` 下载 URL。 |
| LoopHub artifact | HTTP 200，`application/zip`，26,211 bytes | ZIP magic 为 `PK`；恰好 1 个 `SKILL.md`，声明名为 `self-improvement`，与展示名 `Self-Improving Agent` 不同。 |
| ClawHub ranking | HTTP 200，Convex JSON | 快照中的旧 Flowy 解析结果只有详情页 URL 与 `openclaw` 安装命令，没有已验证 artifact 字段；首期普通市场已移除该 source。 |
| `skills.sh` fallback | HTTP 200，`text/html` | 快照中的旧解析器曾把条目标记为 SkillHub，安装命令可能指向 GitHub；首期已移除该 fallback。 |

项目自带的只读 live 测试也通过：

- `live_market_pages_still_match_the_ranking_contract`：1 passed；
- `live_new_market_sources_return_ranked_items`：1 passed；
- `live_skillhub_package_ranking_contract`：1 passed。

demo 证明的是**当前公开接口和样本包的可下载性**，不是对第三方市场永久契约的保证。LoopHub 仍需增加多个样本和 fixture
测试；ClawHub 在 artifact 契约确认前应视为“发现源”，不应视为“托管安装源”。

### 2.3 统一的是生命周期，不是下载协议

所有普通 Skill 只要能被来源适配器解析为以下内部结果，就可以进入同一条安装管线：

```text
SkillArtifact {
  source
  source_id
  expected_name
  archive_or_directory
  version_or_revision (optional)
}
```

来源适配器只负责解决“从哪里拿到什么”；通用安装器负责：

1. 校验 source、slug、重定向和响应大小；
2. 将归档写入临时目录，禁止路径穿越、符号链接逃逸和过量解压；
3. 找到一个或多个 Skill 目录；
4. 校验目录是普通目录，存在合法 `SKILL.md`，名称与预期 Skill 名一致；
5. 不覆盖现有有效 Skill，不把无效同名条目替换成下载内容；只有来源、版本/修订和摘要均一致时才复用；
6. 通过 rename/原子提交进入用户 Skill 根目录；
7. 清理临时归档和失败 staging；
8. 返回已安装、复用、失败、冲突和来源信息，供 UI 刷新 catalog 和展示进度。

这是一条适合下沉到 `nomifun-extension` 的深模块边界。UI 不应继续理解 `openclaw`、`npx` 或某个市场的 URL 细节。

## 3. 推荐的落盘和运行模型

### 3.1 持久化目录

普通市场 Skill：

```text
<data_dir>/skills/<skill-name>/
├── SKILL.md
├── references/        # 可选
├── scripts/           # 可选
└── assets/            # 可选
```

平台默认数据根目录可从 `docs/guides/desktop-app.zh.md` 查询；用户配置
`NOMIFUN_DATA_DIR` 后，唯一权威路径就是该配置值下的 `skills/`。

建议约定：

- `<skill-name>` 使用来源声明的安全 slug；不得把 owner、URL 或版本字符串直接拼进路径；
- 现有同名有效目录不能仅凭目录存在就视为可复用：只有来源、版本/修订和 artifact 摘要一致时才返回 `reused`；无法证明同一内容时返回名称冲突，禁止静默覆盖；
- 同名但来自不同市场时，当前 name-based catalog 无法表达两个可执行 Skill。首期应报告冲突并让用户选择，不要自动覆盖；
- 原始 ZIP 不作为成功安装后的长期资产保存；如需更新/卸载记录，另存来源、版本和摘要元数据，不污染 `SKILL.md`；
- `builtin-skills`、`preset-skills`、`cron/skills` 与会话工作区保持独立，不作为市场 Skill 的替代存储。

### 3.2 临时目录

当前专家包使用：

```text
<data_dir>/skills/.market-import/package-<pid>-<nonce>/
```

普通市场安装建议复用这个保留目录和同样的 Drop cleanup 语义；现有通用 ZIP 导入使用的 `.import-tmp` 可以逐步收敛到同一套安全 staging 原语。staging 不应出现在 catalog，也不应在提交前被 materialize 到 Agent 工作区。

### 3.3 对话物化

安装成功后，不需要为每个对话复制 Skill。对话执行时：

```text
<data_dir>/skills/<name>/
          │
          └── symlink / Windows fallback copy
                    ▼
<workspace>/.claude/skills/<name>/
<workspace>/.agents/skills/<name>/
...
```

实际目标目录由 Agent 适配器决定；当前 `link_workspace_skills` 已提供链接/降级复制的边界。这样既便于设置页展示
`user_skills_dir`，也能让新会话和已有会话按相同 canonical source 使用 Skill。

## 4. 运行时依赖的边界

应用托管下载的价值在于：**安装阶段只需要应用自身的网络能力**，不要求用户预装 `openclaw`、`npx`、`node` 或 `uv`。
不要把“下载 Skill”和“执行 Skill 中声明的工具”混成一个依赖问题：

- 当前 `nomifun-runtime` 的 Bun 版本元数据是 `1.3.13`；是否随桌面包嵌入由 `NOMIFUN_EMBED_BUN=1` 控制，现有文档仍要求检查 Bun 或使用该构建选项；
- 当前运行时会解析 `bun`/`bunx`，并不会自动提供 `npx`/`npm`；
- `uv`/`uvx` 是 MCP 或工具启动器可能需要的外部命令，当前没有自动安装流程；
- `openclaw` 是外部 Agent/CLI，不是 Flowy 的内置安装依赖。

所以第一阶段应让 Skill 下载完全走 Rust 后端 HTTP 客户端；如果某个 Skill 的脚本之后需要 Bun、Python、uv 或其他工具，应由执行前的依赖诊断和产品提示处理，而不是让市场安装流程偷偷执行任意包管理器命令。

## 5. 分阶段建议

### Phase 1：SkillHub 单 Skill

- 把专家包中的 `download_skillhub_skill_zip`、安全解压、manifest 校验和原子提交抽成普通 Skill 安装路径；
- 新增明确的后端安装 API，入参只接受已解析的市场来源和安全标识，不接受任意 shell 命令；
- UI 点击 Add 后直接调用安装 API，展示下载、校验、已存在、失败和完成状态；
- 安装完成刷新 `/api/skills`，下一次对话通过现有 materialize 流程可用；
- 对网络错误、404、无效 ZIP、多个 Skill、同名无效目录和中途取消添加测试。

### Phase 2：LoopHub

- 先用 fixture/受控 HTTP 测试确认 `download_url` 的归档格式、内容类型、重定向和命名契约；
- 复用通用下载器，只增加 LoopHub artifact 解析和 host allowlist；
- 以包内 `SKILL.md` 的安全 `name` 作为安装目录名和 catalog identity，不使用展示名；
- 若 LoopHub 只提供需要 CLI 解包的私有格式，则暂不纳入“应用托管下载”，保留手动/命令安装提示。

### Phase 3：SkillHub 专家包

- 重新接通现有 `POST /api/skills/market/package/install`；
- 将“多个 Skill 的下载事务”和“Preset 的提交事务”在结果中明确区分；
- 增加进度、部分失败和回滚结果展示；
- 首期维持顺序下载，待有真实耗时数据后再评估受控并行。

### Phase 4：ClawHub（重新接入条件）

- 先确认 ClawHub 官方稳定的归档/API/export 契约，并将其加入 allowlist；在此之前从普通 Skill 市场 source 列表移除；
- 只有拿到可验证的 Skill artifact 后，才能接入通用安装器；
- 在此之前，市场可以继续展示“打开来源/复制命令”，但不应宣称 Flowy 已托管安装，也不应强行要求用户安装 `openclaw`。

MCP 与 Plugin 另立安装器和存储模型，不纳入以上普通 Skill 的兼容承诺。

## 6. 验收标准

### 普通 Skill 安装

- 全新机器只具备 Flowy 与网络时，SkillHub Skill 可安装到 `<data_dir>/skills/<name>/`；
- 安装过程不调用 `openclaw`、`npx`、`node`、`uv` 或 shell；
- `/api/skills` 能发现新 Skill；
- 新建对话能通过 materialize 使用该 Skill；
- 下载中断、非法归档、路径穿越、符号链接、超大解压、同名冲突不会破坏已有 Skill；
- 失败 staging 可清理，成功后不残留原始 ZIP；
- UI 能区分“已安装/复用”“下载失败”“校验失败”“名称冲突”和“来源暂不支持”。

### 市场覆盖声明

每个来源都必须有独立的 artifact fixture 和契约测试。只有同时满足“稳定 artifact、host allowlist、内容校验、安装结果可回读”
才将其标记为“Flowy 托管安装”；只有页面 URL 或 CLI 命令的来源只能标记为“发现/手动安装”。

## 7. 仍需拍板的风险

1. **来源漂移：** 市场 API、下载 URL、限流和鉴权策略可能变化，必须 fail closed，不将页面 HTML 当作 Skill 包。
2. **名称冲突：** 当前 catalog 和 materialize 以 Skill name 为主要执行标识，跨市场同名 Skill 需要明确拒绝、替换或来源限定策略。
3. **供应链：** 下载成功不代表内容可信；应保留来源、版本/修订、摘要和校验结果，必要时增加签名或人工审核状态。
4. **执行依赖：** Skill 目录安装成功不等于其中脚本依赖已满足；依赖诊断应在执行边界单独处理。
5. **Windows 文件系统：** 符号链接权限、杀毒软件锁文件和 rename 失败需要通过可重试的本地错误呈现，不能回退到不安全覆盖。

## 8. 解耦落地方案

### 8.1 解耦结论

需要拆分，但只拆分已经被真实差异证明的职责，不做“大一统市场插件系统”：

1. **市场发现**与**Skill 安装**必须分开。发现只负责榜单、展示信息和来源链接；安装负责 artifact、校验、落盘和结果。
2. **普通 Skill**、**SkillHub 专家包**、**MCP Server**、**Plugin** 必须分开。它们的安装目标、权限和回滚语义不同。
3. **SkillHub** 与 **LoopHub** 之间需要一个内部 Adapter seam。两者都能返回 ZIP，但 SkillHub 可由 slug 预期名称，LoopHub 必须以包内 manifest 名称为准。
4. 共享 HTTP 安全客户端和 Skill 文件系统原语继续复用，不需要复制实现，也不需要每个来源拆成独立 crate。

### 8.2 目标模块关系

```text
SkillMarketSettings / MarketSettingsPanel
        │ 只消费展示 DTO 和安装结果
        ▼
ipcBridge: installMarketSkill(source, id)
        ▼
POST /api/skills/market/skill/install
        ▼
ManagedSkillInstaller                 ← 深模块：一个小接口隐藏完整事务
   ├── ManagedSkillSourceAdapter       ← SkillHub / LoopHub
   ├── MarketHttpClient                ← host、redirect、timeout、size
   └── Skill filesystem primitives     ← safe extract / validate / commit
        ▼
<data_dir>/skills/<manifest-name>/
        ▼
existing catalog + materialize + agent workspace links
```

当前文件建议按以下方式演进：

| 文件/模块 | 调整 | 保留的职责 |
| --- | --- | --- |
| `market/mod.rs` | 收敛为来源选择和发现入口，不再承载安装细节 | market source 编排、ranking 请求、错误聚合 |
| `market/parse.rs` | 保留发现响应解析；不生成托管安装的 shell 权威信息 | 将外部响应转成展示 DTO |
| `market/client.rs` | 扩展为 artifact 读取能力，统一大小、超时和重定向 | 所有市场 HTTP 请求的安全策略 |
| `market/managed_skill.rs`（新增） | 放置 `ManagedSkillSourceAdapter`、SkillHub/LoopHub artifact 解析 | 来源差异只停留在 Adapter 内 |
| `market/install.rs`（新增） | 放置 `ManagedSkillInstaller` 和单 Skill 事务 | 下载、staging、manifest 校验、原子提交、结果 |
| `market/package.rs` | 改为调用 `install.rs` 安装子 Skill | 专家包解析、包级事务、Preset 提交/回滚 |
| `market/mcp.rs` | 不与普通 Skill 合并 | MCP 配置解析 |
| `skill_service.rs` | 暂不迁移 | 用户 Skill 根目录、safe ZIP、校验、commit、catalog、materialize |

这是有实际深度的模块拆分：UI 和路由只需要一个 `install_market_skill` 接口，来源 URL、归档格式、临时目录和回滚细节
隐藏在后端实现中。不要把每个来源的 URL 拼接、命令判断和文件操作继续散落到 React、route handler 和 package code。

### 8.3 后端接口与数据契约

内部安装接口建议保持小而明确：

```text
install_market_skill(
    paths,
    MarketSkillInstallRequest { source, id }
) -> MarketSkillInstallResponse
```

请求只带 `source` 和 `id`，不信任前端提交的 `url`、`name` 或 `install_command`。后端依据 source registry 重新解析来源，防止
缓存条目被篡改后变成任意下载地址。

响应至少区分：

```text
status: created | reused
source
market_id
installed_skill_name
revision: optional
```

失败分类固定为 `source_unsupported`、`artifact_not_found`、`artifact_invalid`、`manifest_invalid`、`name_conflict`、
`local_io`、`cancelled`，避免 UI 只能解析一段 shell 错误文本。

`SkillMarketItemResponse` 建议新增 `resource_kind` 与 `install_mode`：

```text
resource_kind: skill | skill_package | mcp | plugin
install_mode: managed | manual | unsupported
```

这两个字段采用向后兼容的新增字段；现有 `url` 保留用于打开来源，`install_command` 先保留用于兼容旧缓存/复制入口，但托管
Skill 的主按钮和后端安装流程不得再依赖它。后续再将命令字段改成可选，不要一次把旧 DTO 全部删除。

### 8.4 来源注册与筛选策略

后端维护安装能力，而不是让前端猜测：

```text
managed Skill sources:
  skillhub  -> SkillHubApiAdapter
  loophub   -> LoopHubAdapter

discovery/manual only:
  clawhub
  skills_sh fallback

separate resource surfaces:
  skillhub_packages
  skillhub_mcp
  mcpworld
  clawhub_plugins
```

普通 Skill 市场首期只请求并显示 `skillhub`。LoopHub 保留后端解析能力但在独立 artifact 绑定交付完成前从普通市场 UI 隐藏；SkillHub 的
`skills.sh` HTML fallback 已移除，不再伪装成可托管 `skillhub` 条目。

服务端安装接口必须再次拒绝 `clawhub`、MCP、Plugin 和 discovery-only source，即使恶意客户端绕过 UI 直接请求。

### 8.5 安装事务

单 Skill 的完整顺序：

1. 根据 `source + id` 在后端解析 artifact；
2. 校验固定 host、HTTPS、重定向、响应大小和请求超时；
3. 写入 `<data_dir>/skills/.market-import/<operation-id>/`；
4. 安全解压，禁止 Zip Slip、符号链接逃逸和过量解压；
5. 递归扫描限定深度，普通 Skill 必须得到唯一一个 `SKILL.md`；
6. 读取 manifest，校验 `name` 是安全文件名且非空；SkillHub 还要校验它等于 slug；LoopHub 以该 name 为最终目录名；
7. 获取安装锁，检查同名目标：读取来源/版本/摘要元数据后，完全一致才返回 `reused`；缺少元数据、来源不同或摘要不同都返回 `name_conflict`，绝不覆盖；
8. rename 到 `<data_dir>/skills/<manifest-name>/`；
9. 清理 staging；返回结构化结果；
10. 现有 `/api/skills`、catalog 和 materialize 自动复用新目录。

专家包继续由包模块管理“全部子 Skill 成功后提交”和“Preset 失败回滚”；本次只让它与普通安装共享 Skill mutation lock，保持现有包级事务
和复用语义不变。单 Skill installer 的内部原语可在后续专家包独立交付中复用，本次不改变专家包 UI 或事务边界。

### 8.6 UI 改造

`MarketSettingsPanel` 已经有通用的 pending/completed/error action 状态，不需要重写。只改 Skill 市场页面的 action：

- `SKILL_MARKET_SOURCES` 改成 `['skillhub']`，默认源为 `skillhub`；LoopHub 待独立 artifact 绑定交付后再加入；
- `SkillMarketSettings` 移除 `useNomiQuickStart` 安装草稿逻辑，主按钮直接调用 `installMarketSkill`；
- 安装中显示“下载/校验中”，成功显示“已安装”，失败显示结构化错误；
- 成功后刷新安装记录和 `AVAILABLE_SKILLS_SWR_KEY`，状态按 `source + market_id` 判断，不按展示名猜测；
- Managed Skill 隐藏“复制安装命令”，保留“打开来源”；
- managed Skill 不走安装命令；其他 manual 条目的既有命令安全校验仍保留在各自入口；
- MCP/Plugin 若未来接入，使用各自的页面和 action，不复用普通 Skill 的安装按钮。

### 8.7 分阶段提交

| 阶段 | 内容 | 主要验证 |
| --- | --- | --- |
| M0 | source policy：普通市场只保留 SkillHub API；LoopHub 暂时隐藏，普通 ClawHub 与 `skills.sh` fallback 移除 | source 列表、空 source 行为、缓存过滤测试 |
| M1 | 新增 SkillHub managed install；复用现有 safe extract/validate/commit | SkillHub fixture + 本地 HTTP server + `cargo test -p nomifun-extension` |
| M2 | 独立交付 LoopHub Adapter；manifest name 驱动；处理 `display name != manifest name`，完成后再开放 UI | 多个 LoopHub fixture、重复名、无效 manifest、live ignored smoke |
| M3 | UI 主按钮切换为托管安装，刷新 catalog，删除普通 Skill 的命令草稿路径 | `skillMarket`、页面结构测试、`bun run typecheck`、`bun run check` |
| M4 | 专家包复用通用安装原语，补充子 Skill 进度与包级回滚展示 | package 单元/集成测试、失败注入、前端回滚状态 |
| M5 | 版本/更新、来源元数据、签名或审核状态 | 仅在真实更新需求出现后设计，不提前引入数据库迁移 |

### 8.8 明确不做

- 不在安装过程中执行任意 shell、`npx`、`openclaw`、`uv` 或包管理器；
- 不把 MCP/Plugin 伪装成 Skill 目录；
- 不为了兼容 ClawHub 保留普通 Skill 市场中的不可执行条目；
- 不把每个市场拆成独立 crate；
- 不在第一阶段引入下载任务数据库、断点续传、自动更新和复杂来源插件系统；
- 不改变 Bun 是否随发行包嵌入的运行时策略；那是 Skill 执行依赖问题，不是 Skill 下载问题。

## 9. 边界审查与补充约束

当前拆分方向正确，但原方案仍有几处会造成“市场显示可安装、接口返回成功，最后对话却没有使用到正确 Skill”的边界。以下约束应在 M0/M1 进入实现契约，而不是留到后续优化。

### 9.1 首期阻塞项

1. **可托管性必须落实到条目级，不是只看 source。** 后端 DTO 明确 `resource_kind`、`install_mode`，托管安装接口再次按条目能力 fail closed。旧缓存中没有这两个字段的条目不进入 v5 缓存，不能默认托管。
2. **LoopHub 的 artifact 身份不能只靠排行榜 ID。** 当前排行榜给了 `id` 和 `download_url`，但安装请求若只有 `source + id`，服务端必须能够重新解析当前详情并验证下载 URL；不能让前端任意提交 URL，也不能假设数字 ID 可以拼出下载地址。建议首期由服务端按 ID 重新拉取详情并校验 `dl.cocoloop.cn`，必要时把版本/修订一起绑定到安装操作。
3. **同名复用必须有来源和摘要依据。** 当前 `commit_market_skill_directory` 的“已有有效目录即 `Reused`”适用于幂等导入，但不适用于市场内容：用户手工安装的同名目录可能内容完全不同。首期宁可返回 `name_conflict`，也不能把未知来源目录误报为当前市场条目已安装；成功安装至少需要记录 `source`、`source_id`、版本/修订（若有）和 SHA-256。
4. **安装锁必须覆盖所有 Skill 目录变更。** 现有锁只在 `market/package.rs` 的包提交阶段生效，普通市场安装、Import、删除和 workspace projection 可能并发操作同一目录。应抽出 `SkillMutationLock`，至少协调 managed install、package commit、现有 import/delete 和 projection；锁只能降低进程内竞争，仍必须保留原子 rename、目标存在检查和失败后的再次校验。
5. **“安装成功”必须包含 workspace projection 验证。** `link_workspace_skills` 当前对已存在目标采用 first-write-wins；若工作区已有同名用户目录或旧的 fallback copy，新的 canonical Skill 安装后可能不会进入对话实际使用的目录。需要区分 Flowy 自己管理的 link/junction/copy 与用户拥有的目标：只允许替换前者，对后者返回 `shadowed`/冲突提示，并在测试中从安装到新会话 materialize 跑完整链路。

### 9.2 M0/M1 必须补齐的工程边界

- **来源级 HTTP 策略要进一步收窄。** 当前市场 HTTP client 的 allowlist 是跨来源共享的；managed artifact 下载应使用 source-scoped host 和重定向策略，例如 SkillHub API 加其明确 COS bucket、LoopHub 仅允许 `dl.cocoloop.cn`。发现页面可以有另一套策略，但不能把发现 client 当作安装 client。
- **下载应流式写入 staging。** 当前通用读取函数会把响应完整放进内存；首期应采用“写临时文件 + 上限计数”，并同时限制单操作总下载量、ZIP 条目数、累计未压缩大小和单个归档大小。现有 `zip_safe` 已覆盖条目数、累计解压大小、路径穿越和 ZIP 符号链接，但当前实现是在一个 entry 完整 `io::copy` 后才记账，不能把单个超大 entry 当作严格的即时硬上限；安装器应复用其规则，并将读取改为边读边限流，同时补充 ZIP magic/content-type 校验，而不是重新实现整套安全策略。
- **保留 staging 的崩溃回收。** Drop cleanup 只能覆盖正常 unwind/取消；进程崩溃后仍可能残留 `.market-import`/`.import-tmp`。启动时应只在 `<data_dir>/skills/` 下按固定前缀、年龄阈值和目录类型做 GC，绝不递归清理未知目录。
- **保留目录保留名和根目录语义。** `skills/` 下已有 `companion`、`shared`、`_drafts` 等特殊布局，`.market-import`/`.import-tmp` 是 staging。市场 manifest name 必须禁止命中这些保留名，并且 ZIP 只能提交一个普通顶层 Skill 目录，不能把 staging、嵌套 package 或 workspace 路径写入 catalog。
- **身份在服务端重新确认。** 前端榜单可能是 6 小时缓存，source 条目可能下线、换版本或变更 artifact。安装时必须重新解析 canonical ID/slug、版本和 manifest name；不能把缓存的展示名、URL 或 `install_command` 作为安装依据。
- **幂等、取消和错误需要固定契约。** 双击、多个窗口或专家包并发安装同一 Skill 时应返回同一个幂等结果或 `already_in_progress`，而不是重复下载/覆盖。失败至少区分临时网络、artifact/manifest 不合法、本地名称冲突、权限/磁盘错误和取消；响应和日志不得回显完整响应体、任意 URL 或 Skill 内容。
- **缓存和 fallback 必须同步迁移。** 移除普通 ClawHub 与 `skills.sh` fallback 后，前端缓存 key 升为 `v5` 并不迁移 v4；普通 Skill 页面显式请求 `['skillhub']`，不依赖 `sources=[]` 的“全部 source”语义。

### 9.3 应明确但可以后置的产品边界

- **首期是首次安装，不是更新器。** `reused` 只表示同一 artifact 已存在；不自动覆盖旧版本，也不承诺榜单永远保持最新版。更新需要备份、原子替换、失败回滚和 projection 重建，应单独设计。
- **内容安全不等于归档安全。** 路径和压缩炸弹校验只能保证落盘边界，不能证明 Skill 内容可信。首期不执行 `scripts/`，至少展示来源、版本/修订和 SHA-256；签名、审核状态、许可证策略和恶意内容扫描可作为后续准入能力，但必须保留扩展位置。
- **包安装与 Preset 安装是两个结果。** 专家包的 Skill 子项可以复用 managed installer，但 Preset 的提交失败、Skill 已复用、部分子项回滚不能混成一个“安装成功”布尔值。首期可维持顺序下载，先把包级结果和回滚语义测清楚，再做并行和进度任务化。
- **私有源、鉴权源和第三方依赖不在首期承诺内。** 不能默认使用用户浏览器 cookie、应用凭据或下载包管理器依赖；需要认证、许可确认、外部运行时或安装脚本的条目只能显示为 manual/unsupported。

### 9.4 修订后的首期准入门槛

只有同时满足以下条件，来源/条目才能标记为 `managed`：

```text
稳定的 canonical ID + 服务端可重解析
  -> source-scoped HTTPS/artifact host
  -> ZIP 类型与 magic 校验
  -> Zip Safe 解压限制与唯一 manifest
  -> 来源/版本/摘要可记录
  -> 同名冲突不覆盖
  -> workspace projection 可验证
  -> 可回读 /api/skills，失败可分类、可清理
```

因此最终市场分层应固定为：

```text
managed Skill:    SkillHub API
gated next phase: LoopHub（完成服务端 artifact 绑定后逐条通过准入）
removed ordinary: ClawHub、skills.sh fallback
separate surface: SkillHub package、MCP、Plugin
```

这不是把所有来源永久排除，而是把“不具备稳定 artifact 契约”的来源拒绝纳入托管安装，避免 UI 和后端对第三方 CLI 形成隐式依赖。

## 源码与文档依据

- [Skill 路径、catalog 与 materialize](../../crates/backend/nomifun-extension/src/skill_service.rs)：`SkillPaths`、`list_available_skills`、`materialize_skills_for_agent`、`link_workspace_skills`。
- [市场解析与命令映射](../../crates/backend/nomifun-extension/src/market/parse.rs)：ClawHub、SkillHub、LoopHub、MCP、Plugin 和专家包的来源字段。
- [市场聚合与来源 allowlist](../../crates/backend/nomifun-extension/src/market/mod.rs)：市场 source、ranking URL 和来源分类。
- [SkillHub 专家包安装](../../crates/backend/nomifun-extension/src/market/package.rs)：下载、staging、严格校验、原子提交与回滚。
- [市场 HTTP 安全边界](../../crates/backend/nomifun-extension/src/market/client.rs)：host allowlist、重定向上限、超时和响应大小限制。
- [市场路由](../../crates/backend/nomifun-extension/src/skill_routes.rs)：`/api/skills`、`/api/skills/materialize-for-agent` 与专家包安装 API。
- [前端 IPC](../../ui/src/common/adapter/ipcBridge.ts)：市场目录、Skill 路径、materialize 和专家包安装调用。
- [桌面数据目录](../guides/desktop-app.zh.md)：平台默认 `<data_dir>` 与 `NOMIFUN_DATA_DIR`。
- [运行时与 Bun 打包说明](../architecture/data-and-storage.zh.md)、[安装说明](../getting-started/installation.zh.md)：Bun、Node/npm/npx 和外部运行时的实际边界。

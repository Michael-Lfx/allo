# 设定市场与 SkillHub 专家包可用性研究

> 状态：阶段 1 结论已记录，阶段 2 已完成 `tech-test-automation` 当前接口验证。
>
> 范围：只分析“设定市场”与 SkillHub 专家包，不改变产品代码，不删除本地设定或缓存。

## 当前结论

### 1. “设定”目录不是单一来源

正式设定目录合并三类本地来源：Builtin、User、Extension。当前版本的 Builtin 设定目录为空；User 设定保存在 SQLite，指令和头像使用 NomiFun 数据目录；Extension 设定由已安装扩展的 `presets` contribution 生成。详见[设定来源与编辑规则](guides/presets.zh.md#来源与编辑规则)。

设定市场是另一条发现和导入链路。市场卡片不是本地设定。虽然 SkillHub 原始排行榜条目实际还可能包含完整 `content`、`skillSlugs`、`skills`、`published` 和 `updatedAt`，但当前 Flowy 后端的 `SkillMarketItemResponse` 只投影名称、描述、URL、安装命令、标签和统计信息；原始 Prompt 等字段在列表解析阶段被丢弃。[SkillMarketItemResponse](../crates/backend/nomifun-api-types/src/skill.rs)定义了 Flowy 侧的市场元数据投影。

### 2. 专家包添加后的本地形态

添加 SkillHub 专家包时，当前实现会：

1. 用市场条目的 ID/URL 解析专家包 slug；
2. 请求 SkillHub 专家包详情接口；
3. 读取专家包 Prompt 和子技能 slug；
4. 下载并导入子技能；
5. 把专家包 Prompt 和成功导入的子技能写入一个 `source=user` 的本地设定。

实现入口见[专家包安装流程](../crates/backend/nomifun-extension/src/market/package.rs)。因此，导入成功后本地主要保留的是 Prompt、技能文件和 User Preset 记录，而不是一个独立的专家包对象；当前 UI 已停止提供专家包市场，历史安装结果按本地 User 设定管理。

### 3. 当前市场列表没有逐条详情验证

列表同步只请求 SkillHub 专家包排行榜接口，解析列表元数据；列表解析不会为每个条目再次请求详情接口。详情请求发生在用户点击添加时。[市场同步入口](../crates/backend/nomifun-extension/src/market/mod.rs)和[专家包详情解析](../crates/backend/nomifun-extension/src/market/package.rs)分别负责这两层。

阶段 2 的现场探针发现，排行榜原始响应本身已经携带一部分可用于包级校验的内容。因此，是否需要逐条请求详情，不能只根据“当前 Flowy 卡片没有这些字段”来判断；应先比较原始列表条目与详情条目的字段一致性，再决定是否只校验异常项或完全复用列表内容。

前端市场列表还会使用 6 小时缓存；当前同步失败或没有有效新条目时，存在保留旧缓存的逻辑。[useMarketCatalog](../ui/src/renderer/pages/settings/skill/useMarketCatalog.ts)负责缓存读取、TTL 判断和同步结果落盘。

### 4. 拟采用的缓存语义

市场列表缓存与单个专家包健康缓存应分开：

| 数据 | 建议策略 |
| --- | --- |
| 市场排行榜列表 | 6 小时缓存 |
| 新条目或健康缓存已过期的专家包 | 重新验证详情 |
| 详情明确 404、内容缺失或 slug 不匹配 | 隐藏 10 天，记录负向缓存 |
| 超时、5xx、DNS/TLS 或其他网络错误 | 不判定为下架；保留已知可用项并标记暂时无法验证 |
| 排行榜中暂时消失 | 从当前市场结果移除，但不直接判定为下架 |

“缓存过期”只表示需要重新验证，不应本身被当成专家包失效。验证通过的条目可继续显示；只有明确的资源不存在或内容非法才进入长期隐藏状态。

## 阶段 2 现场验证：`tech-test-automation`

验证时间：2026-09-03，使用当前 `main` 的 Rust 网络客户端直接读取 SkillHub API；探针只读取响应，不安装技能、不写入用户数据。

官方接口：

- [SkillHub 专家包排行榜 API](https://api.skillhub.cn/api/v1/skillsets?page=1&pageSize=200)
- [SkillHub `tech-test-automation` 详情 API](https://api.skillhub.cn/api/v1/skillsets/tech-test-automation)

实际响应摘要：

| 项目 | 排行榜条目 | 详情条目 |
| --- | --- | --- |
| HTTP 状态 | `200 OK` | `200 OK` |
| 响应大小 | `410,118` bytes | `9,613` bytes |
| 列表总数 | `skillSets=59` | — |
| 是否包含目标包 | 是 | — |
| `slug` | `tech-test-automation` | `tech-test-automation` |
| 展示名 | `自动化测试` | `自动化测试` |
| `published` | `1` | `1` |
| 版本 | Prompt frontmatter 为 `1.0.0` | Prompt frontmatter 为 `1.0.0` |
| `skillSlugs` | 6 个 | 同样 6 个 |
| 子技能 | `superpowers-tdd`、`test-case-generator`、`test-patterns`、`e2e-testing-patterns`、`api-test-automation`、`afrexai-qa-test-plan` | 同样 6 个 |
| 中文 `content` | 有，约 2,048 字符 | 有，约 2,048 字符 |
| 英文 `contentEn` | 有，约 4,211 字符 | 有，约 4,211 字符 |
| `skills` | 有 6 项，并带部分命名空间 | 有 6 项，并带部分命名空间 |
| `updatedAt` | `1785123874237` | `1785123874237` |

该专家包的 Prompt 是一个 YAML frontmatter + Markdown 工作流文档。frontmatter 中明确声明：

- `package_type: meta-skill`；
- `version: 1.0.0`；
- `display_name: 自动化测试`；
- `orchestration.children` 为上述 6 个子技能。

因此，对当前这个包来说，排行榜响应已经足以做“包级可用性”和“包内容完整性”的初筛，不必为了判断 404 而再请求一次详情。详情接口仍可作为添加阶段的权威复核，因为列表和详情在远程服务上可能发生短暂不一致。

但这组数据也暴露出一个边界：`skillSlugs` 只证明专家包声明了 6 个子技能，不证明 6 个子技能的 ZIP 下载都正常。要验证“点击添加一定成功”，仍需对子技能下载进行单独检查；这不适合在列表加载阶段执行。

随后按详情返回的 6 个 slug 直接检查下载接口，未导入 ZIP，结果如下：

| 子技能 | HTTP 状态 | ZIP 大小 |
| --- | --- | ---: |
| `superpowers-tdd` | `200 OK` | 2,956 bytes |
| `test-case-generator` | `200 OK` | 8,104 bytes |
| `test-patterns` | `200 OK` | 5,923 bytes |
| `e2e-testing-patterns` | `200 OK` | 6,681 bytes |
| `api-test-automation` | `200 OK` | 25,405 bytes |
| `afrexai-qa-test-plan` | `200 OK` | 2,920 bytes |

这说明在本次验证时，“自动化测试”包本身、详情内容和 6 个子技能下载均可用。若用户仍然看到“专家包不可用”，更可能是旧缓存、不同时间点的上游状态、用户本地网络，或其他专家包的子技能失败，而不是当前这个 slug 的整体接口必然失效。

### 2.1 失败子技能的隔离导入复现

随后对上述 6 个 ZIP 逐个执行了与 Flowy 相同的 ZIP 解压和 Skill 导入路径，但写入的是临时目录，不触碰用户的技能目录。5 个 ZIP 可以完成导入；`afrexai-qa-test-plan` 的下载仍然是 `200 OK`、`application/zip`、2,920 bytes，但导入失败：

```text
Invalid skill path: No valid frontmatter in ...\\afrexai-qa-test-plan\\SKILL.md
```

该 ZIP 的 `SKILL.md` 以 `# QA Test Plan Generator` 开头，没有 Flowy 导入器要求的 YAML frontmatter（至少需要 `---`、`name:`、`description:` 和结束的 `---`）。因此这不是“下载失败”，而是 SkillHub 返回了可下载但不符合 Flowy Skill 导入契约的 Skill。当前“自动化测试”包实际结果是 6 个声明子技能中 5 个可导入、1 个不可导入，属于不完整专家包。

该 ZIP 的 `_meta.json` 只包含 `ownerId`、`slug`、`version` 和 `publishedAt`，没有可直接补全 Flowy `name`/`description` 的字段；因此不能仅靠现有元数据安全地自动修复，优先应由 SkillHub 重新发布带合法 frontmatter 的 Skill。

这也解释了用户界面的状态错觉：后端响应模型同时返回 `installed_skill_names` 和 `errors`，前端在有成功子技能时仍继续创建并启用 User Preset，所以卡片会显示“已添加”，但该 Preset 只绑定了成功导入的 5 个 Skill。

## 阶段 2 待验证问题

仍需要读取更多当前 SkillHub 数据后再确定：

- 其他专家包的排行榜字段是否同样包含完整 `content`，还是只有部分条目包含；
- 排行榜中的 `published` 是否可以作为稳定的下架判断字段，还是只能作为辅助信号；
- 其他专家包的 `skillSlugs` 与详情返回的子技能列表是否一致；
- 详情返回哪些字段，Prompt 是否含 YAML frontmatter；
- 其他专家包详情接口返回的子技能是否都能下载并通过 Flowy 导入校验；
- 哪些响应可以可靠地归类为“下架/资源非法”，哪些只能归类为“网络暂时不可用”；
- 当前列表接口是否可以作为稳定的包级健康来源，能否避免逐条请求完整详情。

## 实施前的边界

即使列表阶段验证通过，添加时仍需保留最终校验，因为远程资源可能在验证和下载之间发生变化。列表阶段只验证专家包详情，不应下载所有子技能 ZIP；否则请求量会变成专家包数乘以子技能数，并增加首次市场加载延迟。

如果列表响应已包含完整包内容，优先应由后端在解析列表时完成结构校验，并把经过校验的包摘要/版本信息传给前端；不能直接信任前端传回的完整 Prompt 或子技能列表。添加时仍应由后端按 slug 复核详情，并负责子技能安装。

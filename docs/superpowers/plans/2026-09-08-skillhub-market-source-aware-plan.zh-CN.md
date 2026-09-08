# SkillHub 市场来源、语言默认与安装映射整合执行计划

Date: 2026-09-08
Status: 只读核查完成，待审查后实施
Scope: 普通 Skill 市场；不改变 MCP、专家包、插件和独立 VIMAX Skill Hub 能力

## 1. 结论

本项目可以支持“按当前界面语言默认选择 SkillHub 或 ClawHub 来源”，但实现上应保留一个市场服务商：SkillHub。

这里需要区分两个概念：

- **市场服务商**：应用实际请求、解析和下载的服务是 SkillHub。
- **内容来源筛选**：SkillHub 聚合目录中的 ClawHub 内容或 SkillHub 本地内容。

因此不恢复独立的 ClawHub Skill 安装适配器，也不让前端把 ClawHub 当成第二个普通市场。前端只增加 SkillHub 内部的内容来源筛选。

拟定默认规则：

| 当前界面语言 | 默认内容来源 | SkillHub API 请求参数 | 备注 |
| --- | --- | --- | --- |
| `zh-CN` | SkillHub | `source=community` | 按官网筛选项命名；实时响应可能包含 `community` 和 `enterprise` 条目 |
| `en-US` | ClawHub | `source=clawhub` | 实时响应条目的 `source` 为 `clawhub` |
| 用户手动选择 | 用户选择 | 对应来源参数 | 手动选择优先于语言默认值 |

“SkillHub”在官网筛选器中对应 `source=community`，但这不是单个条目 `source` 字段的严格等值映射。这个差异必须在后端和测试中明确保留。

## 2. 来源接口实测结果

本次使用 SkillHub 官方 API 直接请求，验证时间为 2026-09-08。每次请求 `page=1&pageSize=24&sortBy=score&order=desc`，结果如下：

| 请求 | HTTP / 业务结果 | `total` | 当前页 | 当前页观察到的条目 `source` |
| --- | --- | ---: | ---: | --- |
| 不带 `source` | `200 / code=0` | 145798 | 24 | `community, enterprise` |
| `source=community` | `200 / code=0` | 82320 | 24 | `community, enterprise` |
| `source=clawhub` | `200 / code=0` | 63476 | 24 | `clawhub` |
| `source=enterprise` | `200 / code=0` | 33514 | 24 | `enterprise` |
| `source=official` | `200 / code=0` | 0 | 0 | 无 |
| `source=skillhub` | `200 / code=0` | 0 | 0 | 无 |

本次实测请求：

- [全部来源 API](https://api.skillhub.cn/api/skills?page=1&pageSize=24&sortBy=score&order=desc)
- [SkillHub 官网来源筛选对应的 API](https://api.skillhub.cn/api/skills?page=1&pageSize=24&sortBy=score&order=desc&source=community)
- [ClawHub 来源 API](https://api.skillhub.cn/api/skills?page=1&pageSize=24&sortBy=score&order=desc&source=clawhub)
- [SkillHub 官网 ClawHub 筛选页](https://skillhub.cn/skills?sortBy=score&source=clawhub)
- [SkillHub 官网本地来源筛选页](https://skillhub.cn/skills?sortBy=score&source=community)

实测结论：

1. `source` 参数确实被当前 API 接受，`source=clawhub` 不是只有官网前端才认识的参数。
2. `source=clawhub` 返回 ClawHub 内容，当前页所有条目的 `source` 都是 `clawhub`。
3. 官网标注为 SkillHub 的 `source=community` 是一个本地内容分组，当前页同时出现 `community` 和 `enterprise`。不能再把它客户端过滤成 `source === community`，否则会误删企业发布内容。
4. 不带来源的结果总数与两个主要筛选结果之和不完全相同，说明来源值不是只有 `community` 和 `clawhub` 两种，必须保留未知来源兼容能力。
5. 总数和排序结果会随线上目录变化；这些数字只作为本次接口证据，不作为容量承诺或 SkillHub SLA。

官方文档当前公开说明 `source` 是列表筛选参数，常见值为 `community` 和 `official`，并且明确说明来源值可能扩展；列表项和详情均包含 `source` 字段。因此代码应把条目来源按字符串保留，不能把官方文档中的示例值当成封闭枚举。[SkillHub Skills API 文档](https://github.com/Tencent/skillhub/blob/main/docs/api/skills.md)

SkillHub 官方仓库同时说明平台会同步全球 Skills（包括 ClawHub），也支持本地作者和企业发布。这支持“SkillHub 是统一入口、ClawHub 是聚合内容来源”的产品模型。[Tencent/skillhub](https://github.com/Tencent/skillhub)

## 3. 当前代码核查结果

当前分支为 `fix/skillhub-market-skill-install`，工作区已有 v3 相关未提交修改，本次只新增本文档，不覆盖或清理现有修改。

已确认的代码缺口：

- 普通 Skill 市场列表目前固定为 `['skillhub']`；`clawhub_plugins` 属于独立插件市场，不是普通 Skill 来源。[skillMarket.ts](../../../ui/src/renderer/pages/settings/skill/skillMarket.ts)
- `SkillHubMarketQueryRequest` 没有来源字段，后端构造 `/api/skills` URL 时没有转发 `source`。[skillhub.rs](../../../crates/backend/nomifun-extension/src/market/skillhub.rs)
- `SkillHubMarketItem` 没有保留上游条目的 `source`，前端无法显示或诊断真实来源。[skill.rs](../../../crates/backend/nomifun-api-types/src/skill.rs)
- 查询缓存 key 当前不包含来源条件，增加来源筛选后必须隔离 `all`、SkillHub 和 ClawHub 缓存。[useSkillHubMarket.ts](../../../ui/src/renderer/pages/settings/skill/useSkillHubMarket.ts)
- 当前筛选栏使用 Arco Select，默认宽度规则会覆盖 `min-width`，导致三个控件在宽屏上纵向铺满；当前组件没有 sticky 布局。
- 当前安装状态包含页面内存中的市场 ID 集合和名称/slug 启发式判断，后端在 `reuse_existing_skill` 之前已经下载 ZIP，因此会出现“没有重复目录，但重复下载和重复校验”的问题。
- 本地 Skill Catalog 没有持久化的 SkillHub market ID 与本地声明名映射，市场显示名、slug 和 `SKILL.md` 的声明名不同或页面重新进入时可能再次显示可安装。

## 4. 已锁定的产品和契约决策

### 4.1 单一 SkillHub 入口

普通 Skill 市场只保留 SkillHub 适配器和 SkillHub 下载链路。来源筛选只影响 SkillHub 的列表请求，不改变安装接口的 `source: skillhub` 契约。

普通市场 UI 可以展示：

- 全部来源
- SkillHub
- ClawHub

这里的控件名称建议使用“内容来源”，避免与普通市场服务商混淆。

### 4.2 产品来源枚举与上游参数分离

应用内部使用稳定的产品枚举：

```text
all
skillhub
clawhub
```

后端负责映射到 SkillHub API：

```text
all      -> 不发送 source
skillhub -> source=community
clawhub  -> source=clawhub
```

响应同时保留每个条目的原始 `source` 字符串，用于详情、诊断和未来来源扩展。不能把 `source=community` 响应中的 `enterprise` 条目丢弃。

canonical market ID 继续使用：

```text
skillhub:{owner}/skills/{slug}
```

owner + slug 是身份，内容来源筛选不是身份。相同 slug 的不同 owner 必须视为不同 Skill。

### 4.3 语言默认只负责首次选择

当前项目的有效语言来源是：

1. `localStorage.i18nextLng`
2. 桌面端注入的 `window.__osLocale`
3. 项目默认 `en-US`

Web 模式当前不读取 `navigator.language`。因此“按语言默认来源”应使用最终解析后的 i18n 语言，不应直接读取浏览器或原始 OS 字符串。

来源选择状态分为：

```text
auto
manual
```

行为规则：

- 没有手动选择时，`zh-CN` 自动选 SkillHub，`en-US` 自动选 ClawHub。
- 用户手动选择后，保存手动选择，优先级高于语言推导。
- 用户切换语言时，`manual` 不改变；`auto` 才根据新语言重新选择。
- 来源偏好数据损坏时安全忽略，重新按语言推导。
- 不因为来源请求失败而静默切换到另一个来源。

## 5. 实施计划

### M1：后端来源查询契约

1. 在 `SkillHubMarketQueryRequest` 增加产品层来源字段，例如 `source` 或 `upstream_source`，只接受 `all`、`skillhub`、`clawhub`。
2. 后端将产品层来源映射为 SkillHub API 参数，`all` 不发送 `source`。
3. 保留上游列表项原始 `source` 字符串；未知值不能使整页失败。
4. 列表响应携带已应用的产品层来源，便于前端诊断和缓存校验。
5. 保持 owner + slug 解析和 canonical URL 校验不变。
6. 来源筛选与关键词、分类、API Key、排序、分页组合测试。
7. `source=clawhub` 返回空列表时显示真实空结果；不能回退到全部来源。
8. 上游返回 200 但业务 code 非 0、429、5xx、超时和非法 JSON 继续按现有错误契约处理。

### M2：前端语言默认和缓存隔离

1. 新增统一的来源解析函数，输入最终 i18n 语言，输出默认产品来源。
2. 增加版本化来源偏好存储，区分自动选择和手动选择。
3. 查询 Hook 增加来源状态，来源变化时清空旧页数据并加载第 1 页。
4. v6 查询缓存 key 必须包含：关键词、来源、分类、API Key 条件、排序、页码、页大小。
5. 不读取旧来源缓存；接口成功返回空数组时不复用旧来源数据。
6. 同条件请求失败时可以显示同来源缓存，并明确标记缓存/过期状态。
7. 旧请求不得覆盖新来源请求，继续使用请求序列号或 AbortController。
8. 跨页按 canonical market ID 去重。

### M3：筛选栏和市场展示

1. 将现有三个纵向铺满的 Select 改为紧凑横向工具栏。
2. 增加“内容来源”控件，默认值由语言规则决定，选项为全部来源、SkillHub、ClawHub。
3. 工具栏使用 sticky 布局，保持页面唯一滚动容器；不增加内部滚动区域。
4. sticky 区域使用不透明背景、z-index 和正确顶部偏移，避免覆盖共享技能页头部。
5. 搜索、来源、分类、API Key 和排序在桌面端横向排列，窄屏才换行。
6. 卡片或列表中展示产品来源；必要时在详情中展示原始 `source`，但不把未知来源强行翻译为 SkillHub 或 ClawHub。
7. 继续保留版本、标签、分类、子分类、下载量、安装量、收藏量、评分和更新时间。
8. “热门”“下载量”“最近更新”只映射到 API 已确认支持的排序。除非 SkillHub 提供增长率或趋势字段，不把 `score` 伪装成“近期飙升”。

### M4：安装映射和重复下载修复

不新增数据库迁移。由后端在用户 Skill 数据目录维护原子写入的 market mapping 元数据，记录：

```json
{
  "market_id": "skillhub:owner/skills/slug",
  "source": "skillhub",
  "upstream_source": "clawhub",
  "owner": "owner",
  "slug": "slug",
  "installed_skill_id": "user:实际声明名",
  "version": "1.0.0"
}
```

安装流程调整为：

1. 解析并校验 canonical market ID。
2. 获取精确详情，校验 owner、slug 和版本。
3. 在下载 ZIP 之前查询精确 mapping。
4. mapping 有效且本地 Skill 存在时直接返回 `reused`，不再请求下载接口。
5. mapping 不存在时，才下载并执行 ZIP 魔数、大小、路径穿越、软链接、布局和 `SKILL.md` 校验。
6. 临时目录完成校验后原子提交 Skill 文件。
7. 原子写入 mapping。
8. 返回现有安装响应：`source`、`skill_id`、`skill_name`、`status`；不恢复旧 `id` 字段，也不新增不在 v3 契约中的响应字段。

安装锁需要覆盖“映射检查 → 下载 → 校验 → 提交 → 写入映射”，否则并发点击仍会重复下载。已有旧安装没有精确 mapping 时，只能使用保守的名称/slug 兼容判断，并标记为未确认来源；不能把推测当成精确来源。

本地目录接口增加可选的 market ID 元数据，前端优先按精确 ID 判断已安装，旧安装才使用兼容名称判断。删除、升级和自动启用不纳入本期。

### M5：目录刷新和实际使用

安装成功后：

1. 显示“已安装到技能库，可在预设编辑器中选择”。
2. `reused` 显示“已在技能库”。
3. 刷新 `skills.available`。
4. 广播 `skill-catalog-changed`。
5. `useSkillCatalog` 和 `usePresetEditor` 监听事件并重新加载。
6. 用户主动在预设中选择 Skill 后，发送对话时继续生成不可变技能快照和 SHA-256，并写入 `conversation_skill_loads`。

安装不等于启用，不自动加入预设。

## 6. 边界和失败策略

- **未知来源**：保留原始字符串；不能因为未来新增来源就整页失败。
- **SkillHub 分组变化**：`source=community` 可能继续包含 `enterprise` 或其他本地来源，后端不得按条目字段二次过滤。
- **全部来源**：允许出现 ClawHub、SkillHub 本地、企业和未来来源；“全部”不是只拼接当前两个来源。
- **来源与分页**：来源条件必须进入缓存 key、请求序列和去重逻辑。
- **语言变化**：只影响自动选择，不覆盖用户明确选择。
- **Web 首次语言**：当前浏览器语言不参与 i18n 解析；如果未来要读取浏览器语言，应单独增加验收，不隐式改变本期行为。
- **旧安装**：没有历史 market ID 时无法安全还原真实来源，只做保守兼容，不删除已有 Skill。
- **名称冲突**：同名、同 slug 但不同 owner 不得误判为同一个市场 Skill。
- **版本变化**：本期只识别已安装，不自动升级；mapping 保存版本用于审计和后续升级功能。
- **网络失败**：来源请求失败不能伪装成其他来源数据；缓存兜底必须显示 stale。
- **安全输入**：来源参数、owner、slug、详情、下载地址、ZIP 内容和 `SKILL.md` 全部视为不可信输入。

## 7. 验收标准

### 接口和后端

- 三个基线请求（全部、`community`、`clawhub`）均能通过 stub fixture 和真实网络 smoke test。
- `community` fixture 同时含 `community`、`enterprise` 时，全部保留。
- `clawhub` fixture 只返回 ClawHub 条目时，产品来源正确显示为 ClawHub。
- 未知条目来源保留，不导致全页失败。
- 详情 owner/slug 校验、版本固定、ZIP 安全和并发安装测试通过。
- 第二次安装不再产生新的下载请求。

### 前端

- 中文首次进入默认 SkillHub，英文首次进入默认 ClawHub。
- 手动来源选择在重新进入页面后仍然有效。
- 切换语言不会覆盖手动来源选择。
- 来源条件进入缓存 key，来源切换不会显示另一来源缓存。
- 空结果、stale、error、loading 和 installed 状态彼此区分。
- 筛选栏横向布局正常，滚动后置顶，不覆盖标题，不产生嵌套滚动。
- 不再展示 `skills.sh`、外部 CLI 或已删除普通市场适配器文案。

### 端到端

```text
语言解析
→ 来源默认选择
→ SkillHub 列表请求
→ 来源/分类/API Key/排序/分页
→ 安装
→ 精确 market mapping
→ 本地 Skill Catalog
→ 预设主动选择
→ 对话发送
→ 不可变技能快照和 SHA-256
→ Agent 指令上下文
→ conversation_skill_loads
```

大陆和海外网络可达性需要分别记录 DNS、TLS、HTTP 状态和延迟；可访问性测试不等于 SkillHub SLA 承诺。

## 8. 不纳入本期

- 恢复独立 ClawHub Skill 市场适配器。
- 直接调用 ClawHub API 绕过 SkillHub。
- 新增数据库迁移。
- 自动升级、删除和自动启用 Skill。
- 为“近期飙升”自行伪造趋势数据。
- 修改独立 MCP、专家包、插件和 VIMAX Skill Hub 链路。

## 9. 实施前最终检查

实施时仍需保护当前工作区已有修改，不执行 reset、clean、stash 或无关文件覆盖。完成代码后执行与本项目约定一致的验证：

```text
cargo test -p nomifun-extension
cargo test -p nomifun-conversation
cargo test -p nomifun-api-types
bun test
bun run typecheck
bun run check
cargo fmt --all -- --check
cargo check --workspace
bun run build:ui
```

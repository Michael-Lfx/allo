# SkillHub owner 身份与下载链路修复执行计划

Date: 2026-09-08  
Status: 身份修复、v8 缓存隔离与回归测试已实施；修复后真实链路复验通过（见 2.3）  
Scope: 普通 Skill 市场；不修改 MCP、专家包、插件和独立 VIMAX Skill Hub

关联文档：[SkillHub 市场来源、语言默认与安装映射整合执行计划](2026-09-08-skillhub-market-source-aware-plan.zh-CN.md)

## 1. 目标和结论

本计划处理两类同源问题：

1. 市场外链把企业 Skill 的公开 namespace 错误显示成内部账号 owner。
2. 安装详情校验要求两个不同语义的 owner 字段完全相等，导致有效 Skill 返回 `MARKET_SKILL_NOT_FOUND`。

已确认的正确模型是：

```text
公开路由身份 = namespace.handle（存在且有效时）
账号归属身份 = ownerName / owner.handle
Skill 身份 = 公开路由身份 + slug
```

以 `agently-mail` 为例：

```text
列表 ownerName       = u_d95b6787
列表 namespace.handle = tencent-adm
详情 owner.handle     = u_d95b6787
详情 namespace.handle = tencent-adm
```

因此最终身份必须是：

```text
skillhub:tencent-adm/skills/agently-mail
https://skillhub.cn/skills/tencent-adm/agently-mail
```

## 2. 已完成的真实验证

### 2.1 列表来源验证

2026-09-08 19:57（Asia/Hong_Kong）通过 SkillHub 官方 API 实测：

| 请求 | HTTP | code | total | 当前页 source |
| --- | ---: | ---: | ---: | --- |
| 不带 `source` | 200 | 0 | 145,928 | enterprise 9、community 15 |
| `source=community` | 200 | 0 | 82,452 | enterprise 9、community 15 |
| `source=clawhub` | 200 | 0 | 63,476 | clawhub 24 |

`source=community` 仍然是 SkillHub 的内容分组，不能把返回项二次过滤为
`source === "community"`。`enterprise` 条目必须保留并归类为产品侧 SkillHub。

### 2.2 五个下载链路

测试不写入本地 Skill 目录，只在内存中完成：

```text
列表 → 详情 → latestVersion → /api/v1/download → 302 → 最终 ZIP
→ Content-Type → ZIP 魔数 → 完整字节 SHA-256
```

| 筛选 | canonical owner / slug | source | 详情 | 版本 | 下载 302 | 最终 ZIP | Content-Type | 大小 | 魔数 | SHA-256 前缀 |
| --- | --- | --- | ---: | --- | ---: | ---: | --- | ---: | --- | --- |
| community | `tencent-adm/agently-mail` | enterprise | 200 | 1.0.13 | 302 | 200 | `application/zip` | 5,211 B | `504b0304` | `174bd9f7f4a745c3` |
| community | `tencent-adm/tencent-docs` | enterprise | 200 | 1.0.41 | 302 | 200 | `application/zip` | 378,994 B | `504b0304` | `960e3a78029d087c` |
| community | `user_741dc82b/dev-expert` | community | 200 | 1.0.52 | 302 | 200 | `application/zip` | 345,833 B | `504b0304` | `8c9964c400321cc5` |
| clawhub | `clawhub_pskoett/self-improving-agent` | clawhub | 200 | 3.0.24 | 302 | 200 | `application/zip` | 27,405 B | `504b0304` | `46bebc277672bf29` |
| clawhub | `clawhub_root/find-skills` | clawhub | 200 | 1.0.0 | 302 | 200 | `application/zip` | 10,131 B | `504b0304` | `f6fb4805b7e1b6f7` |

这证明 SkillHub 当前至少存在以下三种详情身份形态：

- 企业/SkillHub：`owner.handle` 与 `namespace.handle` 不同。
- 普通社区：两个字段通常相同。
- ClawHub 聚合：有些条目已有 `namespace.handle`，namespace 缺失的条目才需要 owner 回退。

测试只证明本次请求的可达性和响应形状，不代表大陆或海外 SLA，也不代表所有 Skill 永远可下载。

### 2.3 修复后复验（M6）

身份修复与回归测试落地后，于 2026-09-08 当晚对同一批 5 个样本重新执行了完整链路
（列表 → 详情 → 302 → 最终 ZIP），5/5 通过：

- 列表、详情均 200 且 `code=0`；列表 source 与 §2.1 一致
  （`source=community` 返回 enterprise 条目，ClawHub 条目为 clawhub）。
- 按新规则解析公开身份：`agently-mail`、`tencent-docs` 通过 `namespace.handle`
  解析为 `tencent-adm`；`dev-expert` 与两个 ClawHub 条目按 owner 回退规则解析，全部与预期一致。
- 详情按“namespace 优先、owner 回退”校验通过；版本号与 §2.2 相同
  （1.0.13 / 1.0.41 / 1.0.52 / 3.0.24 / 1.0.0）。
- 下载均为 302 → 最终 200，`Content-Type: application/zip`，ZIP 魔数 `504b0304`，
  字节数与 SHA-256 前缀同 §2.2 完全一致。
- 网络观测（本机出口，单次样本）：DNS 命中缓存约 0s，TLS 握手约 0.09–0.12s，
  列表/详情 TTFB 约 0.25–0.42s，完整 ZIP 下载约 0.46–0.66s。
- 签名下载地址仅在 curl 进程内使用，未写入日志、文档或磁盘文件。

## 3. 当前错误链路

### 3.1 列表到外链

当前 [`market/skillhub.rs`](../../../crates/backend/nomifun-extension/src/market/skillhub.rs)
无条件读取 `ownerName`，再用它构造：

```text
SkillHubMarketItem.owner
skillhub:{owner}/skills/{slug}
https://skillhub.cn/skills/{owner}/{slug}
```

企业 Skill 的 `ownerName` 可能是 `u_xxxxxxxx` 内部账号标识，因此页面链接错误。

### 3.2 安装到错误提示

当前 [`market/skill.rs`](../../../crates/backend/nomifun-extension/src/market/skill.rs)
执行：

```text
列表 ownerName = u_d95b6787
→ 请求详情 /api/v1/skills/agently-mail
→ owner.handle = u_d95b6787       匹配
→ namespace.handle = tencent-adm  不匹配
→ NotFound
→ MARKET_SKILL_NOT_FOUND
```

前端 [`skillMarket.ts`](../../../ui/src/renderer/pages/settings/skill/skillMarket.ts)
将该后端错误翻译成“技能市场条目已不存在”，所以用户看到的是误导性提示。

### 3.3 测试盲区

当前详情测试 helper 同时写入：

```json
{
  "owner": { "handle": "owner" },
  "namespace": { "handle": "owner" }
}
```

它没有覆盖真实企业形态 `owner.handle != namespace.handle`，所以现有测试会通过但无法发现本问题。

## 4. 目标契约

### 4.1 列表规范化

新增内部身份解析函数，规则固定为：

```text
namespace 是对象且 handle 合法
  → public_owner = namespace.handle
namespace 缺失或为 null
  → public_owner = ownerName
namespace 存在但 handle 缺失/非法
  → 丢弃条目并记录 dropped 计数
```

要求：

- `public_owner` 通过现有 market slug 校验。
- `owner` DTO 字段表示 `public_owner`。
- `ownerName` 仅作为账号归属诊断信息，不进入普通 DTO 或用户界面，不能复用 `owner`。
- `id` 和 `url` 只能由 `public_owner + slug` 构造。
- 不信任 `homepage` 改写身份；它只可用于诊断比对。
- 相同 `slug`、不同 `public_owner` 永远是两个不同 Skill。

### 4.2 详情规范化

详情校验应使用“公开 namespace 优先、owner 回退”的规则：

```text
skill.slug 必须等于请求 slug
owner.handle 必须存在且格式合法

namespace 是对象
  → resolved_public_owner = namespace.handle
  → 必须等于请求 owner

namespace 缺失或为 null
  → resolved_public_owner = owner.handle
  → 必须等于请求 owner
```

重要约束：

- `owner.handle` 与 `namespace.handle` 都是有效身份，但语义不同，不能互相强制相等。
- namespace 对象存在但 handle 缺失或非法时 fail-closed，返回 `NotFound`。
- slug、公开 owner 或必要版本缺失时，不进入下载。
- 下载接口只使用已校验的 `slug + latestVersion.version`，不缓存带签名的对象存储地址。

### 4.3 安装 mapping

mapping 的 key 必须使用修正后的 canonical ID：

```json
{
  "skillhub:tencent-adm/skills/agently-mail": {
    "source": "skillhub",
    "market_source": "skillhub",
    "owner": "tencent-adm",
    "slug": "agently-mail",
    "installed_skill_id": "user:实际声明名",
    "version": "1.0.13",
    "installed_at": 1788868290000
  }
}
```

本轮采用严格切换策略：旧版本写入的 `skillhub:u_d95b6787/skills/agently-mail` 不自动迁移，
旧 mapping 不参与新的精确匹配；已存在的本地 Skill 文件不删除，必要时重新安装并生成新的 canonical mapping。

## 5. 代码实施步骤

### M0：基线和冻结

1. 确认当前分支 `fix/skillhub-market-skill-install` 和工作区状态。
2. 保留已完成的 5 个 SkillHub 提交，不执行 reset、clean、stash 或无关覆盖。
3. 记录现有 v7 市场缓存行为和 mapping 文件格式；修复后缓存版本切换为 v8。
4. 把真实 `agently-mail` 列表/详情 JSON 固定为脱敏 fixture，不把签名下载 URL 写入仓库。

### M1：列表身份修复

涉及：

- `crates/backend/nomifun-extension/src/market/skillhub.rs`
- `crates/backend/nomifun-api-types/src/skill.rs`

实施：

1. 新增 namespace 优先的规范化 helper。
2. `parse_skillhub_item` 改用规范化 public owner。
3. 对 namespace 存在但无有效 handle 的条目丢弃并记录计数。
4. 继续保留原始 `source`，不改变 `community` 包含 `enterprise` 的分组规则。
5. 从 public owner 构造 canonical ID 和 SkillHub URL。
6. 给相同 slug 的不同 owner 加唯一性测试。

### M2：详情身份修复

涉及：

- `crates/backend/nomifun-extension/src/market/skill.rs`
- `crates/backend/nomifun-extension/src/skill_routes.rs`

实施：

1. 把 `parse_skill_detail` 的 owner 校验改为 namespace 优先、owner 回退。
2. 删除“namespace 必须与 owner.handle 相同”的错误约束。
3. 保留 slug、版本、格式和 fail-closed 校验。
4. 详情身份失败仍映射为 `MARKET_SKILL_NOT_FOUND`，但只有真正不存在或不匹配时才触发。
5. 确认下载请求仍固定 `slug + version`，不从详情返回的 URL 直接跳转。

### M3：缓存和前端契约

涉及：

- `ui/src/renderer/pages/settings/skill/useSkillHubMarket.ts`
- `ui/src/renderer/pages/settings/skill/marketViewModel.ts`
- `ui/src/renderer/pages/settings/SkillHubMarketPanel.tsx`

实施：

1. 市场缓存已由 v7 升级为 v8，旧 v7 不读取，清除旧错误 URL 的影响。
2. `isSkillHubMarketItem` 继续校验 `id === skillhub:{owner}/skills/{slug}` 和 canonical URL。
3. 外链只使用后端修正后的 `item.url`，前端不增加 `u_` 替换规则。
4. 详情抽屉和列表均展示 canonical owner；账号 owner 仅供内部诊断，不新增到普通 UI。
5. 安装成功后继续广播 `skill-catalog-changed`，不改变安装不等于启用的规则。

### M4：mapping 兼容

涉及：

- `crates/backend/nomifun-extension/src/market/skill.rs`
- `crates/backend/nomifun-extension/src/skill_service.rs`

实施：

1. 新安装只写 canonical namespace ID。
2. mapping 查询必须在详情与下载之前完成。
3. 有效 canonical mapping 且本地 Skill 存在时直接返回 `reused`，不请求任何上游接口（评审修订：本地复用不依赖上游详情/下载可用性；precise mapping 仅在需要真正下载时执行）。
4. 旧 owner alias 不自动迁移；只按新的 canonical ID 精确匹配 mapping。
5. mapping 损坏、同 key 冲突或本地 Skill 缺失时 fail-closed 或走安全重装，不覆盖已有记录。
6. 同 slug 不同 owner 不得共用 mapping。

### M5：回归测试

后端测试必须增加：

1. 企业列表 fixture：`ownerName=u_d95b6787`、`namespace.handle=tencent-adm`，断言 ID 和 URL 使用 `tencent-adm`。
2. 企业详情 fixture：`owner.handle` 与 `namespace.handle` 不同，断言详情校验成功。
3. 错误 namespace、错误 slug、缺失 owner、缺失 namespace handle 断言 fail-closed。
4. namespace 缺失的 ClawHub fixture 使用 `owner.handle` 成功。
5. `community` 返回 `enterprise` 不被二次过滤。
6. HTTP stub 断言详情请求后才下载，并且下载只带 slug/version。
7. 第一次安装写入 canonical mapping，第二次安装下载器调用次数仍为 1。
8. 旧 alias mapping 不迁移；旧错误 ID 不得阻止新 canonical ID 重新安装。

前端测试必须增加：

1. v7 缓存不读取，v8 缓存可读取。
2. 错误 owner URL 不会从缓存进入页面。
3. `agently-mail` canonical URL 为 `tencent-adm`。
4. 安装错误只在后端确实返回 NotFound 时展示“条目不存在”。

### M6：真实网络复验

修复后重复执行 5 个样本，验收每条均满足：

```text
列表 200 / code=0
详情 200
slug 与 canonical owner 匹配
下载响应 302
最终响应 200
Content-Type 为 application/zip 或 application/octet-stream
前四字节为 PK ZIP 魔数
```

至少额外检查：

- `agently-mail`：验证此前失败的企业双 owner 情形。
- 一个 enterprise、一个 community：验证同一 `source=community` 分组。
- 两个 ClawHub：验证 namespace 存在和缺失两种形态。
- 记录 DNS、TLS、HTTP 状态、首字节/完整下载延迟、大小和失败原因。
- 签名 URL 只在进程内使用，不写日志、不落盘、不进入 mapping。

### M7：完整验证和交付

按项目验证阶梯执行：

```text
cargo test -p nomifun-extension
cargo test -p nomifun-api-types
bun test ui/src/renderer/pages/settings/skill
bun run typecheck
bun run check
cargo fmt --all -- --check
cargo check --workspace
bun run build:ui
```

另外执行：

- `git diff --check`
- 两个文档链接和外部 API 链接检查
- 1440×900、1280×800、768×900、390×844 浏览器验收
- 市场列表 → 安装 → Catalog → 预设主动选择 → 对话发送链路验收

## 6. 验收标准

修复完成必须同时满足：

- `agently-mail` 页面外链为 `https://skillhub.cn/skills/tencent-adm/agently-mail`。
- `agently-mail` 安装不再因 `owner.handle != namespace.handle` 返回 NotFound。
- 企业、社区和 ClawHub 三类样本均可完成详情和 ZIP 下载。
- 列表、详情、下载和 mapping 使用同一个 canonical owner + slug 身份。
- 旧错误 v7 缓存不会继续展示错误外链。
- 有效 mapping 第二次安装不产生第二次下载。
- 失败详情不会创建空目录、半成品目录或错误 mapping。
- 不恢复独立 ClawHub Skill 安装适配器，不调用 ClawHub API 绕过 SkillHub。

## 7. 不做和剩余风险

不做：

- 不用前端字符串替换修复 owner。
- 不把 `owner.handle` 和 `namespace.handle` 强行合并成一个字段。
- 不信任上游 `homepage`、签名下载地址或未经校验的 ZIP 内容。
- 不通过名称猜测两个不同 namespace 的 Skill 相同。
- 不新增数据库迁移，不自动升级、删除或启用 Skill。

剩余风险：

- SkillHub 详情接口按 slug 请求，若未来出现同 slug、多 namespace 且接口不支持 owner 参数，仍需依靠详情返回的 namespace 做 fail-closed 校验。
- 线上目录、版本、下载地址和总数会变化；本次样本验证不能替代持续监控。
- 当前网络验证来自本机出口，不能代表所有中国大陆和海外网络环境。
- 浏览器多分辨率（1440×900、1280×800、768×900、390×844）与
  “市场列表 → 安装 → Catalog → 预设 → 对话快照”链路为人工验收项，尚未在本轮自动化中覆盖。
- `bun run typecheck` 在本分支基线上已存在 83 个与本修复无关的错误
  （videoCanvas / videoGeneration / Markdown 区域，主干继承），
  本修复涉及的文件不产生新增类型错误。

# ViMax Skill Hub API 需求文档（Go + MySQL）

> 供云端服务端实现「垂直 Skill」的创建元数据托管、社区发布、广场浏览与安装拉取。  
> 客户端本地已具备：Skill 挂载、本机创建 / 导入、本机 Hub（创建表单对齐 LibTV）。  
> **本文定义云端契约**；风格对齐 [TV Show API](./vimax-tv-show-api.md) 与 [LibTV 创建 Skill](https://www.liblib.tv/skill/create) 的「导演级可发布 Skill」产品形态。  
> 上传复用已有 OSS 预签名：`POST /api/v1/uploads/oss/presignPut`。

---

## 1. 背景与目标

### 1.1 产品分层（客户端已落地）

| 概念 | 含义 | 是否上云 |
|------|------|----------|
| **Mode** | `idea2video` / `script2video` / `novel2video` 输入范式 | 否（客户端管线） |
| **Vertical Skill** | 导演方法论：叙事 overlay + 视觉 overlay + playbook | **是**（可发布到社区） |
| **Template** | Mode + Skill + 预填槽位（后续） | 可选二期 |

一句话：**Mode 决定从什么原料开工；Skill 决定按哪位导演的规矩拍完。**

### 1.2 本期目标

1. 用户可将本地 Skill 包（`SKILL.md` + 可选资源）**发布到云端 Skill Hub**。  
2. 其他用户可在广场 **浏览 / 搜索 / 安装**（下载包到本机 `user` 目录）。  
3. 作者可管理「我的发布」：更新版本、下架、删除。  
4. 运营可审核（与 TV Show 同态：`pending → published`）。

### 1.3 非目标（本期不做）

- 不在云端执行 ViMax 管线 / LLM。  
- 不做 Skill 内付费分成结算（可预留字段）。  
- 不替代客户端内置官方 Skill（`builtin:*` 仍随客户端发版）。

---

## 2. Skill 包格式（与客户端 / LibTV 创建规范一致）

> 产品创建页对标：[LibTV 创建 Skill](https://www.liblib.tv/skill/create)  
> 广场分类对标：[LibTV Skill](https://www.liblib.tv/skill)（短漫剧 / 电影 / 商业广告 / 创意·社媒玩法 / 音乐 MV）

每个 Skill 是一个目录，至少包含 `SKILL.md`：

```markdown
---
name: luxury-tvc
display-name: 高奢版TVC
description: 简短描述该 Skill 的能力（一句话介绍）
category: advertising
version: "1.0.0"
tags: [tvc, luxury]
use-scenario: |
  品牌 / 产品需要高级成片质感的广告短片时使用。
how-to-use: |
  用户提供产品卖点或一句话想法；可选参考图与时长。
output: |
  15–45s 高奢 TVC 成片，附分镜与脚本。
cover-url: https://cdn.example.com/covers/luxury-tvc.jpg
case-url: https://www.liblib.tv/canvas/share?shareId=...
visibility: private
requirement-overlay: |
  Direct this as a high-end luxury TVC…
style-overlay: |
  luxury commercial cinematography…
---

## 做什么
（一句话说明用途）例：把一句话故事想法做成一条短漫剧成片

## 需要什么输入
（最少提供什么）例：一句话想法，可选画风、时长、主角设定

## 怎么做
（写你在意的环节和要求，不用写全）例：脚本要反转多，画风固定成韩漫

## 产出什么
（最终交付什么）例：成片，附脚本和分镜

## 什么时候问你
（什么情况下停下来问你）例：拿不准题材或风格时问一次，其余自己定
```

### 2.1 创建表单字段（客户端）

| UI 字段 | Frontmatter / API | 必填 | 说明 |
|---------|-------------------|------|------|
| Skill 名称 | `display-name` / `displayName` | 是 | ≤64；客户端自动推导 kebab-case `name` |
| 一句话介绍 | `description` | 是 | ≤500；广场列表摘要 |
| 上传封面（选填） | `cover-url` / `coverUrl` | 否 | 本地创建支持选图上传（写入包内）；发布广场建议横版 |
| Skill 内容 | 正文 playbook | 是 | Markdown；可上传 `SKILL.md` |
| 使用场景 | `use-scenario` / `useScenario` | 是 | 何时触发 / 适用语境 |
| 如何使用 | `how-to-use` / `howToUse` | 是 | 用户如何调用、需要哪些输入 |
| 输出内容 | `output` | 是 | 预期交付物 |

> 客户端创建页**不再**要求「选择类型 / 案例链接 / 标签 / Overlay」；`category`、`case-url`、`tags`、overlay 仍为包格式可选字段，供 Hub 发布或手工编辑 `SKILL.md` 使用。

### 2.2 广场类型枚举（`category`，可选）

| value | 展示名（中） | 备注 |
|-------|--------------|------|
| `short-drama` | 短漫剧 | LibTV「短漫剧」 |
| `film` | 电影 | LibTV「电影 / 专业影视」 |
| `advertising` | 商业广告 | LibTV「商业广告」 |
| `creative-social` | 创意/社媒玩法 | LibTV「创意/社媒玩法」 |
| `music-mv` | 音乐 MV | LibTV「音乐 MV」 |

历史包可继续使用 `travel` / `action` / `drama` / `aesthetic` / `product` / `documentary` 等扩展值；**新建 Skill 优先使用上表五类**。

### 2.3 Frontmatter 总表

| Frontmatter | 必填 | 说明 |
|-------------|------|------|
| `name` | 是 | kebab-case，`^[a-z0-9]+(-[a-z0-9]+)*$`，≤64 |
| `display-name` | 否* | 展示名；创建 UI 视为必填，缺省用人性化 `name` |
| `description` | 是 | ≤500；**一句话介绍**（能力摘要） |
| `category` | 建议 | 见 §2.2 |
| `version` | 否 | semver，缺省 `1.0.0` |
| `tags` | 否 | ≤20，单 tag ≤32 |
| `compatible-modes` | 否 | **建议省略**（空 = 全 Mode；Skill 不按 Mode 区分） |
| `use-scenario` | 建议 | LibTV「使用场景」 |
| `how-to-use` | 建议 | LibTV「如何使用」 |
| `output` | 建议 | LibTV「输出内容」 |
| `cover-url` | 否 | 封面公网 URL |
| `case-url` | 否 | 精选案例链接 |
| `requirement-overlay` | 否 | 注入叙事 `<USER_REQUIREMENT>` |
| `style-overlay` | 否 | 注入视觉 `style` |
| 正文 playbook | 是* | Markdown；创建 UI 必填；建议含「做什么 / 需要什么输入 / 怎么做 / 产出什么 / 什么时候问你」 |

### 2.4 Skill 内容写作模板（推荐）

与 LibTV 创建页 placeholder 一致：

```markdown
## 做什么
（一句话说明用途）例：把一句话故事想法做成一条短漫剧成片

## 需要什么输入
（最少提供什么）例：一句话想法，可选画风、时长、主角设定

## 怎么做
（写你在意的环节和要求，不用写全）例：脚本要反转多，画风固定成韩漫

## 产出什么
（最终交付什么）例：成片，附脚本和分镜

## 什么时候问你
（什么情况下停下来问你）例：拿不准题材或风格时问一次，其余自己定
```

可选子目录：`references/`、`templates/`（随 zip 打包）。  
文件夹导入：根目录必须存在全大写 `SKILL.md`（与 LibTV `createSkillFolderPrimaryRequired` 一致）。

**发布物**：将 Skill 目录打成 zip（建议扩展名 `.vimaxskill` 或 `.zip`），经 OSS 直传后把 `publicUrl` / `objectKey` 提交给 publish 接口。

---

## 3. 状态机

| status | 含义 | 广场可见 | 可安装 |
|--------|------|----------|--------|
| `draft` | 仅服务端草稿（可选；客户端也可只本地） | 否 | 否 |
| `pending` | 已提交，待审核 | 否 | 否 |
| `published` | 审核通过 | **是** | **是** |
| `offline` | 驳回或下架 | 否 | 否（已安装本地副本仍可用） |
| `deleted` | 作者软删 | 否 | 否 |

```
用户 publish ──► pending
                   │
          运营 approve
                   ▼
               published ◄──► offline（运营/作者 unpublish）
                   │
              作者 DELETE
                   ▼
                deleted
```

与 TV Show 一致：**`POST /publish` 成功后为 `pending`，不立刻进广场。**

---

## 4. 认证与统一响应

| 项 | 说明 |
|----|------|
| Header | `Authorization: Bearer <C端 JWT>` |
| 作者 | 仅服务端从 JWT 解析，**禁止**客户端传 `userId` |
| 响应 | `{ "code": 200, "msg": "操作成功", "data": { } }`，`code === 200` 为成功 |

路径约定同 TV Show：业务根 + `/vimax/skills/...` ↔ 服务端 `/api/v1/vimax/skills/...`。

---

## 5. 数据模型（MySQL 建议）

### 5.1 `vimax_skill`

| 列 | 类型 | 说明 |
|----|------|------|
| `id` | BIGINT PK AI | 云端 Skill id |
| `author_id` | BIGINT | 发布者 |
| `name` | VARCHAR(64) | 包内 name（作者维度唯一） |
| `display_name` | VARCHAR(128) | |
| `description` | VARCHAR(500) | |
| `category` | VARCHAR(64) | 索引 |
| `version` | VARCHAR(32) | 当前版本 |
| `tags_json` | JSON | string[] |
| `compatible_modes_json` | JSON | string[]；可空 |
| `use_scenario` | TEXT | LibTV 使用场景 |
| `how_to_use` | TEXT | LibTV 如何使用 |
| `output_desc` | TEXT | LibTV 输出内容（列名避免 SQL 关键字 `output`） |
| `case_url` | VARCHAR(1024) | 精选案例链接 |
| `status` | VARCHAR(16) | pending/published/offline/deleted |
| `package_url` | VARCHAR(1024) | zip 公网 URL |
| `package_object_key` | VARCHAR(512) | OSS key |
| `package_size_bytes` | BIGINT | |
| `package_sha256` | CHAR(64) | 可选 |
| `cover_url` | VARCHAR(1024) | 封面（选填；广场建议必填） |
| `cover_object_key` | VARCHAR(512) | 封面 OSS key |
| `install_count` | INT | 安装次数 |
| `like_count` | INT | |
| `reject_reason` | VARCHAR(500) | |
| `published_at` | DATETIME NULL | 首次上架时间 |
| `created_at` / `updated_at` | DATETIME | |

唯一约束：`(author_id, name)` WHERE `status <> 'deleted'`（或软删后允许复用 name，需产品确认；建议删除后 30 天内保留占用）。

### 5.2 `vimax_skill_version`（建议）

每次重新发布写一版快照，便于回滚与审计：

| 列 | 说明 |
|----|------|
| `id` | PK |
| `skill_id` | FK |
| `version` | semver |
| `package_*` | 同主表 |
| `manifest_text` | 可选：SKILL.md 全文（便于详情页不下载 zip） |
| `created_at` | |

### 5.3 `vimax_skill_like` / `vimax_skill_install`（可选一期）

- like：`(user_id, skill_id)` 唯一  
- install：记一次安装事件（或仅 `install_count++`）

---

## 6. C 端接口

### 6.1 发布 / 更新 Skill

- **URL**: `/api/v1/vimax/skills/publish`
- **Method**: `POST`
- **Auth**: 需要

**Request**

```json
{
  "name": "luxury-tvc",
  "displayName": "高奢版TVC",
  "description": "简短描述该 Skill 的能力",
  "category": "advertising",
  "version": "1.0.0",
  "tags": ["tvc", "luxury"],
  "useScenario": "品牌需要高级成片质感的广告短片时使用",
  "howToUse": "用户提供产品卖点或一句话想法；可选参考图",
  "output": "15–45s 高奢 TVC 成片，附分镜与脚本",
  "compatibleModes": [],
  "requirementOverlay": "可选；也可只放在 package 的 SKILL.md 内",
  "styleOverlay": "可选",
  "playbook": "可选 Markdown；若 package 已含 SKILL.md 可省略",
  "packageUrl": "https://cdn.example.com/claw/presigned/42/.../skill.vimaxskill",
  "packageObjectKey": "claw/presigned/42/.../skill.vimaxskill",
  "packageSizeBytes": 24576,
  "packageSha256": "可选64位十六进制",
  "coverUrl": "可选，建议横版封面",
  "coverObjectKey": "可选",
  "caseUrl": "可选精选案例 http(s) 链接",
  "clientSkillId": "user:luxury-tvc"
}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `name` | 是 | 与 SKILL.md `name` 一致 |
| `displayName` | 是* | LibTV「Skill 名称」；服务端也可从包内读取 |
| `description` | 是 | LibTV「一句话介绍」 |
| `category` | 是* | 见 §2.2；非法值拒绝 |
| `useScenario` / `howToUse` / `output` | 是* | LibTV 使用说明三件套；可从 SKILL.md frontmatter 回填 |
| `packageUrl` + `packageObjectKey` | 是 | 必须属于当前用户 presign 路径 |
| `coverUrl` | 否 | 广场展示强烈建议 |
| `caseUrl` | 否 | 须 `http(s)://` |
| `compatibleModes` | 否 | 建议空数组 / 省略 |
| `clientSkillId` | 否 | 客户端本地 id，便于对账 |
| overlay / playbook | 否 | 若提供，服务端可冗余存库便于列表检索；**以 package 内 SKILL.md 为准** |

\*标「是*」：若请求体缺省但 zip 内 `SKILL.md` 已有对应 frontmatter / 正文，服务端应解析补齐；两者都缺则 400。

**行为**

- 同作者同 `name` 再次 publish：更新包与元数据，状态回到 `pending`，写新 `vimax_skill_version`。  
- 校验 zip 内存在 `SKILL.md`，且 frontmatter `name` 与请求 `name` 一致。  
- 包大小建议上限：**5 MiB**（一期足够；可配置）。

**Response `data`**

```json
{
  "id": 10001,
  "name": "luxury-tvc",
  "displayName": "高奢版TVC",
  "status": "pending",
  "version": "1.0.0",
  "submittedAt": "2026-08-12T10:00:00+08:00"
}
```

### 6.2 广场列表

- **URL**: `/api/v1/vimax/skills/list`
- **Method**: `GET`
- **Auth**: 建议需要（与 TV Show 一致）

**Query**

| 参数 | 说明 |
|------|------|
| `page` / `pageSize` | 分页，pageSize 默认 20，最大 50 |
| `keyword` | 搜 displayName / name / description / tags |
| `category` | 分类过滤 |
| `mode` | 兼容 Mode：`idea2video` 等；空 compatible_modes 视为全兼容 |
| `sort` | `hot`（安装数）/ `new`（published_at）/ `like` |
| `authorId` | 可选，按作者筛 |

**仅返回 `status=published`。**

**Response `data`**

```json
{
  "total": 100,
  "page": 1,
  "pageSize": 20,
  "list": [
    {
      "id": 10001,
      "name": "luxury-tvc",
      "displayName": "高奢版TVC",
      "description": "…",
      "category": "advertising",
      "version": "1.0.0",
      "tags": ["tvc"],
      "compatibleModes": [],
      "useScenario": "…",
      "howToUse": "…",
      "output": "…",
      "coverUrl": null,
      "caseUrl": null,
      "installCount": 12,
      "likeCount": 3,
      "publishedAt": "…",
      "author": { "id": 42, "name": "Alice", "avatarUrl": null }
    }
  ]
}
```

### 6.3 我的发布

- **URL**: `/api/v1/vimax/skills/mine`
- **Method**: `GET`
- **Query**: `page` / `pageSize` / `status`（可选）

返回作者名下非 `deleted` 记录（含 pending / offline）。

### 6.4 详情

- **URL**: `/api/v1/vimax/skills/{id}`
- **Method**: `GET`

`published`：任意登录用户可读。  
`pending/offline`：仅作者或运营可读。

`data` 在列表字段基础上增加：

```json
{
  "packageUrl": "…",
  "packageSizeBytes": 24576,
  "manifestText": "可选 SKILL.md 全文",
  "rejectReason": null,
  "status": "published",
  "liked": false
}
```

### 6.5 安装（拉取包）

- **URL**: `/api/v1/vimax/skills/{id}/install`
- **Method**: `POST`
- **Auth**: 需要

**行为**

1. 校验 `published`。  
2. `install_count++`（可异步）。  
3. 返回安装所需信息（客户端再下载 zip 写入本地 `vimax/skills/user/<name>/`）：

```json
{
  "id": 10001,
  "name": "luxury-tvc",
  "version": "1.0.0",
  "packageUrl": "…",
  "packageSha256": "…",
  "qualifiedId": "hub:luxury-tvc"
}
```

> 说明：客户端安装后本地 id 建议为 `user:<name>`（覆盖同名）或保留 `cloud:<id>` 映射表；**一期推荐安装为 `user:<name>`，并在本地 manifest 写入 `cloud-id` / `cloud-version` 注释字段。**

### 6.6 点赞 / 取消

- `POST /api/v1/vimax/skills/{id}/like`  
- `DELETE /api/v1/vimax/skills/{id}/like`  

响应：`{ "id", "liked", "likeCount" }`（对齐 TV Show）。

### 6.7 作者下架 / 删除

- `POST /api/v1/vimax/skills/{id}/unpublish` → `offline`  
- `DELETE /api/v1/vimax/skills/{id}` → `deleted`  

仅作者；已安装用户本地副本不强制删除。

---

## 7. 运营接口（可内网 / Admin JWT）

| 接口 | 说明 |
|------|------|
| `GET /api/v1/admin/vimax/skills?status=pending` | 审核队列 |
| `POST /api/v1/admin/vimax/skills/{id}/approve` | → published，写 `published_at` |
| `POST /api/v1/admin/vimax/skills/{id}/reject` | body: `{ "reason": "…" }` → offline |
| `POST /api/v1/admin/vimax/skills/{id}/offline` | 强制下架 |

---

## 8. 校验与安全

1. **OSS 归属**：`packageObjectKey` / `coverObjectKey` 必须含当前用户 presign 前缀。  
2. **Zip 安全**：防 zip-slip；限制解压后文件数（≤100）与总大小（≤5MiB）。  
3. **内容**：至少存在根级或单层目录下的 `SKILL.md`；YAML 解析失败则 400。  
4. **敏感词**：description / playbook 走现有内容安全（若有）。  
5. **限流**：publish 建议 10 次/小时/用户；install 60 次/小时/用户。  
6. **版权**：发布协议文案由产品提供；接口可增加 `acceptedTerms: true`。

---

## 9. 错误码建议

| code | 场景 |
|------|------|
| 400 | 参数 / SKILL.md 非法 / Mode 非法 |
| 401 | 未登录 |
| 403 | 非作者操作 |
| 404 | 不存在或不可见 |
| 409 | 同名冲突（若策略不允许覆盖） |
| 413 | 包过大 |
| 429 | 限流 |

业务 `msg` 用中文，便于客户端直接 toast。

---

## 10. 客户端对接计划（供联调）

| 阶段 | 客户端 | 服务端 |
|------|--------|--------|
| 已完成 | 本机 Mode×Skill、本机创建/导入/本机 Hub、plan overlay | — |
| **本期** | UI 已改为 Mode 同款 Popover；创建/本机发布可用 | 实现本文 6.x + 表结构 |
| 联调 | `FlowyApiClient` 增加 `/vimax/skills/*`；安装写入本地 catalog；广场 Tab 拉云端 list | 提供测试环境 JWT + 审核工具 |
| 二期 | 云端更新检测、官方精选位、激励 | 运营后台、排序策略 |

客户端本地目录约定（已实现）：

```
{data_dir}/vimax/skills/
  user/<name>/SKILL.md
  hub/<name>/SKILL.md    # 本机「发布到 Hub」副本
```

云端安装建议落入 `user/<name>/`，frontmatter 增加：

```yaml
cloud-id: 10001
cloud-version: "1.0.0"
source: cloud
```

---

## 11. 验收清单

- [ ] 用户 A 上传 zip 并 publish → `pending`，广场不可见  
- [ ] 运营 approve → 用户 B list 可见并可 install  
- [ ] install 后客户端可挂载该 Skill 并成功 plan（overlay 生效）  
- [ ] 同名再次 publish → 新版本 pending，旧 published 按产品策略（建议：**直接替换主表并重新审核** 或 **双版本保留仅最新 published**——推荐前者简单）  
- [ ] reject 带 reason，作者 mine 可见  
- [ ] 非法 zip / 缺 SKILL.md / name 不一致 → 400  
- [ ] 未登录 publish / install → 401  

---

## 12. 联系与参考

- 产品对标：[LibTV](https://www.liblib.tv/) Agent Skills；创建页 [skill/create](https://www.liblib.tv/skill/create)  
- 客户端现状：`allo/crates/agent/nomi-vimax/src/skills/`、`allo/ui/.../VerticalSkillCreateModal.tsx`  
- 接口风格参考：`allo/docs/vimax-tv-show-api.md`

**文档版本**：v1.1 · 2026-08-12  
**对接优先级**：P0 = publish / list / detail / install / mine / admin approve；P1 = like / version 表 / 封面与案例素材审核。

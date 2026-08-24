# ViMax Skill Hub API 文档（客户端对接）

> 本文档为服务端落地后的正式接口说明，供客户端实现发布、广场浏览、安装拉取等能力。  
> 上传仍复用已有 OSS 预签名直传：`POST /api/v1/uploads/oss/presignPut`。  
> 需求原稿：`vimax-skill-hub-api.md`。

---

## 1. 业务说明

### 1.1 状态机


| status      | 含义             | 广场可见  | 可安装   |
| ----------- | -------------- | ----- | ----- |
| `pending`   | 用户已提交，等待运营审核   | 否     | 否     |
| `published` | 运营审核通过 / 上架    | **是** | **是** |
| `offline`   | 运营驳回 / 作者或运营下架 | 否     | 否     |
| `deleted`   | 作者软删除          | 否     | 否     |


```
用户 publish ──► pending
                   │
          运营 approve
                   ▼
               published ◄──► offline（作者 unpublish / 运营 offline / reject）
                   │
              作者 DELETE
                   ▼
                deleted
```

**重要：** `POST /publish` 成功后状态为 `pending`，不会立刻出现在广场；需运营审核通过后变为 `published`。

### 1.2 幂等 / 更新策略

同一作者同一 `name` **再次发布会更新**元数据与包，写入新版本快照，并重新进入 `pending`（清空驳回原因与审核信息）。主表始终保留最新一版。

### 1.3 认证


| 项      | 说明                                                |
| ------ | ------------------------------------------------- |
| Header | `Authorization: Bearer <C端 JWT>`                  |
| 作者     | 仅服务端从 JWT 解析，**禁止**传 `userId` / `authorId` 作为身份声明 |




### 1.4 统一响应

```json
{ "code": 200, "msg": "操作成功", "data": { } }
```

客户端以 `code === 200` 判断成功。业务错误时关注 HTTP 状态码与响应中的 `error_key`（不要依赖 `msg` 文案做分支）。

### 1.5 路径约定


| 环境   | 客户端业务根（示例）                         | 服务端           |
| ---- | ---------------------------------- | ------------- |
| 生产国内 | `https://server.flowyaipc.com/claw` | `/api/v1/...` |


示例：`POST {业务根}/vimax/skills/publish` ↔ `POST /api/v1/vimax/skills/publish`。

---



## 2. 文件上传（已有，不新增）

```
POST /api/v1/uploads/oss/presignPut
→ PUT 到 data.url
→ 业务字段使用 data.publicUrl / data.objectKey
```

Skill 包示例（`.vimaxskill` 或 `.zip`）：

```json
{ "fileName": "luxury-tvc.vimaxskill", "contentType": "application/zip", "expiresSeconds": 3600 }
```

封面示例：

```json
{ "fileName": "cover.png", "contentType": "image/png", "expiresSeconds": 900 }
```

`packageObjectKey` / `coverObjectKey` 必须属于当前用户的 presign 路径（含 `/presigned/{userId}/`）。

**包限制：** 压缩包及解压后总大小 ≤ **5 MiB**；文件数 ≤ **100**；须包含根级或单层目录下的 `SKILL.md`。

---



## 3. C 端接口



### 3.1 发布 / 更新 Skill

- **URL**: `/api/v1/vimax/skills/publish`
- **Method**: `POST`
- **Auth**: C 端 JWT

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
  "requirementOverlay": "可选",
  "styleOverlay": "可选",
  "playbook": "可选 Markdown；包内 SKILL.md 可省略此项",
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


| 字段                                    | 类型       | 必填  | 说明                                                                              |
| ------------------------------------- | -------- | --- | ------------------------------------------------------------------------------- |
| `name`                                | string   | 是   | kebab-case，`^[a-z0-9]+(-[a-z0-9]+)*$`，≤64；须与包内 `SKILL.md` frontmatter `name` 一致 |
| `displayName`                         | string   | 是*  | ≤64；缺省可从包内 `display-name` 或人性化 `name` 补齐                                        |
| `description`                         | string   | 是*  | ≤500                                                                            |
| `category`                            | string   | 是*  | 见下方分类枚举                                                                         |
| `useScenario` / `howToUse` / `output` | string   | 是*  | 可从包内 frontmatter 回填；两者都缺则 400                                                   |
| `version`                             | string   | 否   | 缺省 `1.0.0`                                                                      |
| `tags`                                | string[] | 否   | ≤20，单 tag ≤32                                                                   |
| `compatibleModes`                     | string[] | 否   | `idea2video` / `script2video` / `novel2video`；空=全兼容                             |
| `packageUrl` + `packageObjectKey`     | string   | 是   | OSS 直传结果                                                                        |
| `packageSizeBytes`                    | number   | 否   | 服务端也会按实际下载大小校验                                                                  |
| `packageSha256`                       | string   | 否   | 64 位十六进制                                                                        |
| `coverUrl` / `coverObjectKey`         | string   | 否   | 封面                                                                              |
| `caseUrl`                             | string   | 否   | 须 `http(s)://`                                                                  |
| `clientSkillId`                       | string   | 否   | 客户端本地 id，便于对账                                                                   |
| `overlay` / `playbook`                | string   | 否   | 冗余存库；以包内 `SKILL.md` 为准                                                          |


「是*」：请求体缺省时，服务端会下载并解析 zip 内 `SKILL.md` 补齐；仍缺则返回 400。

**分类** `category`**（新建优先）**


| value             | 展示名     |
| ----------------- | ------- |
| `short-drama`     | 短漫剧     |
| `film`            | 电影      |
| `advertising`     | 商业广告    |
| `creative-social` | 创意/社媒玩法 |
| `music-mv`        | 音乐 MV   |


另兼容历史值：`travel` / `action` / `drama` / `aesthetic` / `product` / `documentary`。

**Response** `data`

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

---



### 3.2 广场列表

- **URL**: `/api/v1/vimax/skills/list`
- **Method**: `GET`
- **Auth**: C 端 JWT

**Query**


| 参数                  | 说明                                         |
| ------------------- | ------------------------------------------ |
| `page` / `pageSize` | 分页，pageSize 默认 20，最大 50                    |
| `keyword`           | 搜 displayName / name / description / tags  |
| `category`          | 分类过滤                                       |
| `mode`              | 兼容 Mode；空 `compatibleModes` 视为全兼容          |
| `sort`              | `hot`（安装数）/ `new`（published_at，默认）/ `like` |
| `authorId`          | 按作者筛                                       |


**仅返回** `status=published`**。**

**Response** `data`

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
      "liked": false,
      "status": "published",
      "publishedAt": "…",
      "submittedAt": "…",
      "isMine": false,
      "author": { "id": 42, "name": "Alice", "avatarUrl": null }
    }
  ]
}
```

---



### 3.3 我的发布

- **URL**: `/api/v1/vimax/skills/mine`
- **Method**: `GET`
- **Auth**: C 端 JWT
- **Query**: `page` / `pageSize` / `status`（可选：`pending` / `published` / `offline`）

返回当前用户名下非 `deleted` 记录。作者可见 `rejectReason`。

---



### 3.4 详情

- **URL**: `/api/v1/vimax/skills/{id}`
- **Method**: `GET`
- **Auth**: C 端 JWT

可见性：

- `published`：任意登录用户可读  
- `pending` / `offline`：仅作者可读

在列表字段基础上增加：

```json
{
  "packageUrl": "…",
  "packageSizeBytes": 24576,
  "packageSha256": "…",
  "manifestText": "可选 SKILL.md 全文",
  "rejectReason": null,
  "status": "published",
  "liked": false,
  "clientSkillId": "仅作者可见"
}
```

---



### 3.5 安装（拉取包）

- **URL**: `/api/v1/vimax/skills/{id}/install`
- **Method**: `POST`
- **Auth**: C 端 JWT

行为：校验 `published` → `install_count++` → 返回下载信息。

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

客户端建议安装到本地 `vimax/skills/user/<name>/`，并在 frontmatter 写入：

```yaml
cloud-id: 10001
cloud-version: "1.0.0"
source: cloud
```

---



### 3.6 点赞 / 取消

- `POST /api/v1/vimax/skills/{id}/like`
- `DELETE /api/v1/vimax/skills/{id}/like`

响应：

```json
{ "id": 10001, "liked": true, "likeCount": 4 }
```

仅对 `published` 生效；重复点赞 / 取消为幂等。

---



### 3.7 作者下架 / 删除

- `POST /api/v1/vimax/skills/{id}/unpublish` → `offline`（`pending` / `published` 可下架）
- `DELETE /api/v1/vimax/skills/{id}` → `deleted`

仅作者；已安装用户本地副本不强制删除。成功响应 `data` 可为 `null`。

---



## 4. 运营接口（Sys JWT）

> 路径对齐本仓库运营后台惯例：`/api/v1/sys/...`（需求稿中的 `/admin/vimax/skills` 映射到此处）。  
> 权限码：`sys:vimax_skill:read` / `sys:vimax_skill:write`。


| 接口                                           | 说明                                    |
| -------------------------------------------- | ------------------------------------- |
| `GET /api/v1/sys/vimaxSkills?status=pending` | 审核队列 / 列表                             |
| `GET /api/v1/sys/vimaxSkills/{id}`           | 详情                                    |
| `POST /api/v1/sys/vimaxSkills/{id}/approve`  | → `published`（首次写入 `publishedAt`）     |
| `POST /api/v1/sys/vimaxSkills/{id}/reject`   | body: `{ "reason": "…" }` → `offline` |
| `POST /api/v1/sys/vimaxSkills/{id}/offline`  | 强制下架 published → offline              |


列表 Query 另支持：`page` / `pageSize` / `category` / `keyword` / `authorId` / `externalChannelCode`。

---



## 5. 错误码


| HTTP / code | error_key                                            | 场景                       |
| ----------- | ---------------------------------------------------- | ------------------------ |
| 400         | `error.invalid_param` / `error.invalid_request_body` | 参数非法                     |
| 400         | `error.vimax_skill.invalid_package`                  | zip 非法 / zip-slip / 文件过多 |
| 400         | `error.vimax_skill.skill_md_missing`                 | 缺 SKILL.md               |
| 400         | `error.vimax_skill.invalid_skill_md`                 | YAML 解析失败                |
| 400         | `error.vimax_skill.name_mismatch`                    | 包内 name 与请求不一致           |
| 400         | `error.vimax_skill.package_download_failed`          | 无法下载包校验                  |
| 401         | `error.unauthorized` / auth.*                        | 未登录                      |
| 403         | `error.vimax_skill.forbidden`                        | 非作者操作                    |
| 404         | `error.vimax_skill.not_found`                        | 不存在或不可见                  |
| 409         | `error.vimax_skill.invalid_state`                    | 状态不允许                    |
| 413         | `error.vimax_skill.package_too_large`                | 包过大                      |


---



## 6. 联调建议

1. 用 `presignPut` 上传 zip（含合法 `SKILL.md`）→ `publish` → 确认 `status=pending`，`list` 不可见。
2. 运营 `approve` → `list` 可见 → `install` 拿到 `packageUrl`。
3. 同 `name` 再次 `publish` → 回到 `pending`，详情版本更新。
4. `reject` 带 `reason` → 作者 `mine` 可见 `rejectReason`。
5. 非法 zip / 缺 SKILL.md / name 不一致 → 400。
6. 未登录 publish / install → 401。

---

**文档版本**：v1.0 · 2026-08-12（与服务端实现同步）
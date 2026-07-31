# ViMax TV Show API 文档（客户端对接）

> 本文档为服务端落地后的正式接口说明，供客户端实现发布、广场列表、详情下载工程包等能力。  
> 上传仍复用已有 OSS 预签名直传：`POST /api/v1/uploads/oss/presignPut`。  
> 设计参考：`vimax-tv-show-api-params.md`。

---

## 1. 业务说明

### 1.1 状态机

| status | 含义 | C 端广场可见 |
|--------|------|-------------|
| `pending` | 用户已提交，等待运营审核 | 否 |
| `published` | 运营审核通过 / 上架 | **是** |
| `offline` | 运营驳回或下架 | 否 |
| `deleted` | 作者删除（软删除） | 否 |

```
用户 publish ──► pending
                   │
          运营 approve / publish
                   ▼
               published ◄──► offline（运营 unpublish / reject）
                   │
              作者 DELETE
                   ▼
                deleted
```

**重要：** `POST /publish` 成功后状态为 **`pending`**，不会立刻出现在广场列表；需运营审核通过后变为 `published`。

### 1.2 幂等策略

同一用户同一本地工程 `(userId, clientSessionId)` **再次发布会更新**封面/工程包/标题等字段，并重新进入 `pending`（清空驳回原因与审核信息）。

### 1.3 认证

| 项 | 说明 |
|----|------|
| Header | `Authorization: Bearer <C端 JWT>` |
| 作者 | 仅服务端从 JWT 解析，**禁止**传 `userId` |

### 1.4 统一响应

```json
{ "code": 200, "msg": "操作成功", "data": { } }
```

客户端以 `code === 200` 判断成功。

### 1.5 路径约定

| 环境 | 客户端业务根（示例） | 服务端 |
|------|---------------------|--------|
| 生产国内 | `https://server.flowyaipc.cn/claw` | `/api/v1/...` |

示例：`POST {业务根}/vimax/tv-show/publish` ↔ `POST /api/v1/vimax/tv-show/publish`。

---

## 2. 文件上传（已有，不新增）

```
POST /api/v1/uploads/oss/presignPut
→ PUT 到 data.url
→ 业务字段使用 data.publicUrl / data.objectKey
```

封面示例：

```json
{ "fileName": "cover.png", "contentType": "image/png", "expiresSeconds": 900 }
```

工程包示例（`.nomivimax` 本质为 ZIP，扩展名已支持）：

```json
{ "fileName": "雨夜咖啡馆.nomivimax", "contentType": "application/zip", "expiresSeconds": 3600 }
```

若传入 `coverObjectKey` / `packageObjectKey`，须属于当前用户的 presign 路径（含 `/presigned/{userId}/`）。

---

## 3. C 端接口

### 3.1 发布到 TV Show

- **URL**: `/api/v1/vimax/tv-show/publish`
- **Method**: `POST`
- **Auth**: C 端 JWT

**Request**

```json
{
  "clientSessionId": "550e8400-e29b-41d4-a716-446655440000",
  "title": "雨夜咖啡馆",
  "description": "可选简介",
  "workflow": "idea2video",
  "style": "cinematic, warm tones",
  "targetDurationSecs": 30,
  "coverUrl": "https://cdn.example.com/claw/presigned/42/20260731/aaaa.png",
  "coverObjectKey": "claw/presigned/42/20260731/aaaa.png",
  "packageUrl": "https://cdn.example.com/claw/presigned/42/20260731/bbbb.nomivimax",
  "packageObjectKey": "claw/presigned/42/20260731/bbbb.nomivimax",
  "packageSizeBytes": 157286400,
  "packageSha256": "可选64位十六进制",
  "archiveVersion": 1
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `clientSessionId` | string | 是 | 本地工程 id，最长 64 |
| `title` | string | 是 | 最长 200 |
| `description` | string | 否 | 最长 1000 |
| `workflow` | string | 是 | `idea2video` / `script2video` / `novel2video` |
| `style` | string | 否 | 最长 200 |
| `targetDurationSecs` | int | 否 | 1–86400 |
| `coverUrl` | string | 是 | http(s) URL |
| `coverObjectKey` | string | 否 | OSS key |
| `packageUrl` | string | 是 | http(s) URL |
| `packageObjectKey` | string | 否 | OSS key |
| `packageSizeBytes` | int64 | 否 | ≥0 |
| `packageSha256` | string | 否 | 恰好 64 字符 |
| `archiveVersion` | int | 否 | 默认 1，须 >0 |

**Success `data`**

```json
{
  "id": 10086,
  "clientSessionId": "550e8400-e29b-41d4-a716-446655440000",
  "title": "雨夜咖啡馆",
  "status": "pending",
  "coverUrl": "https://cdn.example.com/...",
  "packageUrl": "https://cdn.example.com/...",
  "workflow": "idea2video",
  "submittedAt": "2026-07-31T06:00:00.000Z",
  "publishedAt": null,
  "author": {
    "id": 42,
    "name": "Alice",
    "avatarUrl": "https://..."
  }
}
```

---

### 3.2 广场列表（仅已上架）

- **URL**: `/api/v1/vimax/tv-show/list`
- **Method**: `GET`
- **Auth**: C 端 JWT

**Query**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `page` | int | 否 | 默认 1 |
| `pageSize` | int | 否 | 默认 20，上限 50 |
| `workflow` | string | 否 | 工作流筛选 |
| `keyword` | string | 否 | 标题模糊搜索 |
| `sort` | string | 否 | `publishedAtDesc`（默认）/ `publishedAtAsc` |

**Success `data`**

```json
{
  "total": 120,
  "page": 1,
  "pageSize": 20,
  "list": [
    {
      "id": 10086,
      "title": "雨夜咖啡馆",
      "coverUrl": "https://cdn.example.com/...",
      "workflow": "idea2video",
      "style": "cinematic, warm tones",
      "targetDurationSecs": 30,
      "status": "published",
      "publishedAt": "2026-07-31T06:10:00.000Z",
      "submittedAt": "2026-07-31T06:00:00.000Z",
      "updatedAt": "2026-07-31T06:10:00.000Z",
      "author": {
        "id": 42,
        "name": "Alice",
        "avatarUrl": "https://..."
      },
      "likeCount": 0,
      "viewCount": 0,
      "liked": false,
      "isMine": false
    }
  ]
}
```

> 列表**不返回** `packageUrl`。下载请走详情接口。  
> `liked` 表示**当前登录用户**是否已点赞该视频。

---

### 3.3 我的发布

- **URL**: `/api/v1/vimax/tv-show/mine`
- **Method**: `GET`
- **Auth**: C 端 JWT

**Query**：`page` / `pageSize` / `status`（可选：`pending`/`published`/`offline`，不含 `deleted`）

返回结构同广场列表。自己的记录在 `offline` 时可能带 `rejectReason`。

---

### 3.4 详情（含工程包下载）

- **URL**: `/api/v1/vimax/tv-show/{id}`
- **Method**: `GET`
- **Auth**: C 端 JWT

可见性：

- 任何人：`status=published`
- 作者本人：非 `deleted` 的任意状态

**Success `data`**（在列表字段基础上增加）：

| 字段 | 说明 |
|------|------|
| `description` | 简介 |
| `packageUrl` | 工程包下载地址 |
| `packageSizeBytes` | 字节数 |
| `archiveVersion` | 导出包版本 |
| `clientSessionId` | **仅** `isMine=true` 时返回 |
| `liked` | 当前用户是否已点赞 |
| `likeCount` | 点赞总数 |

---

### 3.5 点赞 / 取消点赞

仅可对 **`published`** 状态视频操作；重复点赞 / 取消点赞均为幂等成功。

#### 点赞

- **URL**: `/api/v1/vimax/tv-show/{id}/like`
- **Method**: `POST`
- **Auth**: C 端 JWT
- **Success `data`**:

```json
{ "id": 10086, "liked": true, "likeCount": 12 }
```

#### 取消点赞

- **URL**: `/api/v1/vimax/tv-show/{id}/like`
- **Method**: `DELETE`
- **Auth**: C 端 JWT
- **Success `data`**:

```json
{ "id": 10086, "liked": false, "likeCount": 11 }
```

点赞记录落库表 `tb_tv_show_likes`（唯一约束 `video_id + user_id`），并维护视频上的 `likeCount` 计数。

---

### 3.6 删除自己的发布

- **URL**: `/api/v1/vimax/tv-show/{id}`
- **Method**: `DELETE`
- **Auth**: C 端 JWT（仅作者）

软删除：`status → deleted`。成功 `data` 为空。

---

## 4. 错误码

| HTTP | code | errorKey | 场景 |
|------|------|----------|------|
| 401 | 401 | `error.unauthorized` / `error.auth.*` | 未登录或 JWT 无效 |
| 400 | 400 | `error.invalid_param` | 参数非法（`field` 标明字段） |
| 400 | 400 | `error.invalid_request_body` | Body 解析失败 |
| 403 | 403 | `error.tv_show.forbidden` | 操作他人作品 |
| 404 | 404 | `error.tv_show.not_found` | 不存在、不可见，或非已上架不可点赞 |
| 409 | 409 | `error.tv_show.invalid_state` | 状态不允许该操作（运营侧更多见） |
| 500 | 500 | `error.internal` | 内部错误 |

---

## 5. 客户端对接清单

| 步骤 | 说明 |
|------|------|
| 1 | 本地工程 `status === succeeded` 且有成片后再允许发布 |
| 2 | OSS 直传封面 + `.nomivimax` 工程包 |
| 3 | `POST /publish`，展示「审核中」（`pending`） |
| 4 | `GET /mine` 轮询或进入页面刷新审核结果；`offline` 可读 `rejectReason` |
| 5 | `GET /list` 渲染 TV Show 广场，用 `liked` / `likeCount` 展示点赞态 |
| 6 | `POST|DELETE /{id}/like` 切换点赞 |
| 7 | `GET /{id}` 取 `packageUrl` 下载/导入工程 |
| 8 | 作者可 `DELETE /{id}` 下架删除 |

---

文档版本：2026-07-31  
状态：服务端已实现；客户端可按本文档对接。

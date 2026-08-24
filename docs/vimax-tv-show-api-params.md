# ViMax TV Show 服务端接口参数说明

> 面向 Go + MySQL 服务端：设计表结构与业务 API。  
> **文件上传不新增接口**，复用现有 OSS 预签名直传：见 [新客户端 API 对接文档（OSS 直传 + 视频首尾帧）](./new-client-api-oss-upload-video.md)。  
> 客户端本地工程模型参考：`SessionRecord`（`nomi-vimax`），工程包格式与「导出工程」一致（`.nomivimax` ZIP）。

---

## 1. 业务背景

视频生成主页当前仅有「最近创作」（本地工程列表）。计划增加可切换的 **TV Show** 模块：

- 展示形态与「最近创作」相同：视频项目卡片列表（封面、标题、工作流、时间、作者等）。
- 内容来源：用户在本应用视频生成模块中制作并**发布**的工程。
- 发布动作发生在视频详情页：对已生成完成（`status = succeeded`）的工程执行发布。

### 1.1 发布链路

```
用户登录（JWT）
    ↓
本地打包工程（与「导出工程」相同 → .nomivimax ZIP）
    ↓
POST /uploads/oss/presignPut     # 封面海报、工程包各申请一次（已有接口）
    ↓
HTTP PUT 到 data.url             # 直传 OSS
    ↓
记下 data.publicUrl / data.objectKey
    ↓
POST /vimax/tv-show/publish      # 提交业务字段 + URL（本文档新增）
    ↓
GET  /vimax/tv-show/list         # TV Show 列表（本文档新增）
```

作者信息由服务端通过 **JWT** 解析，客户端不传 `userId`。

---

## 2. 通用约定

### 2.1 Base URL

与现有云端业务 API 一致（示例）：

| 用途 | 客户端 Base（示例） |
|------|---------------------|
| 业务 API | `https://server.flowyaipc.com/claw` |

路径映射示例：`{业务根}/vimax/tv-show/publish` → 服务端 `/api/v1/vimax/tv-show/publish`（最终前缀以服务端路由规范为准）。

### 2.2 响应格式

与现有云端约定对齐：

```json
{ "code": 200, "msg": "操作成功", "data": { } }
```

客户端以 **`code === 200`** 判断成功。

### 2.3 认证

| 接口 | JWT |
|------|-----|
| 已有 `POST /uploads/oss/presignPut` | ✅ 仅 JWT |
| 本文档 TV Show 业务接口 | ✅ 仅 JWT |

推荐请求头：

```http
Content-Type: application/json
Authorization: Bearer <JWT>
token: <JWT>
```

---

## 3. 文件上传（复用已有接口，不新增）

完整说明见 [OSS 直传文档](./new-client-api-oss-upload-video.md)。此处只列出本场景用法。

### 3.1 申请预签名

| 项 | 值 |
|----|-----|
| 客户端路径 | `POST {业务根}/uploads/oss/presignPut` |
| 服务端路径 | `POST /api/v1/uploads/oss/presignPut` |

**封面海报示例：**

```json
{
  "fileName": "cover.png",
  "contentType": "image/png",
  "expiresSeconds": 900
}
```

**工程包示例：**

```json
{
  "fileName": "雨夜咖啡馆.nomivimax",
  "contentType": "application/zip",
  "expiresSeconds": 3600
}
```

> 工程包可能较大（客户端导出上限约 8GB），建议 `expiresSeconds` 适当加大；若现有 presign 对扩展名/类型有白名单限制，需服务端确认是否允许 `.nomivimax`（本质为 ZIP）。

### 3.2 直传后取用字段

预签名成功后 `PUT` 到 `data.url`，业务发布接口只使用：

| 字段 | 用途 |
|------|------|
| `data.publicUrl` | 写入 `coverUrl` / `packageUrl` |
| `data.objectKey` | 可选写入，便于删除/迁移 |
| 本地文件大小 | 可选写入 `packageSizeBytes` |

### 3.3 工程包内容（校验参考，非 API 字段）

`.nomivimax` 为 ZIP，结构与本地导出一致：

```text
manifest.json   // version, app="nomifun-vimax", session_id, workflow, title, exported_at
session.json    // 完整 SessionRecord
working/**      // 工作目录（成片、分镜、cameo 等）
```

`manifest.json` 关键字段示例：

| 字段 | 说明 |
|------|------|
| `version` | 当前为 `1` |
| `app` | `"nomifun-vimax"` |
| `exported_at` | RFC3339 |
| `session_id` | 本地工程 id |
| `workflow` | `idea2video` \| `script2video` \| `novel2video` |
| `title` | 标题 |

---

## 4. 建议数据表（逻辑模型）

### 4.1 `tv_show_videos`（主表）

| 字段 | 类型建议 | 必填 | 说明 |
|------|----------|------|------|
| `id` | BIGINT PK | 是 | 服务端主键 |
| `user_id` | BIGINT | 是 | JWT 解析出的作者 |
| `client_session_id` | VARCHAR(64) | 是 | 客户端本地工程 id（`SessionRecord.id`） |
| `title` | VARCHAR(200) | 是 | 视频标题 |
| `description` | VARCHAR(1000) | 否 | 简介（可先空） |
| `workflow` | VARCHAR(32) | 是 | `idea2video` \| `script2video` \| `novel2video` |
| `style` | VARCHAR(200) | 否 | 视觉风格文案 |
| `target_duration_secs` | INT UNSIGNED | 否 | 规划目标时长（秒） |
| `cover_url` | VARCHAR(1024) | 是 | 封面 `publicUrl` |
| `cover_object_key` | VARCHAR(512) | 否 | 封面 `objectKey` |
| `package_url` | VARCHAR(1024) | 是 | 工程包 `publicUrl` |
| `package_object_key` | VARCHAR(512) | 否 | 工程包 `objectKey` |
| `package_size_bytes` | BIGINT | 否 | 工程包字节数 |
| `package_sha256` | CHAR(64) | 否 | 可选校验 |
| `archive_version` | INT | 否 | 导出包 version，当前 `1` |
| `status` | TINYINT / ENUM | 是 | `pending` / `published` / `offline` / `deleted` |
| `like_count` | INT | 否 | 预留，默认 0 |
| `view_count` | INT | 否 | 预留，默认 0 |
| `published_at` | DATETIME | 是 | 首次发布时间 |
| `created_at` | DATETIME | 是 | |
| `updated_at` | DATETIME | 是 | |

**建议唯一约束：** `(user_id, client_session_id)`  
同一用户对同一本地工程仅保留一条有效发布记录；重复发布建议 **更新** 封面/工程包/标题等字段。

### 4.2 作者展示字段（列表 join 用户表）

| 字段 | 说明 |
|------|------|
| `author_id` | 同 `user_id` |
| `author_name` | 昵称 / 用户名 |
| `author_avatar_url` | 头像 URL，可空 |

---

## 5. 业务接口（需新建）

以下路径为建议；最终以服务端路由规范为准。字段名建议 **camelCase**，与现有云端 API 一致。

### 5.1 发布到 TV Show

| 项 | 值 |
|----|-----|
| 客户端路径 | `POST {业务根}/vimax/tv-show/publish` |
| 需登录 | JWT |

#### Request body

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
  "packageSha256": "可选",
  "archiveVersion": 1
}
```

| 字段 | 类型 | 必填 | 来源 |
|------|------|------|------|
| `clientSessionId` | string | 是 | 本地 `session.id` |
| `title` | string | 是 | 本地 `title`，用户可改 |
| `description` | string | 否 | 客户端输入，UI 可后做 |
| `workflow` | string | 是 | `idea2video` / `script2video` / `novel2video` |
| `style` | string | 否 | 本地 `style` |
| `targetDurationSecs` | uint | 否 | 本地 `target_duration_secs` |
| `coverUrl` | string | 是 | OSS 上传返回的 `publicUrl` |
| `coverObjectKey` | string | 否 | OSS 上传返回的 `objectKey` |
| `packageUrl` | string | 是 | OSS 上传返回的 `publicUrl` |
| `packageObjectKey` | string | 否 | OSS 上传返回的 `objectKey` |
| `packageSizeBytes` | int64 | 否 | 本地文件大小或上传侧统计 |
| `packageSha256` | string | 否 | 可选 |
| `archiveVersion` | int | 否 | 当前固定 `1` |

**不要传：** `userId` / `authorName`（由 JWT 解析）。

#### Response `data` 建议

```json
{
  "id": 10086,
  "clientSessionId": "550e8400-e29b-41d4-a716-446655440000",
  "title": "雨夜咖啡馆",
  "status": "published",
  "coverUrl": "https://cdn.example.com/...",
  "packageUrl": "https://cdn.example.com/...",
  "workflow": "idea2video",
  "publishedAt": "2026-07-31T06:00:00Z",
  "author": {
    "id": 42,
    "name": "Alice",
    "avatarUrl": "https://..."
  }
}
```

#### 幂等策略（请服务端明确一种）

| 策略 | 行为 | 建议 |
|------|------|------|
| A | 同一 `(userId, clientSessionId)` 再次发布 → **更新** 记录 | **推荐** |
| B | 已存在则返回业务错误（如 code ≠ 200，msg 说明已发布） | 备选 |

---

### 5.2 TV Show 列表（广场）

| 项 | 值 |
|----|-----|
| 客户端路径 | `GET {业务根}/vimax/tv-show/list` |
| 需登录 | JWT（若仅对登录用户开放） |

#### Query

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `page` | int | 否 | 默认 1 |
| `pageSize` | int | 否 | 默认 20，建议上限 50 |
| `workflow` | string | 否 | 按工作流筛选 |
| `keyword` | string | 否 | 搜标题 |
| `sort` | string | 否 | 默认 `publishedAtDesc`；可选 `publishedAtAsc` |

#### Response `data`

```json
{
  "total": 120,
  "page": 1,
  "pageSize": 20,
  "list": [
    {
      "id": 10086,
      "title": "雨夜咖啡馆",
      "coverUrl": "https://cdn.example.com/claw/presigned/42/20260731/aaaa.png",
      "workflow": "idea2video",
      "style": "cinematic, warm tones",
      "targetDurationSecs": 30,
      "status": "published",
      "publishedAt": "2026-07-31T06:00:00Z",
      "updatedAt": "2026-07-31T06:00:00Z",
      "author": {
        "id": 42,
        "name": "Alice",
        "avatarUrl": "https://..."
      },
      "likeCount": 0,
      "viewCount": 0,
      "isMine": false
    }
  ]
}
```

**列表不必返回 `packageUrl`**（卡片不需要；详情 / 下载再给）。

卡片展示对齐本地「最近创作」：`coverUrl`、`title`、`workflow`、`publishedAt`、`author`；时长 / 点赞可选。

---

### 5.3 TV Show 详情（可选，第二期）

| 项 | 值 |
|----|-----|
| 客户端路径 | `GET {业务根}/vimax/tv-show/{id}` |
| 需登录 | JWT |

在列表字段基础上增加：

| 字段 | 说明 |
|------|------|
| `description` | 简介 |
| `packageUrl` | 工程包下载地址（他人导入 / 自己备份） |
| `packageSizeBytes` | |
| `clientSessionId` | 可仅在 `isMine=true` 时返回 |
| `archiveVersion` | |

---

### 5.4 下架 / 删除自己的发布（建议预留）

| 项 | 值 |
|----|-----|
| 客户端路径 | `POST {业务根}/vimax/tv-show/{id}/unpublish` 或 `DELETE {业务根}/vimax/tv-show/{id}` |
| 需登录 | JWT（仅作者） |

建议软删除（`status = deleted` / `offline`）。

---

### 5.5 我发布的列表（可选）

| 项 | 值 |
|----|-----|
| 客户端路径 | `GET {业务根}/vimax/tv-show/mine` |
| 需登录 | JWT |

分页参数同列表；用于详情页「已发布」状态或「我的 TV Show」。

---

## 6. 错误码建议

在现有 `code` / `msg` 体系下约定业务语义即可，例如：

| 场景 | 建议 |
|------|------|
| 未登录 / JWT 无效 | 与现有账户体系一致（如 401） |
| 无权限操作他人作品 | 403 |
| 记录不存在 | 404 |
| 重复发布（若选策略 B） | 业务 code + 明确 msg |
| 参数校验失败 | 业务 code + 字段级 msg |
| OSS 相关失败 | 沿用现有 `presignPut` 错误 |

---

## 7. 客户端侧约束（对齐预期）

| 项 | 约定 |
|----|------|
| 可发布条件 | 本地 `status === "succeeded"` 且存在成片；封面建议必传 |
| 封面 | 本地相对路径 `cover`，发布前读文件 → 已有 OSS 直传 → `publicUrl` |
| 工程包 | 与「导出工程」相同 `.nomivimax`，**不是**单独 mp4 |
| 上传 | **仅**使用已有 `POST /uploads/oss/presignPut` + PUT，不新增上传 API |
| 作者 | 仅服务端从 JWT 取 |
| 成片预览 | 一期列表可只展示封面；成片在工程包内，二期再考虑单独 `previewVideoUrl` |

---

## 8. MVP 接口清单

| 优先级 | 接口 | 说明 |
|--------|------|------|
| P0 | 已有 `POST /uploads/oss/presignPut` | 封面 + 工程包上传 |
| P0 | `POST /vimax/tv-show/publish` | 写入结构化数据 |
| P0 | `GET /vimax/tv-show/list` | TV Show 广场列表 |
| P1 | `GET /vimax/tv-show/{id}` | 详情（含 packageUrl） |
| P1 | `DELETE` / `unpublish` | 下架 |
| P2 | `GET /vimax/tv-show/mine` | 我的发布 |

---

## 9. 本地 SessionRecord 字段对照（供映射）

客户端本地工程主要字段（发布时可能用到）：

| 本地字段 | 发布接口字段 |
|----------|--------------|
| `id` | `clientSessionId` |
| `title` | `title` |
| `workflow` | `workflow` |
| `style` | `style` |
| `target_duration_secs` | `targetDurationSecs` |
| `cover`（本地相对路径） | 上传后 → `coverUrl` |
| 导出 `.nomivimax` | 上传后 → `packageUrl` |
| `status` / `final_video` | 仅客户端门禁，不入库也可 |
| 作者 | JWT → `user_id` / `author` |

---

文档版本：2026-07-31  
状态：服务端待设计实现；客户端待 API 文档落地后对接。

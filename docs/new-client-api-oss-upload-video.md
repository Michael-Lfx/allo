# 新客户端 API 对接文档（OSS 直传 + 视频首尾帧）

> 本文档描述 C 端如何通过 **OSS 预签名 PUT** 上传图片，再将 **HTTPS 公网下载地址** 传入视频生成接口（首帧 / 尾帧 / 参考图），避免 base64 导致请求体过大。  
> 实现参考：`code/backend/internal/handlers/oss_presign.go`、`code/backend/internal/services/oss_presign.go`、`code/backend/docs/image-video-generation-api.md`；现有 Electron 用法见 `electron/workbench/video/uploads/presign-service.ts`。  
> **前置依赖**：用户登录与 JWT 见 [新客户端 API 对接文档（用户账户 & 激活上报）](./new-client-api-user-activation.md)。  
> **视频任务完整字段**：见 [生图 & 生视频 API](../code/backend/docs/image-video-generation-api.md)。

---

## 1. 为什么要先上传 OSS

视频生成接口虽支持 `data:image/...;base64,...`，但：

- Base64 约膨胀 **33%**，首尾帧两张图极易顶到厂商 **整包 ≤ 64MB** 限制；
- 大 JSON 也会加重客户端 → 业务网关 → 服务端 → 方舟的整条链路压力。

**推荐流程**：本地图片 → OSS 直传 → 用返回的 `publicUrl`（HTTPS）填入视频任务 `content[].image_url.url`。

```
登录获取 JWT
    ↓
POST /uploads/oss/presignPut     # 首帧、尾帧各申请一次
    ↓
HTTP PUT 到 data.url             # 直传对象存储（不经业务服务器转发文件体）
    ↓
记下 data.publicUrl
    ↓
POST /video/generations/tasks    # content 里用 HTTPS URL + role
    ↓
轮询 GET /video/generations/tasks/:id
```

---

## 2. 通用约定

### 2.1 Base URL

| 用途 | 客户端 Base（示例） |
|------|---------------------|
| 业务 API | `https://server.flowyaipc.cn/claw` |

路径映射：`{业务根}/uploads/oss/presignPut` → 服务端 `/api/v1/uploads/oss/presignPut`。

### 2.2 响应格式

业务 JSON：

```json
{ "code": 200, "msg": "操作成功", "data": { } }
```

客户端以 **`code === 200`** 判断成功。

### 2.3 认证差异（重要）

| 接口 | JWT | 用户 API Key（`flowy-...`） |
|------|-----|------------------------------|
| `POST /uploads/oss/presignPut` | ✅ **仅 JWT** | ❌ |
| `POST/GET/DELETE /video/generations/tasks...` | ✅ | ✅ |

因此：上传素材阶段必须用登录 JWT；创建视频任务可用 JWT 或 API Key。

推荐请求头（与现有客户端一致）：

```http
Content-Type: application/json
Authorization: Bearer <JWT>
token: <JWT>
```

---

## 3. 申请预签名上传

| 项 | 值 |
|----|-----|
| 客户端路径 | `POST {业务根}/uploads/oss/presignPut` |
| 服务端路径 | `POST /api/v1/uploads/oss/presignPut` |
| 需登录 | **仅 JWT** |

### 3.1 请求体

```json
{
  "fileName": "first-frame.png",
  "contentType": "image/png",
  "expiresSeconds": 900
}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `fileName` | 建议填 | 用于推导扩展名（如 `.png` / `.jpg`）。可为空，默认 `.bin`。扩展名仅允许 `[a-z0-9_-]`，长度 ≤ 16。 |
| `contentType` | 建议填 | 上传时必须与此一致；空则按 `application/octet-stream` 签名。图片建议 `image/png` / `image/jpeg` / `image/webp` 等。 |
| `expiresSeconds` | 否 | 预签名有效秒数。默认 **900**（15 分钟）；合法范围 **60 ~ 604800**（7 天）。 |

服务端会生成对象键（客户端无需自拼）：

```text
{presign_put_prefix}/presigned/{userId}/{yyyyMMdd}/{uuid}{ext}
```

### 3.2 成功响应

```json
{
  "code": 200,
  "msg": "操作成功",
  "data": {
    "method": "PUT",
    "url": "https://<bucket>.oss-cn-hangzhou.aliyuncs.com/claw/presigned/123/20260724/<uuid>.png?x-oss-credential=...&x-oss-signature=...",
    "expiresAt": "2026-07-24T04:00:00Z",
    "requiredHeaders": {
      "Content-Type": "image/png"
    },
    "objectKey": "claw/presigned/123/20260724/<uuid>.png",
    "publicUrl": "https://cdn.example.com/claw/presigned/123/20260724/<uuid>.png"
  }
}
```

| 字段 | 说明 |
|------|------|
| `method` | 固定按返回值执行，一般为 `PUT` |
| `url` | **预签名上传地址**（带查询参数签名，有过期时间）。仅用于 PUT 上传，**不要**当作模型可读的长期 URL |
| `expiresAt` | 预签名过期时间（RFC3339 / JSON 时间） |
| `requiredHeaders` | PUT 时**必须原样带上**的请求头（至少包含签名时的 `Content-Type`） |
| `objectKey` | OSS 对象键，排障用 |
| `publicUrl` | **给视频生成 API 用的 HTTPS 下载地址**（由服务端 `oss.cdn_base_url` + `objectKey` 拼出） |

> **注意**：若服务端未配置 `oss.cdn_base_url`，响应里可能没有 `publicUrl`。生产环境应已配置 CDN；客户端若拿不到 `publicUrl`，应视为配置错误并提示用户，不要自行猜 bucket 域名（桶可能非公网读）。

### 3.3 错误码（业务）

| 场景 | HTTP / 业务表现 | errorKey（参考） |
|------|-----------------|------------------|
| 未登录 / Token 无效 | 401 | — |
| Body 非法 | 400 | `error.invalid_request_body` |
| `expiresSeconds` 越界 | 400 | `error.oss.invalid_expires_seconds`（args 含 min/max） |
| `fileName` 扩展名非法 | 400 | `error.oss.invalid_file_name` |
| OSS 未配置 | 503 | `error.oss.presign_not_configured` |

---

## 4. 直传 OSS（客户端 → 对象存储）

对 **每一张** 首帧 / 尾帧 / 参考图：

1. 调用 §3 拿到 `data.url`、`data.method`、`data.requiredHeaders`、`data.publicUrl`
2. 在预签名过期前，用 **原始文件二进制** 调用：

```http
PUT {data.url}
Content-Type: image/png
（以及 requiredHeaders 中的其它键值）

<file bytes>
```

要点：

- **不要**用 `multipart/form-data`，也不要再包一层 JSON；
- `Content-Type` 必须与申请预签名时传入的 `contentType` / `requiredHeaders` **完全一致**，否则签名校验失败（常见 403）；
- 成功一般为 HTTP **200**（以 OSS 实际响应为准）；
- 上传成功后，保存对应的 **`publicUrl`**，供下一步创建视频任务使用。

### 4.1 伪代码示例

```typescript
async function uploadImageToOss(token: string, file: Blob, fileName: string) {
  const contentType = file.type || 'image/png';

  const presignRes = await fetch(`${BUSINESS_BASE}/uploads/oss/presignPut`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${token}`,
      token,
    },
    body: JSON.stringify({ fileName, contentType, expiresSeconds: 900 }),
  });
  const presignJson = await presignRes.json();
  if (presignJson.code !== 200) throw new Error(presignJson.msg || 'presign failed');

  const { url, method, requiredHeaders, publicUrl } = presignJson.data;
  if (!publicUrl) throw new Error('missing publicUrl (cdn not configured)');

  const putRes = await fetch(url, {
    method: method || 'PUT',
    headers: { ...requiredHeaders },
    body: file,
  });
  if (!putRes.ok) throw new Error(`oss put failed: ${putRes.status}`);

  return publicUrl as string;
}
```

---

## 5. 创建视频任务（HTTPS 首尾帧）

| 项 | 值 |
|----|-----|
| 客户端路径 | `POST {业务根}/video/generations/tasks` |
| 认证 | JWT 或 `flowy-` API Key |

将 §4 得到的 HTTPS 地址填入 `content`，**不要**再塞 base64：

```json
{
  "model": "flowy/doubao-seedance-1-0-pro-250528",
  "content": [
    {
      "type": "text",
      "text": "镜头从近景缓慢拉远，人物自然转头微笑"
    },
    {
      "type": "image_url",
      "image_url": {
        "url": "https://cdn.example.com/claw/presigned/123/20260724/aaaa.png"
      },
      "role": "first_frame"
    },
    {
      "type": "image_url",
      "image_url": {
        "url": "https://cdn.example.com/claw/presigned/123/20260724/bbbb.jpg"
      },
      "role": "last_frame"
    }
  ],
  "ratio": "16:9",
  "watermark": false,
  "duration": 5,
  "resolution": "720p",
  "generate_audio": true
}
```

| `content` 项 | `role` | 说明 |
|--------------|--------|------|
| `image_url` | `first_frame` | 首帧 |
| `image_url` | `last_frame` | 尾帧 |
| `image_url` | `reference_image` | 普通参考图（勿与首尾帧模式混用，以上游模型约束为准） |

成功响应：

```json
{
  "code": 200,
  "msg": "操作成功",
  "data": { "id": 12345 }
}
```

之后用本地任务 id 轮询：

```http
GET {业务根}/video/generations/tasks/12345
```

成片 URL 一般在 `data.result.content.video_url`（详见生视频文档）。

---

## 6. 客户端检查清单

- [ ] 上传走 `presignPut` + `PUT`，视频任务只传 `publicUrl`
- [ ] 首帧 / 尾帧各申请一次预签名、各 PUT 一次
- [ ] PUT 的 `Content-Type` 与预签名一致
- [ ] `publicUrl` 为空时不要继续创建视频任务
- [ ] 预签名过期前完成 PUT；过期需重新 `presignPut`
- [ ] OSS 接口只用 JWT；不要用 API Key 调 `presignPut`
- [ ] 图片格式 / 尺寸仍需满足上游 Seedance 要求（如常见 jpeg/png/webp，单张 &lt; 30MB 等）
- [ ] 避免把预签名 `url`（带签名 query）当作 `image_url.url` 传给视频 API

---

## 7. 相关文档

| 文档 | 内容 |
|------|------|
| [new-client-api-user-activation.md](./new-client-api-user-activation.md) | 登录 / JWT |
| [image-video-generation-api.md](../code/backend/docs/image-video-generation-api.md) | 生图 / 生视频完整参数与轮询 |

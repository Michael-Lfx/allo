# MiniMax-H3 视频生成 API 对接文档（客户端）

> 面向已接入 **Seedance（方舟）** 生视频的客户端：服务端已接入 MiniMax-H3，**HTTP 路径与任务生命周期不变**，按 `model` 自动分流。  
> 通用鉴权 / Base URL / Seedance 细节见 [image-video-generation-api.md](./image-video-generation-api.md)；服务端总览见 [API.md](../API.md) §1b。  
> 上游官方： [MiniMax V2 创建视频](https://platform.minimaxi.com/docs/api-reference/video-generation-v2-create)。

---

## 1. 一句话结论

| 项 | 说明 |
|----|------|
| 要不要换接口？ | **不用。** 仍用 `/video/generations/tasks` 一套 CRUD |
| 怎么切到 H3？ | `model` 改为 `flowy/MiniMax-H3`（或模型列表返回的同名 id） |
| 请求体要不要改？ | **要。** 对齐 MiniMax V2 schema；不要再依赖方舟专用字段（如 `watermark`） |
| 成片 URL？ | 继续读 `data.result.content.video_url`（服务端会把上游 `content.url` 归一化补齐） |
| 轮询？ | 流程相同；H3 在 `GET` 时会主动同步上游状态，状态更新更及时 |

---

## 2. 接口清单（与 Seedance 共用）

业务 Base（生产）：`https://server.flowyaipc.cn/claw`  
网关映射：`{业务根}/video/...` → 服务端 `/api/v1/video/...`

| Method | 客户端路径 | 鉴权 | 说明 |
|--------|-----------|------|------|
| `POST` | `/video/generations/tasks` | JWT 或 API Key | 创建任务 |
| `GET` | `/video/generations/tasks/:id` | 同上 | 查询（路径 `id` = 本地主键） |
| `GET` | `/video/generations/tasks` | 同上 | 列表（`page` / `pageSize`） |
| `DELETE` | `/video/generations/tasks/:id` | 同上 | 取消/删除 |

**推荐请求头：**

```http
Content-Type: application/json
Authorization: Bearer <token>
token: <token>
```

模型列表：`GET {业务根}/model/availableListClaw?category=4`（仅 JWT）。列表中应出现 H3（展示名多为 `MiniMax H3`），创建时 `model` 建议使用 `flowy/MiniMax-H3` 或列表返回的 `AIPC-...` / `flowy/...` 形式。

---

## 3. 对接流程（不变）

```
GET /model/availableListClaw?category=4
        ↓
POST /video/generations/tasks          → data.id（本地主键）
        ↓
每 3–10s 轮询 GET /video/generations/tasks/:id
        ↓
status ∈ {3,4,5,6} 终态
        ↓
成功(4)：取 data.result.content.video_url
```

创建成功响应：

```json
{
  "code": 200,
  "msg": "操作成功",
  "data": { "id": 12345 }
}
```

后续查询 / 删除 **只用** 本地 `data.id`，不要用上游 `task_id` 拼路径。

---

## 4. Seedance → H3：客户端必改项

| 维度 | Seedance（已有） | MiniMax-H3（新增） |
|------|------------------|-------------------|
| `model` | `flowy/doubao-seedance-...` | **`flowy/MiniMax-H3`** |
| 请求体规范 | 方舟「创建视频生成任务」 | **MiniMax V2 `/v2/video_generation`** |
| `resolution` | 方舟取值（如 `720p`） | **必填**：`768P` 或 `2K` |
| `duration` | 按方舟能力 | **必填**：整数 **4–15** 秒 |
| `ratio` | 常用 `16:9` 等 | 文生视频 **必填且不能为 `adaptive`**；图生多为 `adaptive` |
| `watermark` | 可用 | **勿依赖**：服务端丢弃；若需水印用 `aigc_watermark` |
| `generate_audio` / `seed` / `service_tier` / `negative_prompt` 等 | 方舟字段 | **勿原样照搬**；仅传 MiniMax 文档支持的字段 |
| 成片字段 | `result.content.video_url` | **同路径**（服务端从 `content.url` 补齐 `video_url`） |
| `GET` 轮询语义 | 仅读库（依赖回调） | 排队/运行中会 **先打上游再返回** |

服务端分流逻辑（客户端无需感知）：`model` → `model_category=4` 渠道模型；`channel_id=10`（minimax）走 H3，`channel_id=5`（ark）走 Seedance。

---

## 5. 创建任务请求体（H3）

服务端行为：

1. 用客户端 `model` 解析视频渠道模型；
2. 将 `model` **改写**为上游名 `MiniMax-H3`；
3. **丢弃** `watermark`、客户端自带的 `callback_url`；
4. 仅当服务端配置了 `minimax_video.callback_url` 时才写入回调（客户端勿自行填回调地址）。

### 5.1 必填 / 常用顶层字段

| 字段 | 必填 | 说明 |
|------|------|------|
| `model` | 是 | 客户端写 `flowy/MiniMax-H3`（或列表 id） |
| `content` | 是 | 多模态数组；**必须含一个非空 `text`** |
| `resolution` | 是 | `768P` \| `2K` |
| `duration` | 是 | `4`–`15`（整数秒） |
| `ratio` | 条件 | 见下表 |
| `aigc_watermark` | 否 | 是否加 AIGC 水印，默认 `false` |
| `callback_url` | 否 | **客户端不要传**；由服务端配置决定 |

**`ratio` 规则：**

| 场景 | `ratio` |
|------|---------|
| 文生视频（仅 `text`） | **必填**，且不能为 `adaptive`：`21:9` / `16:9` / `4:3` / `1:1` / `3:4` / `9:16` |
| 图生视频（含 `first_frame` / `last_frame`） | 实际由图片决定；传 `adaptive` 即可（其它值会被忽略） |
| 多模态参考（`reference_*`） | 可选，默认 `adaptive`，也可指定具体比例 |

### 5.2 `content[]` 项

| `type` | 结构 | `role`（按场景） |
|--------|------|------------------|
| `text` | `{ "type":"text", "text":"..." }` | —；单条最多约 7000 字符 |
| `image_url` | `{ "type":"image_url", "image_url":{ "url":"..." }, "role":"..." }` | `first_frame` / `last_frame` / `reference_image` |
| `video_url` | `{ "type":"video_url", "video_url":{ "url":"..." }, "role":"reference_video" }` | 仅多模态参考 |
| `audio_url` | `{ "type":"audio_url", "audio_url":{ "url":"..." }, "role":"reference_audio" }` | 仅多模态参考 |

**互斥：** `first_frame`/`last_frame` 与 `reference_image`/`reference_video`/`reference_audio` **不可混用**。

**媒体建议：** 优先公网 HTTPS（可先 `POST {业务根}/uploads/oss/presignPut` 再传 `publicUrl`）。请求体总大小 ≤ 64MB；大文件勿用 base64。

### 5.3 示例

**文生视频：**

```json
{
  "model": "flowy/MiniMax-H3",
  "content": [
    {
      "type": "text",
      "text": "史诗级太空歌剧院线预告：女舰长独自站在巨大观景窗前，舰队跃迁离去，强光爆闪。"
    }
  ],
  "resolution": "2K",
  "duration": 5,
  "ratio": "16:9"
}
```

**图生视频（首帧）：**

```json
{
  "model": "flowy/MiniMax-H3",
  "content": [
    {
      "type": "text",
      "text": "镜头推向背景人物，碗中蒸汽更浓。"
    },
    {
      "type": "image_url",
      "image_url": {
        "url": "https://example.com/first-frame.png"
      },
      "role": "first_frame"
    }
  ],
  "resolution": "2K",
  "duration": 5,
  "ratio": "adaptive"
}
```

**首尾帧：**

```json
{
  "model": "flowy/MiniMax-H3",
  "content": [
    { "type": "text", "text": "从白天过渡到黄昏，镜头缓慢推进。" },
    {
      "type": "image_url",
      "image_url": { "url": "https://example.com/first.png" },
      "role": "first_frame"
    },
    {
      "type": "image_url",
      "image_url": { "url": "https://example.com/last.png" },
      "role": "last_frame"
    }
  ],
  "resolution": "768P",
  "duration": 6,
  "ratio": "adaptive"
}
```

**多模态参考（示意）：**

```json
{
  "model": "flowy/MiniMax-H3",
  "content": [
    { "type": "text", "text": "角色说话：Follow the wind, live free. 音色参考音频1" },
    {
      "type": "video_url",
      "video_url": { "url": "https://example.com/ref.mp4" },
      "role": "reference_video"
    },
    {
      "type": "audio_url",
      "audio_url": { "url": "https://example.com/ref.mp3" },
      "role": "reference_audio"
    }
  ],
  "resolution": "2K",
  "duration": 5,
  "ratio": "adaptive"
}
```

---

## 6. 查询任务响应

```json
{
  "code": 200,
  "msg": "操作成功",
  "data": {
    "id": 12345,
    "task_id": "424010985738629",
    "status": 4,
    "result": {
      "id": "424010985738629",
      "model": "MiniMax-H3",
      "status": "succeeded",
      "content": {
        "url": "https://cdn.example.com/out.mp4",
        "video_url": "https://cdn.example.com/out.mp4"
      },
      "resolution": "2K",
      "duration": 5,
      "ratio": "16:9",
      "usage": {
        "total_seconds": 5,
        "input_seconds": 0,
        "output_seconds": 5,
        "input_image_count": 0
      }
    },
    "created_at": "2026-08-19T06:00:00Z",
    "updated_at": "2026-08-19T06:01:20Z"
  }
}
```

| 字段 | 说明 |
|------|------|
| `id` | 本地 `tb_video_task.id` |
| `task_id` | 上游任务 id（创建响应字段为 `task_id`） |
| `status` | 本地状态码（下表） |
| `result` | 上游快照（已剥内部 `_flowy`）；H3 成功时保证可读 `content.video_url` |

**本地 `status`（Seedance / H3 共用）：**

| 值 | 含义 |
|----|------|
| `1` | 排队 |
| `2` | 生成中 |
| `3` | 已取消 |
| `4` | 成功 |
| `5` | 失败 |
| `6` | 过期 |

**客户端取片：** 优先 `data.result.content.video_url`；兼容可读 `data.result.content.url`。

**列表：** `GET /video/generations/tasks?page=1&pageSize=10` → `data.list` + `data.total`，元素结构同单条查询。

---

## 7. 取消 / 删除

`DELETE /video/generations/tasks/:id`

- 成功：本地置为 `status=3`；已取消再删为幂等成功。
- 创建期失败占位任务（`task_id` 形如 `local-fail-*`）：仅改本地状态，不打上游。

| HTTP | code | errorKey | 场景 |
|------|------|----------|------|
| 404 | 404 | `error.video_task_not_found` | 不存在或非本人 |
| 409 | 409 | `error.video_task.delete_running` | 仍在生成，上游拒绝 |
| 409 | 409 | `error.video_task.delete_conflict` | 上游冲突 |
| 502 | 502 | `error.seedance_upstream_failed` | 上游失败（错误 key 与 Seedance 共用命名） |

---

## 8. 错误码（创建 / 查询）

业务包装：`{ "code", "msg", "data"? }`。`msg` 为 i18n 文案。

| HTTP | code | errorKey | 场景 |
|------|------|----------|------|
| 400 | 400 | `error.invalid_param` | 模型未找到等（如 `field=model`） |
| 400 | 400 | `error.seedance_request_rejected` | 上游 **4xx**（参数/敏感内容等） |
| 402 | 402 | `error.insufficient_credit` | 积分不足 |
| 404 | 404 | `error.video_task_not_found` | 任务不存在 |
| 502 | 502 | `error.seedance_upstream_failed` | 上游 **5xx** / 其它失败 |

创建期上游失败仍会落库（`status=5`），错误响应的 `data` 可能带 `{ "id": <本地主键> }`，便于列表回看。无上游 id 时 `task_id` 为 `local-fail-<uuid>`。

---

## 9. 积分预检与扣费（客户端需知）

| 阶段 | 规则 |
|------|------|
| 创建前 | `duration`（秒，向上取整）× **1000** 积分预检；H3 另加 **输入图片** 预检（见下） |
| 成功态 | 按上游 usage / 渠道单价扣成片费；H3 叠加输入图片费用 |

**H3 输入图片：**

| 规则 | 说明 |
|------|------|
| 统计 | 创建请求 `content[]` 中 `type=image_url` 的项 |
| 免费 | 前 **5** 张免费 |
| 超出 | 每张按假 token + `InputPrice` 计费（种子 `InputPrice=17` 时超额约 **170** 积分/张） |
| 不计费 | `video_url` / `audio_url` 参考素材不走本条 |

积分不足会在创建阶段直接 **402**，不会进入上游。

---

## 10. 与 Seedance 的差异速查（给改造同学）

```
已有 Seedance 客户端改造清单：

□ 模型选择 UI 增加 MiniMax-H3（category=4）
□ 创建 body builder 按 model 分支：
    - Seedance → 方舟字段（可含 watermark 等）
    - H3 → MiniMax V2：必填 resolution/duration；文生必填非 adaptive ratio
□ 去掉对 H3 的 watermark / generate_audio 等方舟字段透传
□ 轮询 / 取片逻辑可复用：local id + status + content.video_url
□ 多图素材时提示积分（>5 张有额外费用）
□ 错误码 key 无需改（仍为 seedance_* 命名）
```

**回调：** 客户端无需对接。运维若配置 `minimax_video.callback_url`，服务端会处理 MiniMax 的 `challenge` 校验与 `task` 推送；未配置时完全依赖客户端 `GET` 触发上游同步。

---

## 11. TypeScript 轮询示例（H3）

```typescript
const BUSINESS_BASE = 'https://server.flowyaipc.cn/claw';

function headers(token: string) {
  return {
    'Content-Type': 'application/json',
    Authorization: `Bearer ${token}`,
    token,
  };
}

async function createAndPollH3(token: string) {
  const createRes = await fetch(`${BUSINESS_BASE}/video/generations/tasks`, {
    method: 'POST',
    headers: headers(token),
    body: JSON.stringify({
      model: 'flowy/MiniMax-H3',
      content: [{ type: 'text', text: '海边日落，镜头缓慢推进' }],
      resolution: '2K',
      duration: 5,
      ratio: '16:9',
    }),
  });
  const created = await createRes.json();
  if (created.code !== 200) throw new Error(created.msg);

  const localId = created.data.id as number;
  for (;;) {
    await new Promise((r) => setTimeout(r, 5000));
    const pollRes = await fetch(`${BUSINESS_BASE}/video/generations/tasks/${localId}`, {
      headers: headers(token),
    });
    const polled = await pollRes.json();
    if (polled.code !== 200) throw new Error(polled.msg);

    const { status, result } = polled.data;
    if (status === 4) {
      return (result?.content?.video_url ?? result?.content?.url) as string;
    }
    if (status === 5 || status === 6) throw new Error('failed or expired');
    if (status === 3) throw new Error('cancelled');
  }
}
```

---

## 12. 源码索引

| 模块 | 路径 |
|------|------|
| 路由 | `internal/routes/routes.go` |
| HTTP Handler | `internal/handlers/seedance_video.go` |
| Seedance + 分流 | `internal/services/model_proxy/seedance_video.go` |
| MiniMax-H3 | `internal/services/model_proxy/minimax_h3_video.go` |
| 渠道/模型种子 | `sql/0056/data.sql`（`channel_id=10`，模型 `MiniMax-H3`） |
| 配置 | `minimax_video.callback_url` / `minimax_video.http_timeout` |

---

## 13. 对接检查清单

- [ ] 登录取得 JWT 或 `flowy-` API Key
- [ ] `GET .../availableListClaw?category=4` 能看到 H3
- [ ] 创建使用本地返回的 `data.id` 轮询 / 删除
- [ ] H3 请求体：`content` + `resolution` + `duration`（文生另加合法 `ratio`）
- [ ] 不向 H3 透传方舟专用字段；不由客户端自填 `callback_url`
- [ ] 成功取 `result.content.video_url`
- [ ] 处理 `400/402/404/409/502` 与创建失败落库的 `data.id`

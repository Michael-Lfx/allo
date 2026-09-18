# ViMax 活动营销 API 文档（客户端对接）

> 本文档覆盖视频生成模块的活动营销：首页轮播、活动列表、活动详情、用户投稿、获奖作品。  
> 投稿复用 TV Show 审核流（`/api/v1/vimax/tv-show/*`），不另建投稿表。  
> 上传仍复用 `POST /api/v1/uploads/oss/presignPut`。  
> 运营配置入口：运营平台 → 营销活动 → 视频活动；审核/评奖入口：系统管理 → TV Show 审核/评奖。

---

## 1. 产品模型（先读这一节）

市面主流短剧/视频平台（剪映、CapCut、可灵、即梦等）把「广告位」和「活动」做成**同一活动实体的两种展示面**，而不是两套配置：

| 展示面 | 字段 | 客户端用法 |
|--------|------|-----------|
| 轮播 | `showInCarousel=true` + 轮播图/短视频 | 视频生成首页顶部 Banner |
| 活动列表 | `showInList=true` | 「活动」页卡片列表 |
| 投稿 | `allowSubmission=true`（必须同时 `showInList=true`） | 详情页「立即参与」 |

**仅轮播活动**：`showInCarousel=true` 且 `showInList=false`。不出现在活动列表，不可投稿，点击可走 `linkUrl`（若有）。用于品牌/功能宣发。

**正式活动**：`showInList=true`，可配轮播，可开放投稿。用户投稿后进入 TV Show 待审；运营通过后出现在该活动作品流，并可评奖。

```
运营创建活动（草稿）
        │
        ▼
     上架 published
        │
        ├── 轮播：status=published 且 showInCarousel 且 当前时间在 [startAt, endAt]
        ├── 列表：status=published 且 showInList 且 endAt >= now（含未开始）
        └── 投稿：published 且 allowSubmission 且 showInList 且 startAt <= now <= endAt
```

`phase` 由服务端按当前时间计算，客户端不要自己猜：

| phase | 含义 |
|-------|------|
| `upcoming` | 未开始 |
| `ongoing` | 进行中 |
| `ended` | 已结束 |

---

## 2. 认证与统一响应

与 TV Show 相同：

- Header：`Authorization: Bearer <C 端 JWT>`
- 成功：`{ "code": 200, "msg": "...", "data": { } }`，以 `code === 200` 为准

路径约定：客户端业务根 + `/vimax/campaigns/...` ↔ 服务端 `/api/v1/vimax/campaigns/...`。

---

## 3. C 端接口

### 3.1 首页轮播

- **URL**: `/api/v1/vimax/campaigns/carousel`
- **Method**: `GET`
- **Auth**: C 端 JWT

返回当前有效轮播（已上架、已勾选轮播、在活动期内、已配置媒体），最多 20 条，按 `carouselSort` 降序。

**Success `data`**

```json
{
  "list": [
    {
      "id": 1001,
      "title": "夏日短剧挑战",
      "mediaType": "video",
      "mediaUrl": "https://cdn.example.com/.../banner.mp4",
      "posterUrl": "https://cdn.example.com/.../poster.jpg",
      "linkUrl": null,
      "showInList": true,
      "allowSubmission": true,
      "canSubmit": true,
      "startAt": "2026-08-01T00:00:00.000Z",
      "endAt": "2026-08-31T16:00:00.000Z",
      "phase": "ongoing"
    }
  ]
}
```

**点击规则（按优先级）**

1. `showInList === true` → 打开活动详情（`GET /campaigns/{id}`）
2. 否则若有 `linkUrl` → 打开外链 / 深链
3. 否则仅展示，不跳转（纯宣发轮播）

`mediaType=video` 时用 `posterUrl` 做首帧，自动播放需静音 + 循环，划走即停。

---

### 3.2 活动列表

- **URL**: `/api/v1/vimax/campaigns/list`
- **Method**: `GET`
- **Auth**: C 端 JWT

**Query**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `page` | int | 否 | 默认 1 |
| `pageSize` | int | 否 | 默认 20，上限 50 |
| `includeEnded` | bool | 否 | `true` 时包含已结束活动（往期） |

默认：**已上架 + 出现在列表 + 尚未结束**（含未开始的预热活动）。仅轮播活动不会出现在这里。

**Success `data`**

```json
{
  "total": 8,
  "page": 1,
  "pageSize": 20,
  "list": [
    {
      "id": 1001,
      "title": "夏日短剧挑战",
      "summary": "用 30 秒讲一个夏天的故事",
      "coverUrl": "https://cdn.example.com/.../cover.png",
      "showInCarousel": true,
      "showInList": true,
      "allowSubmission": true,
      "canSubmit": true,
      "startAt": "2026-08-01T00:00:00.000Z",
      "endAt": "2026-08-31T16:00:00.000Z",
      "phase": "ongoing",
      "listSort": 10
    }
  ]
}
```

卡片建议：封面、标题、摘要、时间、`phase` 标签；`canSubmit=true` 显示「可投稿」。列表**不含**正文 HTML。

---

### 3.3 活动详情

- **URL**: `/api/v1/vimax/campaigns/{id}`
- **Method**: `GET`
- **Auth**: C 端 JWT

任意已上架活动均可按 ID 打开（含仅轮播、已结束），便于深链。未上架返回 `error.campaign.not_found`。

在列表字段基础上增加：

| 字段 | 说明 |
|------|------|
| `content` | 富媒体正文，**HTML**。用 WebView / 富文本组件渲染，图片与 `<video>` 需自适应宽度 |
| `canSubmit` | 当前是否允许该用户投稿（综合上架、列表展示、投稿开关、时间窗） |

`upcoming`：展示倒计时，投稿按钮禁用。  
`ended`：隐藏投稿，展示获奖作品区。

---

### 3.4 活动作品流（已审核上架）

- **URL**: `/api/v1/vimax/campaigns/{id}/submissions`
- **Method**: `GET`
- **Auth**: C 端 JWT

结构与 TV Show 广场列表相同（封面、作者、点赞等），且带 `campaignId` / `awardLevel`。  
Query 同 TV Show：`page` / `pageSize` / `workflow` / `keyword` / `sort`。

---

### 3.5 获奖作品

- **URL**: `/api/v1/vimax/campaigns/{id}/winners`
- **Method**: `GET`
- **Auth**: C 端 JWT

只返回该活动下 **已上架且已评奖** 的作品，按 `awardSort DESC, awardedAt ASC`。

`awardLevel`：`first` / `second` / `third` / `merit` / `featured`  
展示优先用 `awardLabel`（运营自定义文案，如「最佳创意」）。

---

## 4. 投稿（复用 TV Show）

活动投稿 **不要** 新建接口，调用现有：

```
POST /api/v1/vimax/tv-show/publish
{
  "clientSessionId": "...",
  "campaignId": 1001,
  "...其余字段与广场发布相同"
}
```

规则：

- 不传或 `campaignId=0`：发到 TV Show 广场
- 同一工程可同时投广场和多个活动（幂等键含 `campaignId`）
- 同一活动同一工程再次发布：更新素材并重新进入 `pending`，清空旧评奖
- 活动未上架、仅轮播、未到开始、已结束、未开放投稿 → `error.campaign.not_open` 或 `error.campaign.submission_closed`

用户侧状态仍走 TV Show：

| 接口 | 用途 |
|------|------|
| `GET /vimax/tv-show/mine?campaignId=1001` | 该活动下我的投稿 |
| `GET /vimax/tv-show/{id}` | 详情 / 下载工程包 |
| `DELETE /vimax/tv-show/{id}` | 撤回自己的投稿 |

审核通过前不会出现在活动作品流。

---

## 5. 错误码

| HTTP | errorKey | 场景 |
|------|----------|------|
| 404 | `error.campaign.not_found` | 活动不存在或未上架 |
| 409 | `error.campaign.not_open` | 活动未上架，不能投稿 |
| 409 | `error.campaign.submission_closed` | 不在投稿期 / 未开放投稿 / 仅轮播 |
| 400 | `error.invalid_param` | 参数非法 |
| 其余 TV Show 错误 | 见 `vimax-tv-show-api.md` | 投稿/点赞/删除 |

---

## 6. 客户端实现方案

### 6.1 信息架构

建议视频生成模块增加两个入口，不要把活动塞进「最近创作」：

```
视频生成首页
├── 顶部轮播（GET /campaigns/carousel）
├── 最近创作（本地）
├── TV Show 广场（GET /tv-show/list）
└── 活动（GET /campaigns/list） → 活动详情
        ├── 正文 HTML
        ├── 立即参与（本地成片 → OSS → publish + campaignId）
        ├── 活动作品流
        └── 获奖作品（有数据才展示）
```

### 6.2 首页轮播

1. 进入首页请求 carousel；失败不阻断首页，隐藏 Banner。
2. `mediaType=image`：全宽图片，圆角，可点。
3. `mediaType=video`：封面帧 + 静音循环播放，停留超过 1 屏再播，离开暂停。短视频建议 3–15 秒。
4. 点击按第 3.1 节规则跳转。
5. 轮播间隔 4–5 秒；仅 1 条时不显示指示点。

### 6.3 活动列表

- 进行中 / 未开始用 `phase` 标签区分；往期用 `includeEnded=true` 二级 Tab。
- 下拉刷新 + 分页。空态：「暂无活动」。

### 6.4 活动详情

- `content` 用 WebView 渲染 HTML；注入样式让 `img, video { max-width: 100%; }`。
- 主按钮文案：
  - `canSubmit`：立即参与
  - `phase=upcoming`：活动未开始（禁用）
  - `phase=ended`：查看获奖作品
  - 已投过：去查看我的投稿（`GET /tv-show/mine?campaignId=`）
- 参与：只允许 `status=succeeded` 的本地工程；上传封面+工程包后 `publish`。

### 6.5 投稿体验

与现有 TV Show 发布尽量同一套 UI，仅多带 `campaignId`：

1. 详情页「立即参与」→ 选择已完成工程（或去创作）
2. OSS 直传封面与 `.nomivimax` / `.nomiccanvas`
3. `POST /tv-show/publish` + `campaignId`
4. 成功后进入「审核中」；`mine` 轮询或下次进入刷新
5. `offline` 展示 `rejectReason`，允许改完再投（同一 `clientSessionId` 幂等更新）

### 6.6 获奖区

- 详情页在作品流上方展示 winners；无数据则整区隐藏。
- 卡片角标用 `awardLabel`。
- 点赞/导入工程仍走 TV Show `like` / 详情 `packageUrl`。

### 6.7 缓存建议

| 数据 | 建议 |
|------|------|
| carousel | 内存缓存 60s，首页可见时刷新 |
| 活动列表 | 进入页面拉新 |
| 详情 HTML | 按 `id + updatedAt` 缓存，避免每次下载体量较大的正文 |

不要在客户端过滤「仅轮播」——服务端列表已经排除。

### 6.8 对接清单

| 步骤 | 说明 |
|------|------|
| 1 | 首页接入 `/campaigns/carousel` |
| 2 | 活动 Tab 接入 `/campaigns/list` |
| 3 | 详情页渲染 `content` HTML + 时间/phase |
| 4 | `canSubmit` 时走 TV Show 发布并传 `campaignId` |
| 5 | 作品流 `/campaigns/{id}/submissions`，获奖 `/winners` |
| 6 | 「我的投稿」用 `/tv-show/mine?campaignId=` |
| 7 | 联调前确认运营已上架测试活动，且时间窗覆盖当前时刻 |

---

文档版本：2026-08-29  
状态：服务端与运营平台已实现；客户端按本文档对接。

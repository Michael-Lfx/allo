# 云服务与计费域（Flowy Cloud）

> **最后维护：** 2026-08-24 · 核对基准：commit `d791691c6` ·
> 文档性质：现行架构文档（新建，基于源码逐项核对）

[`nomifun-cloud`](../../crates/backend/nomifun-cloud/) 是"远程 LLM 服务器客户端"：
只做云登录与 OpenAI 兼容推理网关调用，agent 逻辑全部留在本地。它是云登录、
云端模型目录、积分与内购计费四件事的后端支点。

## 双身份车道

| 车道 | 认证 | 归属 |
| --- | --- | --- |
| 本地身份 | JWT + CSRF（`nomifun-auth`，`/api/auth/*`） | 本机管理员 / 家庭成员 |
| 云身份 | Flowy 云 JWT（微信扫码或邮箱 OTP） | `ServerSession` / `SERVER_TOKEN_PROVIDER` |

关键规则：**云 JWT 只存 Rust 侧** —— 加密落盘于
`<data_dir>/auth/tokens.json`（`FileTokenStore`，可用 `NOMIFUN_TOKEN_STORE_KEY_B64`
或环境变量 `NOMIFUN_SERVER_TOKEN` 覆盖），渲染进程永远拿不到。云登录入口必须先过
本地登录：前端 `/cloud-login`（重复入口 `/settings/cloud-login`）由
`CloudAuthContext` 驱动（whoami 轮询、offline 容忍、失效重定向）。

后端路由挂载在 `/api/cloud/*` 且套 `auth_middleware`：`login/start` /
`login/continue` / `logout` / `whoami` / `settings` / `website-entry` /
`device/status|activate`（设备指纹 + GeoIP，尽力而为）/ `sync-models`。

## 云端模型目录

- 云模型在本地 provider 表中表现为**一个内置 provider 行**
  （`FLOWY_BUILTIN_PROVIDER_ID`）；模型列表是上游 `availableListClaw` 目录的投影
  （chat=1、ASR=7、TTS=8）。
- 同步语义（契约见 [`model-catalog-lifecycle.zh.md`](model-catalog-lifecycle.zh.md)）：
  拉取成功时**整体替换**成员关系（下架模型即消失）；瞬时失败保留上次完好状态。
  `login/continue` ≠ 模型同步；前端编排 `/api/cloud/sync-models` → provider SWR →
  resolver → 默认模型校验；历史会话与默认模型从不被改写；wire 上刻意没有
  `model_sync_required` 字段。
- 推理路由：`server.enabled=true` 时 LLM 请求经 `ServerLlmProvider`
  （OpenAI 兼容 `base_url + /v1/chat/completions` + Bearer 云 JWT）；否则直连各
  provider。
- 注意：shared 层的 [`nomifun-models-dev`](../../crates/shared/nomifun-models-dev/)
  是第三方 models.dev 注册表富化通道，与 Flowy 云目录无关。

## 积分与计费

两套相邻但不同的面：

- **积分**（消费计量）：由 [`nomifun-media`](../../crates/backend/nomifun-media/) 暴露
  `GET /api/media/credits`（余额）、`POST .../checkin`（按时区每日打卡）、
  `GET .../usage-by-turn`（每回合 prompt/completion/cache token 与积分明细）；
  前端 `CreditsContext` 消费，未登录云时按回合芯片隐藏。
- **内购计费**（花钱买）：`/billing` 路由 → `pages/billing/` 应用内结账向导
  （catalog → confirm → pay → success），USD 套餐 + 积分包；
  `AirwallexDropIn.tsx` 用 `@airwallex/components-sdk` drop-in 卡片组件
  （`VITE_AIRWALLEX_ENV`，默认 prod）。**已完整实现**（非纯设计稿）：后端
  `/api/cloud/*` 下有 plans / credit-packs / coupons / payment-channels / orders /
  orders/:orderNo/airwallex/init 等处理器并带测试；待支付订单号暂存
  sessionStorage `flowy.billing.pendingOrderNo`，云登录回跳白名单仅 `/billing`。
  设计稿：
  [`superpowers/specs/2026-08-21-desktop-airwallex-billing-design.md`](../superpowers/specs/2026-08-21-desktop-airwallex-billing-design.md)。
  托管式 `redirectToCheckout` 明确不在范围内（仅 drop-in）。

## 配置面

`GatewayConfig`（持久化 `<data_dir>/config.yaml`，定义在
[`nomi-config/src/gateway.rs`](../../crates/agent/nomi-config/src/gateway.rs)）下的
`server` 段：`enabled`（默认 false）、`base_url`（空 →
`https://server.flowyaipc.com/claw`；遗留 `.cn` 域名自动改写）、`channel`（flowy/gmk）、
`auth`（首选登录方式 / 轮询间隔 / OTP TTL）、`llm`（path_prefix / 默认模型 /
超时）。相关环境变量：`NOMIFUN_SERVER_TOKEN`、`NOMIFUN_SERVER_ENABLED`、
`NOMIFUN_SERVER_URL`、`NOMIFUN_TOKEN_STORE_KEY_B64`、`NOMIFUN_ENABLE_FREE_MODELS`、
`NOMIFUN_SKIP_GEOIP`、前端 `VITE_AIRWALLEX_ENV`。

## 消费者

后端：`nomifun-app`（挂载服务与路由）、`nomifun-media`、`nomifun-vimax`、
`nomifun-canvas`、`nomifun-shell`、`nomifun-insights`；引擎侧：`nomi-media`、
`nomi-vimax`（媒体/视频生成走 Flowy 云后端的公共依赖，见
[media-creation.zh.md](media-creation.zh.md) 与 [poi-insights.zh.md](poi-insights.zh.md)）。

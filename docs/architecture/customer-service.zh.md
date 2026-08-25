# 客服独立域（Customer Service）

> **最后维护：** 2026-08-24 · 核对基准：commit `d791691c6` ·
> 文档性质：现行架构文档（新建，基于源码逐项核对）

[`nomifun-customer-service`](../../crates/backend/nomifun-customer-service/) 是面向
IM 渠道陌生人的独立客服域。设计原则：**与伙伴 / 会话体系不共享任何领域概念** ——
对话是本域自己的聚合，回复由一次性引擎会话产出，工具注册表固定为只读三件套。

## 核心概念

- 身份通道：`CsDialogueKey { channel_plugin_id, channel_user_id, chat_id }` ——
  没有"会话"概念，只有按三元组唯一定位的对话（`cs_dialogues` 表）。
- 并发控制：每 agent `max_concurrent` 信号量（1..=64，默认 8）+ 每通道串行锁 +
  pending 缓冲合并。
- 模块：`dialogue.rs`（无状态并发 turn 引擎 `CsDialogueEngine` /
  `LiveTurnRunner`）、`service.rs`（CRUD/校验）、`routes.rs`、`tools.rs`。

## 一次性引擎会话

回复由 [`nomifun-ai-agent/src/one_shot.rs`](../../crates/backend/nomifun-ai-agent/src/one_shot.rs)
的 `run_one_shot_turn` 产生 —— 走 provider 直连路径，**不是**完整 nomi 引擎会话：

- 工具表只含构造时白名单 `req.tools`，fail-closed；
- 固定三件套只读工具（`tools.rs::build_cs_tools`）：`knowledge_search`（limit 8）、
  `knowledge_read`（知识库白名单校验路径，6000 字符截断）、`cs_notes_search`
  （LIKE 检索，limit 10；MVP 无 FTS）；
- 无 skills / MCP / 文件系统 / workspace 租约；每 turn 无状态；
- 限制：≤8 轮工具调用、4096 max tokens、120 s turn 超时、上下文窗口
  30 条消息 / 8000 字符；
- 失败兜底：固定话术"暂时无法回复，请稍后再试" + `turn_error` 审计行。

## 存储与路由

迁移 `019_customer_service.sql` 六张表：`cs_agents`、`cs_channel_bindings`
（每渠道插件唯一绑定）、`cs_dialogues`、`cs_messages`、`cs_notes`
（共享 NULL agent / 私有）、`cs_audit_events`。仓储 trait
`ICustomerServiceRepository`。

HTTP 面：绝对路径 `/api/customer-service/*`（agents CRUD、bindings GET/PUT、
notes CRUD、监控读 `GET /dialogues?cs_agent_id=` 与
`GET /dialogues/{id}/messages`），经 `protect_instance_owner` 保护。**无 WS 事件**
—— 回复经由原 IM 渠道发送器返回。

## 渠道路由

渠道 crate 保持域无关：trait `CsRouting` 定义在 `nomifun-channel`，由 app 层
`AppCsRouting` 实现。路由由**机器人配置**决定：

- 绑定查询 `binding_for(channel_plugin_id)` → `cs_channel_bindings`，且绑定时强制
  `channel_plugins.owner_domain='customer_service'`；
- cs 绑定的 bot 自动服务陌生人（跳过伙伴配对门）；未绑定 bot 维持配对流程；
- 消息循环在进入 conversation 路径前短路（空回复 = 批量合并后不发送）；
- 隔离硬约束：迁移 `021_channel_owner_domain.sql` + 守卫触发器保证 cs bot 永远
  不能携带 companion_id，owner_domain 创建后不可变。

## 前端

路由 `/customer-service`（花名册）与 `/customer-service/:cs_agent_id`（详情，
`pages/customerService/`）。可管理：agent 启停/删除、问候语/人设/服务策略、
provider + 模型、知识库挂载、max_concurrent、渠道 bot 绑定（可内联创建 cs 域 bot）、
notes CRUD。

## MVP 现状与已知限制

- 对话监控仅有 API + ipcBridge（`listDialogues` / `listDialogueMessages`），
  尚无 UI 页面消费；
- 审计事件已持久化但未暴露 HTTP 读面；
- `knowledge_read` 6000 字符截断、notes LIKE 检索均为显式 MVP 取舍；
- `routes.rs:124` 注释写"since migration 020"，实际 owner-domain 迁移是
  `021_channel_owner_domain.sql`（代码内注释待修）。

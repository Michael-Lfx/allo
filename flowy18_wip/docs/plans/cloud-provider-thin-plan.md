# 云端单一 Provider 接入 — 最小实现方案

- 分支：`feat/flowy18-wip-windows-launch`
- 日期：2026-11-16
- 状态：**可行性已实测验证**，待实现
- 范围：只做 provider 接入（模型目录 + 账号绑定）。LLM 对话内核改动留给下一个分支。

---

## 1. 结论先行

**可行，而且比预想的薄。**

用当前已登录的账号 JWT，直接请求云端模型目录，一次通过：

```
GET https://server.flowyaipc.com/claw/api/v2/model/availableListClaw?category=1&app=flowymes
Authorization: Bearer <jwt>
→ HTTP 200, code=200, msg=Success
→ auto=3, cloud=12, effectiveModelTier=0
```

无设备激活、无二次鉴权、无积分兑换门槛。15 条模型全部 `minTierLevel=0`（当前账号无档位限制）。

样本字段（第一条 cloud 模型）：

```json
{
  "id": "AIPC-deepseek-v4-flash",
  "name": "Deepseek-v4-flash",
  "endpoint": "https://server.flowyaipc.com/claw/v1",
  "extra": "{\"input\":[\"text\"],\"reasoning\":true,\"max_tokens\":384000,
            \"credit_rate\":0.5,\"context_window\":1000000,
            \"reasoning_effort\":[\"low\",\"medium\",\"xhigh\"]}"
}
```

---

## 2. 目标与非目标（做减法的边界）

### 2.1 本期要做

1. 从云端目录拉取模型列表，唯一数据源是账号体系
2. 软件**只有一个 provider**，固定为 `flowy`，用户不可自定义添加
3. 模型选择改为「目录里的真实模型 id」，替换现在的硬编码 4 档位
4. 选择持久化到 `preferences.json`
5. 提供可测试通过的路径（单测 + 手动验证）

### 2.2 本期不做（明确砍掉）

1. **LLM 对话**：不改 `dsh-rs`，不接 `openai_compatible_adapter`，对话仍是 mock —— 下一个分支
2. **BYOK / Flex**：不做自定义 provider（现有 Flex 入口本期只隐藏，物理删除排到二期）
3. **档位门控**：当前账号 `effectiveModelTier=0`、全部模型 `tier=0`，本期只解析字段不渲染锁
4. **多分类**：只取 `category=1`（聊天）；ASR/TTS/图片/视频都不碰
5. **auto 路由**：`auto` 3 条是路由策略不是具体模型，本期只取 `cloud` 12 条
6. **计费展示**：`credit_rate` 只解析不展示

---

## 3. 最薄架构

三层，每层只做一件事：

```
flowy-auth          拉取 + 解析（唯一数据源）
   ↓  Vec<CloudModel>
flowy-agent         承载选择状态（模型 id + 目录快照）
   ↓  model_id: String
flowy-app / studio  渲染选择器 + 持久化到 prefs
```

**关键决定：不做 provider 抽象。** 没有 trait、没有注册表、没有插件。因为只有一个 provider，抽象层是纯负担。

---

## 4. 改动清单（按依赖顺序）

### 第 1 层：`flowy-auth`（数据源）

1. **新增** `crates/flowy-auth/src/model_catalog.rs`（约 110 行）
   - `pub struct CloudModel { id, name, endpoint, context_window, max_tokens, credit_rate }`
   - `pub struct AvailableModels { cloud: Vec<CloudModel>, effective_tier: i32 }`
   - `pub fn parse_available_models(value: &serde_json::Value) -> Result<AvailableModels, AuthError>`
   - 单测：内置一段固定 JSON fixture，断言解析出 12 条、字段映射正确（**不引 wiremock，不加依赖**）

2. **改** `crates/flowy-auth/src/client.rs`（约 +25 行）
   - 新增 `pub fn available_models(&self, token: &str) -> Result<AvailableModels, AuthError>`
   - 复用已有私有 `get(path, bearer)` —— 它已经会同时注入 `Authorization: Bearer` 和 `token` 两个头
   - 路径拼接：`/api/v2/model/availableListClaw?category=1&app={config.app}`

3. **改** `crates/flowy-auth/src/lib.rs`：导出新类型

**零新依赖**：`reqwest` / `serde_json` / `thiserror` 都已在依赖表里。

### 第 2 层：`flowy-agent`（状态承载）

4. **改** `crates/flowy-agent/src/model.rs`
   - 删除 `Model { Max, Standard, Lite, Flex }` 四档位枚举
   - 改为 `pub struct ModelChoice { pub id: String }`
   - 保留 `Plan`（`free/paid`）用于展示，不做门控

5. **新增** `crates/flowy-agent/src/catalog.rs`（约 60 行）
   - `ProviderCatalog { provider: "flowy", models: Vec<CloudModel>, selected: Option<String> }`
   - `refresh(&mut self, token) -> Result<(), &'static str>`
   - 失败语义：**保留上次快照，不发明模型，不清空选择**

### 第 3 层：`flowy-app` / `flowy-studio`（渲染 + 持久化）

6. **改** `crates/flowy-app/src/prefs.rs`
   - 字段 `model: Model` → `model_id: String`（默认空）
   - 反序列化兼容：读到旧值 `lite/standard/max/flex` 时映射为空 + 首条兜底

7. **改** `crates/flowy-studio/src/composer.rs`、`studio.rs`、`panel.rs`
   - 下拉数据源从 `Model::ALL` 换成 `catalog.models`
   - 标签从 i18n 档位名换成 `CloudModel.name`（服务端已给显示名）

8. **隐藏** `crates/flowy-app/src/app/flex.rs` 的入口
   - 不改文件内部逻辑，只在菜单/设置里去掉打开 Flex 的项
   - 二期再物理删除（连带 23 个 locale 的 `flex.*` 键）

---

## 5. 验收标准（可测试通过）

### 5.1 单测（当前分支必须通过）

1. `cargo test -p flowy-auth` 通过，新增 fixture 解析测试断言：
   - 12 条 cloud 模型
   - 首条 `id == "AIPC-deepseek-v4-flash"`
   - `context_window == 1000000`、`max_tokens == 384000`、`credit_rate == 0.5`
2. `cargo test -p flowy-agent` 通过，新增断言：
   - 刷新失败时保留旧快照
   - 选择 id 不在目录里时回退首条

### 5.2 手动验证（必须现场通过）

1. 启动应用 → 进入模型选择器 → 能看到 **12 条云端模型**（名称来自服务端）
2. 选一条 → 关掉应用 → 重开 → 选择仍是那条（`preferences.json` 里 `model_id` 落盘）
3. 退出登录 → 目录清空，选择器显示空态，不崩溃
4. 断网启动 → 目录为空但不 panic（沿用上次快照）

### 5.3 明确不验收

1. 对话是否出真实回答（本期仍 mock，理由见 §2.2）
2. 计费/额度扣减
3. 档位锁 UI

---

## 6. 与下一个分支的边界

本期结束时，`flowy-agent` 手里有：provider=`flowy`、`Vec<CloudModel>`、`selected id`。

下一个分支才做（LLM 内核）：

1. 用 `CloudModel.endpoint` 作为 `base_url`
2. 用账号 JWT 作为 `api_key`
3. 调 `dsh_rs::bundle::openai_compatible_adapter(...)` + `register_adapter` + `apply_config`
4. 验证流式出字与工具调用

**本分支不碰 `dsh-rs` 一个字符**，也不改 `LlmConfig` 的现有 env 通路（保留作兜底）。

---

## 7. 风险与回滚

1. **目录结构漂移**：服务端若改字段名，解析会静默丢字段 —— 缓解：单测锁 fixture，字段全部 `#[serde(default)]`
2. **旧 prefs 兼容**：旧 `model: "lite"` 是字符串枚举，新字段是 id —— 缓解：读旧值时走兜底映射，写回时只写新字段
3. **i18n 23 locales**：本期不删 `flex.*` 键，避免 23 个文件同时改动；二期用脚本批量清
4. **回滚**：全部改动集中在 4 个 crate 的少数文件，`git checkout` 即可回滚；不动 `Cargo.lock`，不引新依赖

---

## 8. 实测记录（2026-11-16）

探测脚本：`.absorb_tmp/probe_cloud_models.py`（未进仓库，可删）

```
account=hoyework@qq.com token_len=269
HTTP 200
code= 200 msg= Success
auto=3 cloud=12 effectiveModelTier=0

[auto ] AIPC-auto-intelligence       tier=0 tools=True  ctx=1000000
[auto ] AIPC-auto-balance            tier=0 tools=True  ctx=1000000
[auto ] AIPC-auto-cost               tier=0 tools=True  ctx=1000000
[cloud] AIPC-deepseek-v4-flash       tier=0 max=384000  credit=0.5
[cloud] AIPC-deepseek-v4-pro         tier=0 max=384000  credit=1.5
[cloud] AIPC-deepseek-v4.1-flash     tier=0 max=384000  credit=0.5
[cloud] AIPC-glm-5                   tier=0 max=16384   credit=1
[cloud] AIPC-glm-5.2                 tier=0 max=128000  credit=1.6
[cloud] AIPC-Kimi-K2.5               tier=0 max=262144  credit=1
[cloud] AIPC-Kimi-K2.6               tier=0 max=262144  credit=1.4
[cloud] AIPC-Minimax-M2.7            tier=0 max=131100  credit=0.5
[cloud] AIPC-Minimax-M3              tier=0 max=128000  credit=0.8
[cloud] AIPC-qwen3.7-max             tier=0 max=65536   credit=4
[cloud] AIPC-qwen3.7-plus            tier=0 max=65536   credit=1.3
[cloud] AIPC-qwen3.8-flash           tier=0 max=128000  credit=0.14
```

远端参考实现（同一份 allo 仓库的 Tauri 侧）：
- `crates/backend/nomifun-cloud/src/provider_sync.rs` — 目录同步
- `crates/backend/nomifun-cloud/src/flowy/mod.rs` — 请求构造与 Bearer 注入
- `crates/backend/nomifun-cloud/src/flowy/types.rs` — `ClawModelEntry` / `AvailableModelsClaw`

现状差距（flowy18_wip）：
- `availableListClaw` 全仓零命中
- `flowy-agent::Model` 是硬编码 4 档位
- `FlexProvider` 是硬编码 3 个 BYOK 端点（本期隐藏、二期删除）

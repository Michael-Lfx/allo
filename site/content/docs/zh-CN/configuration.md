# 配置文件（config.toml）

Agent Store 把所有长期偏好写进 `~/.agent-store/config.toml`（TOML 格式）：默认模型、API 供应商、模型别名与默认市场源。改一次，每次启动都生效。

- **默认位置**：`~/.agent-store/config.toml`（Windows 上为 `%USERPROFILE%\.agent-store\config.toml`）
- **可选文件**：文件缺失时 App Server 正常运行，但模型调用需要先在 Web UI 中手工创建供应商；存在时启动即自动注册其中声明的供应商、模型默认值与市场源。

> 本文件沿用 Claude Code / Codex / Kimi Code 风格的用户级 `config.toml` 惯例（`[providers.<name>]` + `[models."<provider>/<model>"]`）。**未知的顶层与嵌套键会被容忍**——文件属于你，可以携带 Agent Store 不消费的其他工具（如 Kimi Code 的 `thinking`、`permission`、`hooks`）的设置，不会导致解析失败。

## 完整示例

```toml
default_model = "opencode/mimo-v2.5-free"

[providers.opencode]
type = "openai"
api_key = "sk-..."
base_url = "https://opencode.ai/zen/v1"

[providers.mimo]
type = "openai"
base_url = "https://api.xiaomimimo.com/v1"

[models."opencode/mimo-v2.5-free"]
provider = "opencode"
model = "mimo-v2.5-free"
display_name = "MiMo V2.5 Free"
max_context_size = 200000
max_output_size = 8000
capabilities = ["thinking", "tool_use"]

[models."opencode/laguna-s-2.1-free"]
provider = "opencode"
model = "laguna-s-2.1-free"
max_context_size = 256000
display_name = "Laguna S 2.1 Free"

# 默认市场源（首次调 store/market 时自动注册，公网 VPS 镜像）
[default_marketplaces.experts]
source_kind = "url"
source = "http://111.170.173.22:10072/experts/.codebuddy-plugin/marketplace.json"
```

## 顶层字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `default_model` | `string` | 默认模型别名，格式 `"<provider>/<model>"`，必须能在 `[models]` 中解析；调用方不指定模型时使用 |
| `providers` | `table` | API 供应商表 → `providers` |
| `models` | `table` | 模型别名表 → `models` |
| `default_marketplaces` | `table` | 启动自动注册的市场源表 → `default_marketplaces` |

## `providers`

以唯一名称为 key 的供应商表。Agent Store 从**这里**读取凭证，不会从 shell 环境变量自动取后备值。

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `type` | `string` | 否 | 供应商类型（映射到运行时平台），如 `openai`、`anthropic` |
| `api_key` | `string` | 否 | API 密钥，明文写在配置文件里 |
| `base_url` | `string` | 否 | API 基础 URL |
| `enabled` | `boolean` | 否 | 是否启用以自动注册；缺省视为启用 |

> 密钥安全：`api_key` 只用于加密的供应商注册，读取后即被丢弃，**不会**出现在任何日志与追踪负载中。文件权限建议自行收紧（`chmod 600`）。

## `models`

以 `"<provider>/<model>"` 为 key 的模型别名表。`provider` 必须指向 `[providers]` 中已声明的键。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `provider` | `string` | 所属供应商 key（必填） |
| `model` | `string` | 发给上游的模型名（必填） |
| `display_name` | `string` | 界面显示名 |
| `max_context_size` | `integer` | 上下文窗口上限（token 数） |
| `max_output_size` | `integer` | 单次输出上限（token 数） |
| `capabilities` | `array<string>` | 能力标记，如 `["thinking", "tool_use", "image_in"]` |
| `reasoning_key` | `string` | 思考内容在响应中的字段名，如 `reasoning_content` |

## `default_marketplaces`

默认市场源（winget 风格的软件源）。App Server 在首次 `store` / `market` 调用时自动注册；同 id 已存在时按新 source 重新激活。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `source_kind` | `string` | `url` \| `github` \| `git` \| `directory` |
| `source` | `string` | 具体地址；`url` 类型建议同时提供目录枚举（`_files.txt`）以支持条目树镜像 |

```toml
[default_marketplaces.experts]
source_kind = "url"
source = "http://111.170.173.22:10072/experts/.codebuddy-plugin/marketplace.json"

[default_marketplaces.workbuddy-skills]
source_kind = "url"
source = "http://111.170.173.22:10072/skills/.codebuddy-skill/marketplace.json"

[default_marketplaces.connectors]
source_kind = "url"
source = "http://111.170.173.22:10072/connectors/.codebuddy-connector/connectors.json"
```

## 与 Kimi Code / Claude Code 配置的异同

| 维度 | Agent Store | Kimi Code 等 |
| --- | --- | --- |
| 文件位置 | `~/.agent-store/config.toml` | `~/.kimi-code/config.toml` 等（各工具独立目录） |
| `[providers]` / `[models]` | 同构：`type`/`api_key`/`base_url`、`provider`/`model`/`max_context_size`/`capabilities` | 同构 |
| `default_model` | `"<provider>/<model>"` 别名 | 同构 |
| 未知键 | 容忍，不报错 | 容忍 |
| 环境变量后备 | **无**——凭证只从文件读取 | 部分工具有 `env` 子表/环境变量后备 |
| Agent Store 专属 | `default_marketplaces` | 无 |

如果你的配置里已经有 Kimi Code 或其他工具的 `[providers]`、`[models]` 段落，可以**直接复制**它们到 `~/.agent-store/config.toml` 使用（前提是该供应商走 OpenAI/Anthropic 兼容协议）；不相关的段落（`thinking`、`permission`、`hooks` 等）保留与否都不影响 Agent Store 解析。
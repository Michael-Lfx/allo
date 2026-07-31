# Skill 发现与按需加载模式

本文回答一个产品与架构问题：Agent 是否应在会话开始时得到全部 Skill 的名称和描述，以及 Skill 正文何时进入上下文。

## 结论

不要把全部 `SKILL.md` 正文注入上下文。主流 Agent Skills 的共同模式是**渐进披露**：会话开始时让模型看到轻量目录，模型判断相关后再读取具体 Skill 的说明和资源。

“全部目录一次进上下文”只适合目录很小的场景；它不是无限扩展的规则。推荐 Flowy 采用混合模式：

1. 对当前 Agent、工作区和信任策略均可用的 Skill 生成统一目录；
2. 目录在预算内时，向模型提供全部 `name`、`description`、来源、风险级别和按需加载入口；
3. 超出目录预算时，优先保留显式提及、本次推荐、当前项目相关和高置信匹配项；其余通过一个 `search_skills`/`get_skill` 发现工具按需查询；
4. 模型选中后才读完整 `SKILL.md`；其中的引用资料和脚本继续按需读取或执行；
5. "可发现"、"可加载"、"可执行" 分属不同策略。发现目录绝不等同于授予文件、网络或命令权限。

这不是让用户每次手选 allowlist。默认应是“所有可信 Skill 可发现”，会话 UI 只表达“推荐给本任务”和“本次禁用”。

## 一手资料中的模式

| 产品/规范 | 初始提供给模型 | 后续加载 | 面对大目录的策略 |
| --- | --- | --- | --- |
| Anthropic Agent Skills | 每个 Skill 的 YAML `name` 与 `description`，随 system prompt 加载 | 请求与描述匹配后读取 `SKILL.md`；其余资源继续按需读取 | 文档说明元数据约每 Skill 约 100 tokens，因此完整正文不预先占用上下文 |
| OpenAI Codex | 每个 Skill 的名称、描述和路径 | 选中或显式调用后读完整 `SKILL.md` | 初始清单最多占上下文 2%，未知窗口时最多 8,000 字符；先缩短描述，仍过大时可省略部分 Skill 并警告 |
| Claude Code | `description` 用于自动决定是否加载，且可以显式 `/skill-name` | 仅在 Skill 被使用时加载正文 | 通过用户、项目、企业、插件和按目录激活的范围发现，避免把所有嵌套目录在启动时都激活 |
| MCP Tools | `tools/list` 返回 `name`、`title`、`description` 和 schema | 模型选择后 `tools/call` | `tools/list` 原生支持分页和目录变更通知；MCP 不定义 Skill 标准，也不要求 embedding 检索 |

Anthropic 明确将其模型分为元数据、`SKILL.md` 指令、资源/代码三个阶段；OpenAI Codex 进一步给出了初始 Skill 目录的硬预算。这说明“摘要发现 + 内容按需加载”是成熟方案，而不是把每份正文全量塞给模型。

## 为什么不是无条件全量 title + description

目录元数据远小于正文，但大量 Skill 时仍会产生四类问题：

- **上下文成本**：每个描述都消耗输入 token，并挤压任务、历史和工具结果。Codex 的 2%/8,000 字符上限就是对此的直接回应。
- **路由准确性**：同义、宽泛或质量不佳的描述会制造候选噪声；模型会漏掉真正相关项，或错误触发相邻 Skill。
- **安全与信任**：元数据本身也可能是提示注入载体；而真正风险来自加载后指引模型调用工具、读写文件或访问网络。Anthropic 要求只使用可信来源的 Skill；MCP 也要求将工具注解视为不可信，除非来自可信 server。
- **动态性**：项目/工作区、MCP 连接和 Agent 能力会变化。静态会话快照无法单独代表全部可发现能力。

因此，embedding/语义检索可以作为大目录的**召回优化**，但不能成为唯一能力边界：它可能漏召回、不可解释，也不能替代明确的禁用、信任和权限规则。检索结果必须仍经过同一份策略过滤。

## 建议的 Flowy 设计

### 1. 建立一个唯一的目录与策略边界

后端提供 `SkillCatalog`，将 Flowly 安装库、可信全局目录、项目目录、bundled Skill 和可映射的 MCP Skill 收敛为稳定记录：

```text
SkillRecord {
  id, name, description, source, scope, trust_level,
  capability_tags, risk_level, loader, version
}
```

再由 `SessionSkillPlan` 只保存会话差异，而不是复制一份 allowlist：

```text
SessionSkillPlan {
  recommended_skill_ids,
  disabled_skill_ids,
  approval_overrides,
  catalog_version
}
```

`EffectiveSkillSet = policy_eligible(catalog) - disabled + recommendation_rank`。同一个 `EffectiveSkillSet` 必须同时供 Nomi、ACP 注入器、首页和对话页消费，避免 Nomi 额外扫描与 UI 选择相互矛盾。

### 2. 使用预算化的两级发现

目录较小：在 system prompt 放全部可发现 Skill 的规范化摘要。每项只含名称、触发描述、来源/风险和 `get_skill(id)` 提示。

目录较大：

- 固定保留显式点名、任务推荐、当前项目专属和低风险高相关项；
- 暴露 `search_skills(query, tags?, source?)`，结果返回同样的轻量摘要；
- 暴露 `get_skill(id)`，仅对策略允许项返回完整 `SKILL.md`；
- 对返回结果设大小上限、分页和审计字段。MCP 的 `tools/list` 分页与 `list_changed` 通知可作为动态目录的协议先例。

初版可先实现关键词/标签过滤，并记录“用户任务 -> 被展示候选 -> 最终加载”的数据以评估漏召回；只有确认关键词召回不足时再增加 embedding 排序。不要把 embedding 数据库作为访问控制层。

### 3. 分离选择、信任和授权

| 概念 | 用户语义 | 强制位置 |
| --- | --- | --- |
| 可发现 | Agent 能在目录或搜索中知道它存在 | `SkillCatalog` 的策略过滤 |
| 推荐 | 本任务优先考虑，但仍可选其他可信 Skill | `SessionSkillPlan.recommended_skill_ids` |
| 本次禁用 | 本会话不能被搜索、加载或调用 | `SessionSkillPlan.disabled_skill_ids`，所有 loader 强制执行 |
| 信任 | 安装来源/内容是否允许进入目录 | 安装与 catalog policy |
| 授权 | Skill 触发命令、文件、网络或 MCP 写操作时是否可执行 | 运行时 capability/approval 边界 |

截图中的“已启用 N 个”应改为符合真实行为的文案：默认渐进模式下是“本次推荐 N 个 Skills”，并明确“所有可信 Skills 均可由 Agent 按需发现”。只有实现严格 `disabled` 的端到端阻断后，才使用“本次禁用”。

## 对 `/` 命令的关系

Skill 发现不应与 `/` 命令目录混为一谈。`/open`、`/goal` 之类是宿主命令；Skill 是 Agent 可隐式选择或用户显式指定的工作流。首页和对话页可共享宿主命令目录，但会话状态命令仍按 `conversation_id` 作用域筛选。Skill 则由同一 `SkillCatalog` 提供，不应因有没有 `conversation_id` 而变成不同的能力世界。

## 参考（一手资料）

- [Anthropic: Agent Skills](https://docs.anthropic.com/en/docs/agents-and-tools/agent-skills/overview)：三级渐进披露、启动时元数据、触发后读取 `SKILL.md`、可信来源安全建议。
- [OpenAI: Build skills](https://learn.chatgpt.com/docs/build-skills)：Codex 的 name/description 初始目录、2%/8,000 字符预算、描述缩短与省略策略。
- [OpenAI: Skills](https://developers.openai.com/plugins/concepts/skills)：模型先见元数据，匹配或显式调用时加载完整说明；Skill 与 MCP 的职责边界。
- [Claude Code: Extend Claude with skills](https://code.claude.com/docs/en/skills)：正文仅在使用时加载、description 自动匹配、项目和嵌套目录的渐进发现。
- [Model Context Protocol: Tools specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)：`tools/list` 分页、`list_changed`、模型控制调用和人类确认建议。

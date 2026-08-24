# Skill 目录与显式加载

> **最后维护：** 2026-08-24 · 核对基准：commit `d791691c6` ·
> 文档性质：现行边界记录 · 内容基线：2026-08-07

本文记录 Flowy 当前的 Skill 发现和加载边界。它不描述尚未实现的
推荐、会话禁用或自动语义路由机制。

## 当前决策

Skill 与宿主 `/` 命令是两个独立系统：

- `/open`、`/goal` 等是宿主命令。它们可按是否已有
  `conversation_id` 决定可用性。
- Skill 是可加载的 Agent 工作流。首页和对话页从同一份
  `SkillCatalog` 读取候选，因此不会因是否已有会话而成为两套不同的
  Skill 世界。

输入 `/` 时，启动器分组显示系统命令、Skills 和 Agent 命令。选择
Skill 只是在当前待发送内容中加入其 source-qualified ID；它不执行
宿主命令，也不改变 `/` 命令目录。

当前不提供“推荐 Skill”“已启用 N 个 Skill”或会话级 Skill allowlist。
用户不需要先在首页勾选 Skill 才能使用它；所有通过目录策略的候选都
可由 `/` 搜索并显式选择。

## 目录边界

后端目录为每个可选 Skill 提供稳定的 source-qualified identity、显示名、
描述和来源。例如，同名 Skill 可分别表示为 `builtin:writer` 和
`user:writer`。这样 UI、Preset 和加载器不会因重名而把来源猜错。

目录当前聚合以下受信任来源：

- 内置 Skill；
- 应用数据目录中的用户 Skill；
- 全局 `~/.agents/skills`，以 `user:agents:<name>` 标识；
- 后端启动时当前项目的 `.agents/skills`，以
  `project:workspace:<name>` 标识。

全局和项目目录缺失时不会影响其他目录。任意由设置页临时扫描的外部
目录不会自动进入 `/`；这类目录仍须显式导入，避免把未经用户选择的
文件直接注入 Agent 上下文。

`cron`、`nomifun-skills` 和 `skill-creator` 是系统自动注入能力：它们不
出现在 Skill catalog，也不会在启动器的 Skill 分组中被用户选择。

Preset 可以保留自己的 Skill 绑定，但该绑定不会筛选、排序或影响 `/`
的 Skill catalog。Preset 中的 canonical Skill 与用户本次显式选择的
Skill 会在首个实际 turn 合并加载；之后的历史由已持久化的加载记录
表示。旧 Preset 中仅有名称的绑定采用受限的 `legacy:` 标记保留原有
运行时解析，具体的迁移例外见
[ADR-0001](../adr/0001-source-qualified-skill-bindings.md)。

## 加载与历史

选中 Skill 后，服务端验证它仍在当前 catalog 中，再读取完整
`SKILL.md`。每次显式加载都会记录 source-qualified ID、来源、名称、
内容和版本 hash，并以 `skill_load` 消息投影到会话历史。

历史读取的是这份不可变快照，而不是后来磁盘上可能被修改的 `SKILL.md`。
因此历史记录不会随 Skill 文件更新而改变，也不会因为重名 Skill 出现
来源漂移。

## 与渐进披露的关系

主流 Agent Skills 采用渐进披露：先发现轻量元数据，需要时才读取完整
`SKILL.md`，随后再按需访问其资源和脚本。当前 Flowy 的 `/` 显式加载
是这一模式的用户控制入口：catalog 不向对话注入全部 Skill 正文，只有
被选择的 Skill 正文进入该 turn 的上下文和不可变历史。

若未来增加 Agent 自动发现，仍应先以受信任、已授权目录的简短元数据
作为发现层，并在选中后再加载正文；这需要独立的产品和权限设计，不能
通过当前 `/` 选择逻辑隐式推断。

## 参考

- [Anthropic: Agent Skills](https://docs.anthropic.com/en/docs/agents-and-tools/agent-skills/overview)
- [OpenAI: Skills](https://developers.openai.com/plugins/concepts/skills)
- [Claude Code: Extend Claude with skills](https://code.claude.com/docs/en/skills)
- [Model Context Protocol: Tools specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)

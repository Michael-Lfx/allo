# Skill 目录与双通道加载

> **最后维护：** 2026-10-16  
> 文档性质：现行边界记录  
> 详细方案：[session-skill-auto-use.zh.md](session-skill-auto-use.zh.md)

本文记录 Nomi / ACP **会话**里 Skill 如何被发现和加载。Vimax 垂直 overlay 不在这里。

## 当前决策

Skill 与宿主 `/` 命令仍是两套系统，但会话发现已经接到 Flowy catalog：

- `/open`、`/goal` 等是宿主命令。
- Skill 是可加载的 Agent 工作流。Composer 用 `/` 选 chip（不是正文 `@token`）时，等于 **force**：该轮直接注入指针，不必再调 Skill 工具。
- 用户不点 `/` 时，模型仍能看见 catalog 的 L1 元数据（`name` + canonical id + `description`），并自行调用常驻 `Skill` 工具加载正文。默认策略是 `model-auto`。
- 高风险 skill 可写成 `disable-model-invocation: true` / `invocation-policy: /invoke`，或在 Skills Hub 关掉「允许模型自动调用」。这类 skill 只出现在 `/` 菜单，模型猜不到也调不到。

当前 **不做** 用 `/` 选择逻辑去猜自动路由，也 **不** 把 SkillSearch 当主路径。catalog 稳定超过约 200 条时，引擎才会额外注册 `SkillSearch`。

## 目录边界

后端目录为每个可选 Skill 提供稳定的 source-qualified identity、显示名、描述和来源。例如同名 Skill 分别是 `builtin:writer` 与 `user:writer`。

目录聚合受信任来源：

- 内置 Skill；
- 应用数据目录中的用户 Skill；
- 全局 `~/.agents/skills`（`user:agents:<name>`）；
- 启动时当前项目的 `.agents/skills`（`project:workspace:<name>`）。

临时扫描的外部目录不会自动进入模型可见集，须先导入。`cron` 等系统自动注入能力不出现在 `/` 的 Skill 分组里，但仍可按原规则注入。

## 双通道

1. **Force（人点 `/` 或钉住 Mode）**：写入 `[Loaded Skills]` 指针和 `skill_load` 账本。提示明确：已 force 的不要再调 Skill 工具；其余 catalog skill 仍可用工具加载。
2. **Enable（模型对照 description）**：`Skill` 工具按 canonical id 优先、唯一短名其次查找；重名返回歧义错误。加载成功后写入与显式路径同构的 `skill_load`（`invocation: model-auto`）。

L1 只放元数据。正文只在 force 或 Skill 工具成功后进入。工具列表在 loop 中途不增删。

## 超预算与观测

列表预算约 **2%** 上下文窗口，仍用三级降级。再放不下时，宿主加权词法排出 topK；`NOMI_SKILL_RANKING_SHADOW_ONLY=1` 时只写 SQLite、不改模型可见列表。网页只展示 `skill_selection_event` 表。

## 参考

- [session-skill-auto-use.zh.md](session-skill-auto-use.zh.md)
- [Anthropic: Agent Skills](https://docs.anthropic.com/en/docs/agents-and-tools/agent-skills/overview)
- [agentskills.io](https://agentskills.io)

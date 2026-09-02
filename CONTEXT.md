# Allo Agent Store Context

本上下文统一 Agent Store 在产品定义、运行实例与团队规划之间的领域语言，避免把产品级 Agent、运行时执行者和单次规划输入混为一谈。

## Language

**AgentDefinition**：Agent Store 中可复用、可版本化的专家定义，描述身份、指令、能力和策略。
_Avoid_: Runtime Agent, Conversation

**AgentTeamDefinition**：可复用、可版本化的团队定义，包含固定成员名册、Leader 规划角色和协作策略。
_Avoid_: TeamRun, member session

**Runtime Agent**：实际负责模型与工具执行的运行时执行者，不是产品目录中的 AgentDefinition。
_Avoid_: AgentDefinition, team member definition

**Participant**：一次执行中代表一个已解析 AgentDefinition 的固定运行成员，拥有自己的配置和 Prompt 快照。
_Avoid_: dynamic agent, shared prompt

**TeamRun**：一次团队目标执行实例，包含固定 Participant、计划、步骤、尝试、事件和产物。
_Avoid_: TeamDefinition, conversation

**Planning Context**：单次 TeamRun 为生成计划而派生的输入，由 Leader 规划指令、脱敏成员能力摘要、团队目标和策略组成；它不授予权限，也不包含成员完整 Prompt 或真实凭据。
_Avoid_: shared member prompt, permission grant

**Plan Revision**：TeamRun 在初始规划或 replan 后形成的一个不可变计划版本，包含该版本的 DAG 和计划依据。
_Avoid_: mutable plan

**Attempt**：某个 Step 的一次执行尝试；重试会产生新的 Attempt，历史 Attempt 保留。
_Avoid_: retry in place

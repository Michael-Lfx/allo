# 架构说明

Flowy Agent Store 采用**本地优先**的分层架构：云端只管理「定义」，本机负责「执行」。

## 分层

```text
CodeBuddy / WorkBuddy 来源目录
        ↓ Importer（路径/摘要/兼容性校验）
PluginSnapshot（版本化、不可变）
        ↓ Catalog
Agent / Team / Skill / Connector 定义
        ↓ Runtime Adapter（领域对象 → allo 执行语义）
allo Runtime（Rust，唯一执行引擎）
        ↓ Versioned App Server Protocol
TS SDK · CLI · Web UI（协议客户端）
```

## 关键边界

- **Runtime**：`allo` 是唯一的执行引擎，打包为单文件、依赖小、资源占用低。
- **App Server 协议**：SDK、CLI 与 Web 都只依赖它；它暴露版本化、类型安全的生命周期与事件消息。公共 ID 为 opaque，不暴露内部执行 ID。
- **Runtime Adapter**：唯一的领域模型到 `allo` 内部对象的边界；Team 的规划上下文（Planning Context）在运行时派生，不含成员完整 Prompt 或真实凭据。
- **凭据**：只进入本地安全存储；Web / SDK 仅接触状态、账号标识与过期时间，不接触真实 Token。

## Agent Team（V1）

固定成员 + Leader 规划上下文驱动的 **planned DAG**：

- Leader 是规划角色，不创建独立会话；
- 独立 Step 支持局部并行、重试与 replan；
- 事件、产物与终态持久化，可查询、可重放。

## 设计原则

- 先验证单 Agent，再实现 Team；
- 事件日志是唯一事实来源，投影为派生状态；
- 未通过运行时验证的能力不标记为可运行。

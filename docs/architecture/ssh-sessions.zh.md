# SSH 远程会话域

> **最后维护：** 2026-08-24 · 核对基准：commit `d791691c6` ·
> 文档性质：现行架构文档（新建，基于源码逐项核对）

SSH 能力分两层：传输库隔离在 shared 层，业务面在 backend 层。它不是终端页面的
一部分——`/terminal` 是本地 PTY；SSH 的集成模型是"把远程主机挂进 agent 会话"。

## 传输库 `flowy-ssh`（shared）

[`crates/shared/flowy-ssh`](../../crates/shared/flowy-ssh/) 是纯 russh 适配层，
零 nomi-*/nomifun-* 依赖——防止 russh 高频变动的 Handler API 成为
`nomi-tools` / `nomi-agent` / `nomifun-terminal` 的传递依赖
（`dependency_isolation.rs` 测试钉死该约束）。模块：

- `connection` —— 拨号 / 认证；host-key 策略 AcceptNew → known_hosts；
- `credential` —— `SshCredential` + `Auth::{Password, PrivateKey, Certificate,
  Agent}`；秘密用 `Zeroizing`，Debug 输出脱敏；
- `shell` —— `RemoteShell`：跨命令保持 cwd+env 的持久 PTY，哨兵完成协议
  `__NOMI_END_<nonce>__<rc>__<pwd>` 判定命令结束与退出码；
- `fs` —— SFTP；`responder` —— sudo 提示应答。

## 业务面 `nomifun-ssh`

- **主机簿加密存储**：AES-256-GCM（`nomifun_common::encrypt_string`），密钥由 JWT
  secret 派生的应用数据加密密钥承担；DTO 永不回显明文凭据。表
  `ssh_hosts`（迁移 `030_ssh_hosts.sql`，凭据列带 `_encrypted` 后缀）。
- **连接池**：每 (conversation, host) 一条受监督链路，寿命独立于 agent runtime；
  单写者 `watch<SshLinkState>`、重连阶梯 / 拨号冷却 / 存活 ping / 关闭证明。
- **路由** `/api/ssh-hosts*`（实例 owner 保护）：CRUD、`/test-connection`
  （一次性 exec 命令探测，15 s 预算）、`/import-candidates` + `/import`
  （只读解析 `~/.ssh/config` 导入）。
- **WS 事件**：`ssh.status`（owner 作用域实时通道，camelCase
  `SshStatusEvent`）——链路阶段跃迁，非 turn 粒度。
- 无 feature gate；池缺失时 test-connection 如实拒绝。

## 集成模型

"Connect" 创建一个携带 `extra.ssh_host_id` 的 nomi 会话；agent 远程工具经
[`nomifun-ai-agent`](../../crates/backend/nomifun-ai-agent/) 的 `SshBackend` 接缝
拨号到同一个池（`services.rs` 接线）。agent turn 使用持久 PTY shell 通道，
探测才用一次性 exec。

## 前端

- 设置页 `/settings/ssh-hosts` → `pages/settings/SshHostSettings/SshHostManagement.tsx`
  （表单校验 + ssh_config 导入 UI）；API 客户端 `ipcBridge.ssh.*`。
- 会话侧 hooks：`useOpenSshSession` / `useSshLinkStatus`（实时状态 pill）。

# Agent Store V1 测试验收用例（总索引）

> 状态：**TC 总索引**（附 §1/§2 公共口径）；逐条用例正文已归各归属文档，本文件不再复制正文
> 日期：2026-08-26（2026-09-11 结构收敛：正文归编号文档）
> 前置：`16-sdk-webui-site-priority-plan.zh.md` §6 / §7、`10-public-contracts.md` 及本目录中的 Agent Store 规格文档
> 说明：本文件是 V1 测试用例的**总索引与公共口径**。§1/§2 定义全仓共用的测试环境、证据要求与验收等级；**`TC-*` 逐条正文的唯一位置见 §3 归属总表**，本文件不再复制用例正文。新增或修改用例：先改归属文档正文，再同步 §3 的编号归属。

## 1. 测试环境与证据要求

### 1.1 环境

测试必须记录：

```text
App Server 版本
allo Runtime 版本
Protocol version
SDK 版本
Import source digest
Definition version
Connector/Mock Server 版本
workspace 标识
操作系统和关键配置摘要
```

真实凭据只记录：

```text
credential_present: true
credential_id: <opaque id>
```

禁止记录真实值；任何示例统一使用 `[REDACTED]`。

### 1.2 证据

每个通过用例至少保存：

```text
测试输入摘要
请求/响应摘要
相关 event_id/sequence
Run/Task/Attempt/Artifact 公共 ID
最终状态
日志或截图路径（脱敏）
```

外部系统副作用用测试账号、隔离 workspace 或 mock Connector；不得用生产数据验收。

## 2. 验收等级

```text
P0 阻断发布：主链路、安全、数据一致性
P1 必须交付：V1 功能完整性
P2 可延期：体验优化和非核心适配
```

结果：

```text
PASS
FAIL
BLOCKED
NOT_RUN
```

`BLOCKED` 必须写明阻塞原因和替代证据，不能当作 PASS。

## 3. 用例归属总表（正文唯一位置）

| TC 组 | 唯一正文（归属文档） |
|---|---|
| `TC-IMP-001`~`017`、`TC-INS-001`~`007` | `02-codebuddy-workbuddy-import-spec.md` §13（验收用例 TC-IMP / TC-INS） |
| `TC-RT-001`~`010`、`TC-TEAM-001`~`009` | `13-p0-execution-plan.md` §10（Runtime 验收用例 TC-RT / TC-TEAM） |
| `TC-API-001`~`004`、`TC-SDK-001`~`003` | `05-allo-app-server-protocol.md` §14（验收用例正文 TC-API / TC-SDK） |
| `TC-CONN-001/002`、`TC-OAUTH-001`~`004`、`TC-CLI-001`、`TC-STDIO-001` | `06-connector-oauth-security.md` §11（验收用例正文 TC-OAUTH / TC-CONN / TC-CLI / TC-STDIO） |
| `TC-WEB-001`~`004`、`TC-SEC-001`~`003`、`TC-CATALOG-001`~`005` | `19-webui-codex-alignment.zh.md` §9（验收用例 TC-WEB / TC-SEC / TC-CATALOG） |

配套章节：

| 内容 | 唯一位置 |
|---|---|
| 测试环境与证据要求、验收等级（§1/§2 公共口径） | 本文件 §1/§2（`13-p0-execution-plan.md` §11 只保留指针） |
| P0 发布门禁用例清单 | `13-p0-execution-plan.md` §12；发布准入判定见 `09-release-readiness.md` |
| 回归触发与测试报告要求 | `13-p0-execution-plan.md` §13 |

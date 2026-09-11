# Agent Store V1 发布准入与风险清单

> 状态：发布门禁已定义；当前结论 blocked（评估快照见 §9）
> 日期：2026-08-26
> 更新：2026-09-09 —— §9 快照内 TC-SDK-001 矛盾已消（改为 spawn + 回环 WS，与 `12` 非目标一致）；其余门禁结论未变
> 适用范围：`docs/agent-store/` 中的 Agent Store 当前方案文档
> 前置：`00-architecture-decision.md` 至各线规范内的「验收用例」章节（`02` §13 · `05` §14 · `06` §11 · `13` §10–§13 · `19` §9）

## 1. 发布结论等级

```text
ready
ready-with-known-limitations
blocked
not-assessed
```

只有在 P0 测试全部通过、凭据安全检查通过、来源审核完成后，才能标记 `ready`。

`ready-with-known-limitations` 必须同时列出：

```text
限制能力
影响范围
替代路径
预计后续处理阶段
是否允许默认启用
```

## 2. 发布范围引用

V1 的范围和延期能力以 `00-architecture-decision.md` 为准；公共状态、事件、planning 和错误码以 `10-public-contracts.md` 为准。本文件只定义发布判定、门禁、证据、风险和回滚，不重复范围清单。

延期能力必须在公共 API 中使用明确 capability/status 表达，不能返回“支持但运行时失败”的模糊状态。

## 3. P0 发布门禁

以下门禁任一失败，发布结论必须为 `blocked`；P0 线要求的用例编号集合见 `13-p0-execution-plan.md` §12：

### Gate 1：Runtime

- allo Runtime Agent/Participant 实际创建链路通过；
- AgentDefinition 能解析为 Preset/ResolvedPresetSnapshot，并写入 ExecutionParticipant；
- Runtime Adapter 不泄露 allo 内部 ID；
- 单 Agent Run 完成、失败和取消状态正确；
- AgentTeam 固定成员池创建成功；
- Leader 与每个成员独立解析为 Preset/ResolvedPresetSnapshot；
- Planning Context 只含 Leader 规划指令、脱敏成员能力摘要和 Team 策略，不含完整成员 Prompt 或凭据；
- 每个 Step 使用其 Participant 自己的 Prompt Snapshot；
- Planning Context 驱动的 planned DAG、依赖、局部并行、retry、replan 通过；
- `13-p0-execution-plan.md` §10 中 TC-TEAM-001~008 全部 PASS；
- Runtime 重启后保留事件，并将未完成 Run 明确标记为不可恢复或需用户重新启动。

### Gate 2：Importer 与来源

- 有效 Plugin 可生成不可变 PluginSnapshot；
- 5 个 `software-company` Agent 和 1 个 Team 导入结果正确；
- 路径遍历、符号链接逃逸和 digest 冲突被阻断；
- 不支持组件不会静默丢弃；
- 版权状态和来源 digest 被保存；
- 凭据值没有进入 Snapshot。

### Gate 3：App Server 与 SDK

- initialize、AuthContext 和协议版本协商通过；
- Catalog、Agent Run、Team Run、Event、Artifact、Approval API 通过；
- WebSocket Transport 通过（V1 交付不含 stdio，见 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 2）；
- run/get 与 run_result 返回一致的持久化状态；尽力而为通知丢失不影响一致性；event 去重通过；
- 幂等键不会重复创建 Run；
- 错误使用稳定 code，不泄露敏感信息；
- `05-allo-app-server-protocol.md` §14 中 TC-API-001~004、TC-SDK-001~003 全部 PASS；其中 team/run 必须验证 Planning Context 摘要和成员 Prompt 隔离。

### Gate 4：Connector 与 OAuth

- Connector 工具命名空间和 allowlist 通过；
- 标准 PKCE Loopback OAuth 通过；
- issuer、resource、state、redirect_uri 校验通过；
- Token 仅在请求时注入；
- 401 只允许刷新并重试一次；
- 刷新失败进入 `reauthorization_required`；
- Connector Probe 通过后才能显示 `connected`；
- `06-connector-oauth-security.md` §11 中 TC-CONN-001~002、TC-OAUTH-001~004 全部 PASS。

### Gate 5：安全

- 凭据泄露扫描无高危结果；
- 高风险副作用需要 Approval；
- CLI/STDIO 不允许任意命令、参数、工作目录或环境变量覆盖；
- Hook/bin/script 默认不执行；
- Artifact 不允许任意路径读取；
- 审计记录已脱敏；
- `pending-legal-review` 资源不能公开分发。

## 4. 发布证据包

每次发布必须保存一份脱敏证据包：

```text
release-manifest.json
protocol-schema.json
import-report.json
compatibility-report.json
p0-test-report.json
security-scan-report.json
oauth-integration-report.json
source-review-report.json
known-limitations.md
```

证据包必须包含：

```text
产品版本
allo Runtime 版本
App Server Protocol 版本
SDK 版本
测试环境摘要
输入来源 digest
P0 用例统计
失败/阻塞用例
安全扫描结果
来源审核结论
```

不得包含：

```text
API Key
Access Token
Refresh Token
Password
Client Secret
完整 Authorization Header
连接字符串中的秘密部分
未脱敏的工具参数
```

## 5. 来源和版权准入

来源资源分为：

```text
approved-for-distribution
internal-evaluation-only
pending-legal-review
blocked
```

`pending-legal-review` 和 `internal-evaluation-only` 资源允许用于隔离测试或内部 Spike，但不得：

- 进入公开 Marketplace；
- 进入默认安装包；
- 默认启用自动更新；
- 在产品宣传中声明为可分发能力；
- 与其他用户账户同步。

来源审核必须记录：

```text
source_id
source_type
source_url_or_origin_summary
content_digest
license_or_permission_status
reviewed_at
reviewer_or_process
restrictions
```

## 6. 已知风险登记

| 风险 | 触发条件 | 发布处理 |
|---|---|---|
| Agent 运行桥接不完整 | AgentDefinition 无法解析为 Preset/ResolvedPresetSnapshot，或无法由 Runtime Agent 执行 | 阻断 Agent/Team 发布 |
| Team planned 不稳定 | DAG 物化、依赖或 replan 失败 | 阻断 Team 发布 |
| OAuth 只能登录不能调用 | Transport 注入或 Probe 失败 | Connector 标记 partial，不得显示 connected |
| 未完成 Run 状态不可判 | 重启后状态或终态丢失 | 阻断发布；先修复状态持久化与标记 |
| 任意进程执行 | Hook/bin/script/CLI 可绕过策略 | 阻断对应组件，默认不启用 |
| 凭据泄露 | 日志、事件、Prompt 或 Renderer 出现敏感值 | 立即阻断并清理证据 |
| 来源授权不明 | 版权或分发许可未确认 | 标记 pending-legal-review，禁止公开分发 |
| allo 内部 API 变更 | Adapter 或 UI 私有接口变化 | 通过 App Server contract tests 发现并阻断升级 |

## 7. 回滚要求

发布前必须验证：

- Catalog 可选择上一版 Definition；
- 新版本导入失败不会覆盖旧 Snapshot；
- 正在运行的 Run 继续使用已冻结版本；
- SDK 协议不兼容时能明确拒绝；
- Connector 凭据删除和重新授权不会影响其他 Principal；
- 失败发布可以禁用新版本而不删除历史 Run 和 Artifact。

回滚不得使用破坏性数据库重置或删除审计记录的方式完成。

## 8. 发布签字清单

```text
[ ] 架构基线与当前目录文档已同步
[ ] V1 范围与延期能力已确认
[ ] P0 测试全部 PASS
[ ] BLOCKED/FAIL 用例已有处理结论
[ ] Runtime Adapter 证据已保存
[ ] Importer/Catalog 证据已保存
[ ] App Server contract tests 已通过
[ ] SDK Node/Browser 测试已通过
[ ] Web/Flowy 端到端测试已通过
[ ] OAuth 注入、刷新、Probe 已通过
[ ] 凭据泄露扫描已通过
[ ] Tool Policy/Approval/审计已通过
[ ] 来源版权和分发状态已确认
[ ] 发布证据包已脱敏
[ ] 回滚路径已验证
[ ] 已知限制已对外记录
```

## 9. 当前准入判断

本文档本身不替代实际测试。以下为截至 2026-08 迭代（Importer/Phase 1 落地后）的如实快照；更新前必须重新执行测试总索引列出的用例：

```text
release_status: not-assessed（P0 未全过，仍等价 blocked；2026-09-04 增量快照）
assessed_at: 2026-09-04（评估快照，非门禁结果）
blocking_items:
  - Gate 1（Runtime）：TC-RT-001 已通过（单 Agent 真实 Run，`13-p0-execution-plan.md` §14）；
    TC-RT-002/005/006/009/010 未执行
  - Gate 4（Connector/OAuth：登录注入、401 刷新、重试、探针）：OAuth 运行证据已留存（`06-connector-oauth-security.md` §13），TC-CONN/TC-OAUTH 其余用例未全量执行
  - Gate 2（Importer）：TC-IMP-001~009 已通过（`02-codebuddy-workbuddy-import-spec.md` §14）；GitHub/Git/HTTP 市场源未实现
  - Gate 3（App Server）：initialize/版本协商/store 与 run 查询一致已有真机实证（smoke --real + A1）；
    TC-API-002（幂等重放）/TC-API-004（公共 ID 隔离）未执行；TC-SDK-001 已修订为 spawn + 回环 WS，与 `12` 非目标一致（见 `13-p0-execution-plan.md`）
  - Gate 5（安全扫描与来源审核）：未执行
  - V1 默认不启用：Hook/LSP/bin/scripts 执行、跨市场依赖
```

签署清单（上文 §8）仍全部未勾选；只有 P0 测试全部通过、凭据安全检查通过、来源审核完成后才能标记 `ready`。

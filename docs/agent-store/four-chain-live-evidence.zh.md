# 四链路 live 验收证据（WP-2）

> 状态：📎 证据（运行时实测快照，非契约）
> 日期：2026-09-09
> 脚本：`web/scripts/sdk-live-store-chain.ts`（SDK 公共面：`launchClient` + `client.*`）
> 模型：mimo-v2.5（provider 来自 Hermes attachments config，key 仅脚本内存）
> 最近一次结果：**20/20 PASS**（`RESULT PASS`）

## 1. 覆盖与判据

| 链 | 判据 | 结果 |
|---|---|---|
| C1 专家 | `agents.list` 含目标 agent 且 `preset_id` 非空；run 完成 | PASS |
| C2 技能 | `skills.list` 含已装技能；agent+skill 双 mention run 完成；会话快照冻结技能绑定 | PASS |
| C3 连接器 | `store/install-entry` → `connectors.list` 可见 → enable → probe 工具列举 → mention run 真实调用本地 mock MCP | PASS |
| C4 专家团 | `teams.list` 含团队（运行时为 Phase 2） | PASS |
| S1 真实市场 | 宿主默认市场（VPS-A 全树镜像：`experts` / `workbuddy-skills` / `connectors`）全部镜像进 store | PASS |
| TC-CONN-002 | 探针失败的连接器不得报 `connected` | PASS |

## 2. 最近一次运行记录

```text
data=C:\Users\15165\AppData\Local\Temp\agent-store-chain-1788932261601
PASS C1.import :: "completed"            # software-company 夹具
PASS C1.install :: 9
PASS C1.catalog-visible :: {"preset_id":"01a084ac-17fc-7251-a6cf-30d05796c94e"}
PASS C1.run-completed :: "completed"
PASS C4.team-visible :: {"id":"wb-software-company-team","lead":"wb-software-company-software-team-lead"}
PASS C2.import :: "completed"            # skill-market 夹具
PASS C2.install :: 2
PASS C2.skill-visible :: "hello"         # B1 修复实证
PASS C2.run-completed :: "completed"
PASS C2.skill-frozen-in-conversation     # B5 修复实证（会话 extra.skills 冻结 legacy:hello）
PASS S1.real-market-mirrored :: {"configured":["experts","workbuddy-skills","connectors"],"hit":[三个全中]}
PASS C3.store-visible / install(1) / catalog-visible / enabled
PASS C3.tool-listing :: {"success":true,"tools":["echo"]}     # B6 修复实证
PASS C3.mention-accepted / C3.run-completed
PASS C3.tool-called                      # 模型实际调用 MCP 工具，返回 echo:four-chain
PASS TC-CONN-002.install :: 1
PASS TC-CONN-002.probe-failure-not-connected :: {"probe_success":false,"status":"error"}
```

## 3. live 逼出的两个断点与修复

| 断点 | 根因 | 修复 | 验证 |
|---|---|---|---|
| **B5** 商店专家的技能/连接器 mention 永远失效 | `agent/run` 的 owner 默认模型回退分支用 `PresetOverrides{..Default::default()}` 重解析，丢掉 `apply_mentions` 产出的 `include_skills`/`mcp_server_ids`；agent-store preset 无模型绑定，必然走该分支 | `with_default_model` 合并助手，回退解析继承原 overrides | `default_model_fallback_keeps_mention_overrides` + C2.skill-frozen |
| **B6** stdio MCP 连接器丢 args/env | 导入只把 command 写进 `transport_summary`，注册时从 summary 重建 transport（`args=[]`），`bun.exe`/`npx` 类 server 空参启动即退出 | payload 增结构化 `transport`（command+args+env / url），`connector_transport` 优先取结构化值 | importer/installer 单测 + C3.tool-listing / C3.tool-called |

> 两个断点均由本脚本真机复现（B5：`C2.skill-frozen` FAIL；B6：probe 报 `Server closed stdout before responding`，库内 `transport_config.args=[]`）。

## 4. 未覆盖 / 后续

- **TC-OAUTH-001/002/004、TC-CONN-001**（OAuth 登录/凭据隔离/错误边界/工具命名空间）：需要 OAuth-capable MCP 服务与浏览器回环流程，本轮未做；已有证据见 `mcp-oauth-runtime-evidence.zh.md`（覆盖 TC-OAUTH-003 注入与刷新）。
- **C4 运行时**：受 §12 门禁约束（P0-A/B 关闭后立项 Team Spike），本轮只验「下载→安装→可见」。
- **夹具市场为本地目录**：真实市场只做了 S1 镜像冒烟；四链路的完整安装链路用本地夹具保证可重复。

## 5. 复现

```bash
cd web
AGENT_STORE_BIN=<repo>/target/debug/agent-store.exe bun scripts/sdk-live-store-chain.ts
# CHAIN_KEEP_DATA=1 保留实例数据目录供取证
```

退出码即判据：0 = 全部 PASS。宿主管理面（provider 注册、MCP enable/toggle）不是 App Server 协议方法，脚本内经 admin HTTP 完成并已标注 `[host admin]`。

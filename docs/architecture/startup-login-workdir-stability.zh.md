# 启动、登录、配置与工作目录稳定性

本文是 Allo/Flowy 启动、云登录、模型恢复、安装偏好和工作目录切换的维护契约。
修改相关代码前先确认本文的边界；实现细节以代码和测试为准，本文负责记录哪些数据
可以动、哪些数据绝对不能动，以及失败后应该如何恢复。

当前实现入口：

- 工作目录迁移：[`nomifun_common::work_dir_relocation`](../../crates/backend/nomifun-common/src/work_dir_relocation.rs)
- system API：[`nomifun-system/src/routes.rs`](../../crates/backend/nomifun-system/src/routes.rs)
- 安装偏好：[`installation_preferences.rs`](../../crates/backend/nomifun-system/src/installation_preferences.rs)
- 云模型恢复：[`CloudAuthContext.tsx`](../../ui/src/renderer/hooks/context/CloudAuthContext.tsx)
- 开发进程监督：[`run-dev-supervisor.mjs`](../../scripts/run-dev-supervisor.mjs)

## 1. 不可破坏的边界

| 内容 | 所属位置 | 普通工作目录切换是否改变 |
| --- | --- | --- |
| SQLite、聊天记录、Provider、Provider models、默认模型 | `<data_dir>/nomifun-backend.db` | 否 |
| Provider 加密凭据和数据集密钥 | `<data_dir>/encryption_key` | 否 |
| 日志 | `<data_dir>/logs/` | 否 |
| 安装级 UI 偏好 | `<data_dir>/installation-preferences.json` | 否 |
| runtime/Bun、companion 等 data-root side store | `<data_dir>/...` | 否 |
| 受管会话文件 | `<work_dir>/conversations/` | 是，仅迁移这一棵树 |
| 自定义外部 workspace | 用户自行配置的外部目录 | 否，不复制、不移动、不修改 |
| storage generation、dataset receipt、加密绑定 | data root 与受管 marker | 否，沿用原 generation |

因此，工作目录切换是文件根迁移，不是数据集切换，也不是 Factory Reset：

- 不轮换 storage generation。
- 不清空 SQLite，不删除 Provider，不清除模型缓存，不要求重新登录。
- 不搬迁日志、数据库、密钥、安装偏好或历史 `retired-datasets`。
- Factory Reset 和历史非 v3 数据升级仍然走 hard reset；不要把两条流程重新合并。

例如本地开发显式设置 `NOMIFUN_DATA_DIR=D:\tmp\flowy-dev`、UI 工作目录为
`D:\tmp\3333` 时，SQLite、Provider、聊天记录、密钥、日志和安装偏好都留在
`D:\tmp\flowy-dev`；只有受管的 `D:\tmp\3333\conversations` 参与工作根迁移。

## 2. 工作目录切换生命周期

### 2.1 请求阶段

`POST /api/system/work-dir` 只负责校验并写入迁移计划，不在当前进程中搬动数据。

目标必须满足：

- 是安全的绝对路径，source 与 target 不能相同。
- source/target 不能互相嵌套，也不能与 data root 重叠。
- Windows 路径比较使用项目路径等价函数，兼容大小写、UNC 和 `\\?\` 前缀；不能用
  简单的 `toLowerCase()` 代替文件系统身份校验。
- 目标不能已有受管 `conversations`、owner marker、dataset binding 或未完成迁移。
- 必须拒绝 symlink、junction 和其他不支持的 reparse point。
- 与当前规范化工作目录相同是 no-op：返回 `restart_required: false`，不返回
  `operation_id`。

真正的切换会在 `<data_dir>/.work-dir-relocation.pending/` 写入 version 2
`plan.json`，并返回 UUIDv7 `operation_id` 与 `restart_required: true`。计划记录
source、target、generation、目标身份、尝试次数和错误分类。旧版 version 1 计划可以读取，
第一次管理操作时原子升级为 version 2。

### 2.2 启动阶段

新进程必须先消费 relocation plan，再执行普通 v3 receipt 校验：

1. 排他锁定 data root、旧 work root 和新 work root。
2. 再次验证 generation、receipt、owner/binding marker、路径嵌套关系和目标身份。
3. 同卷优先原子 rename；跨卷复制到 operation 专属 staging，流式校验大小、SHA-256
   和完整目录树后再发布 `conversations`。
4. 跨卷成功后将旧会话树保留到
   `<old_work_dir>/.nomifun-work-relocation-backups/<operation_id>/conversations`，
   并在 `<data_dir>/.work-dir-relocation.backups/<operation_id>.json` 保存备份描述。
5. 目标内容确认完整后，按顺序更新 target owner marker、data-side binding、receipt、
   bootstrap binding 和 `dir-config.json`，最后删除旧 owner marker 并完成计划。

阶段 marker 位于 pending 目录，包括：

```text
phase-requested
phase-copying
phase-verified
phase-target-published
phase-bindings-rebound
phase-source-preserved
phase-completed
```

任何阶段崩溃都必须幂等恢复。发现 source 会话树意外消失、目标碰撞、锁失败、空间不足、
文件内容变化或 reparse point 时，继续使用旧 work root；不能写入新 binding，不能触发
hard reset，不能修改 SQLite 或 Provider。失败原因写入受限的 last-status 和 plan metadata。

### 2.3 失败与管理操作

管理 API：

| 方法 | 路径 | 语义 |
| --- | --- | --- |
| `GET` | `/api/system/work-dir-relocation` | 查看当前 operation 和跨卷备份描述 |
| `DELETE` | `/api/system/work-dir-relocation/:operationId` | 取消尚未发布的计划并清理受管残留 |
| `POST` | `/api/system/work-dir-relocation/:operationId/retry` | 清除 paused，立即要求重启并重试 |
| `POST` | `/api/system/work-dir-relocation/:operationId/replace` | 用新目标原子替换尚未发布的计划 |
| `DELETE` | `/api/system/work-dir-relocation/:operationId/backup` | 校验 active target 后手动删除跨卷备份 |

状态分为 `planned`、`copying`、`verified`、`published`、`source_preserved`、
`completed`、`failed`、`paused`、`cancelled`。`failed`/`paused` 优先于低层 phase
marker；`published` 之后不能普通 cancel。

- transient 锁/I/O 错误允许有限次重试；连续失败达到阈值后进入 `paused`。
- deterministic 错误（generation/marker 不匹配、路径碰撞、空间不足、目标被修改）
  直接暂停，等待用户 Retry 或 Replace。
- Cancel 必须先原子写 `cancelled`，再按 operation ownership 和 target identity 清理；
  不能因为目录“看起来为空”就删除外部创建的目标。
- 跨卷备份不自动删除。只有用户明确执行 backup DELETE，且 active target、generation、
  descriptor 和 conversations 完整性都验证通过时才可删除。

`GET /api/system/info` 的 `work_dir_change` 只展示最近一次 completed/failed 结果；
损坏或超限的 last-status 文件采用 best-effort quarantine，不能阻断 system/info。

## 3. Provider、登录与模型环境

### 3.1 两个独立状态

认证状态由 `CloudAuthContext.authState` 表示：`unknown`、`authenticated`、
`unauthenticated`、`offline`。网络错误、超时和 5xx 进入 offline，不得把已确认的
登录状态清成 unauthenticated；只有 401 或服务端明确声明 session/token invalid 才清理认证。

模型环境单独执行：

```text
认证完成
  -> 云模型同步
  -> Provider catalog 刷新
  -> task='chat' resolver
  -> 校验持久化默认模型
  -> ready / degraded / failed
```

task resolver 是“模型可用于对话”的唯一依据。Provider 行中残留的 metadata 只能用于
诊断，不能在 resolver 返回零模型时放行发送：

- resolver 有模型且同步成功：`ready`。
- resolver 有模型但同步或 resolver 有错误：`degraded`，允许继续聊天并显示 warning。
- resolver 返回零模型或解析失败：内部 `modelStatus=failed`，发送被阻止，但设置、日志、
  Provider 配置和退出登录仍可用；Retry 会重新执行完整流程。

登录、退出、账号切换和真实 storage generation 变化才清理 Provider/model SWR cache；
普通 work root 变化不代表新数据集，不能清缓存或触发重新登录。云同步只维护 Flowy
内建 Provider，不禁用或修改用户自建 Provider；退出只禁用内建 Provider。

### 3.2 默认模型规则

- `nomi.defaultModel` 是显式用户偏好，恢复时不得自动写成第一个模型。
- 默认模型暂时不在 resolver 中时，保留原值并提示重新选择；只有用户显式选择才覆盖。
- 已有会话失去绑定时，只能修复到仍被 resolver 确认可用的持久化默认模型。
- 自动修复会话不能再次写默认模型；显式选择才同时更新会话和默认模型。

## 4. 安装级偏好与工作区级数据

`installation-preferences.json` 位于固定 data root，受限为 4 MiB，拒绝 symlink，采用
临时文件、`sync_all`、原子 rename 和一代 `.bak`。安装级键包括：

```text
language
theme
colorScheme
ui.zoomFactor
window.bounds
customCss
css.themes
css.activeThemeId
```

迁移顺序是“先发布并重新读取验证 JSON，再删除 SQLite legacy rows”；无效 legacy 值保留
并告警，主文件损坏时尝试 `.bak`，两者都损坏才 quarantine 后重建。未知 JSON key
原样保留。文件 I/O 在 `spawn_blocking` 中执行，不能把受限文件读写直接放进 Tokio 请求线程。

不要把 `nomi.defaultModel`、Provider、Provider credentials 或工作区聊天数据加入上述
安装级列表；它们必须继续属于当前 v3 SQLite 数据集。

## 5. 开发重启与构建缓存

`bun run dev` 由一个长期 supervisor 管理一份 Vite 和多轮 Tauri CLI：

- Vite 先通过 `http://127.0.0.1:5173/` 健康检查，工作目录重启只替换 Tauri，Vite
  PID 和监听端口保持不变。
- 每轮 Tauri 使用临时控制目录、一次性 64 位 token 和 marker 文件；Rust 的退出码 73
  只是内部开发重启请求，不应作为 supervisor 的系统退出码判断依据。
- marker 合法且 token 匹配才重启；损坏、超限、符号链接、版本错误或 token 不匹配按普通
  退出处理。Ctrl+C、SIGTERM 或 Vite 退出优先清理，不采纳 marker。
- Windows 停止顺序是温和关闭、等待、`taskkill /T`、等待、`taskkill /T /F`；所有路径
  必须等待 close 事件，不能遗留 Tauri/Cargo/rustc 子进程。
- 5 分钟内超过 5 次开发重启会触发熔断。

正常开发/测试入口不再自动 destructive prune：

```bash
bun run build:inspect  # 只读统计
bun run build:gc       # 停止活跃 dev session 后显式回收历史产物
bun run build:clean    # 明确接受冷编译后才执行破坏性清理
```

`build:gc` 或 `build:clean` 之前先确认没有活跃 `dev-session` lock；不要在一次清理
超时后再并发启动第二个清理进程。stable `bun run build` 和 dev channel 的 Cargo cache
不要交替反复切换，最终安装包构建应单独执行一次。

## 6. 变更前后检查清单

修改 relocation、启动、登录或 Provider 代码时，至少确认：

1. 是否误把 data root 当成 work root，或在普通切换中轮换 generation？
2. 是否把 logs、SQLite、密钥、安装偏好或自定义 workspace 放进迁移树？
3. 是否在 Windows 上只做字符串大小写比较，遗漏 UNC、`\\?\`、junction 或嵌套路径？
4. 是否让过期的登录/模型请求写回新账号、新 generation 或新 retry 的状态？
5. 默认模型失效时是否偷偷选择第一个模型或删除原配置？
6. 是否让 WebUI 调用 desktop-only work-dir/restart API？先看 `runtime_capabilities`。
7. 是否新增了 package script 却没有同步 `scripts/scripts.json`？
8. 是否在未停止 supervisor/dev session 时运行了 `build:gc` 或 `build:clean`？

推荐验证顺序：

```bash
bun test scripts/run-dev-supervisor.test.mjs scripts/run-dev-restart-signal.test.mjs scripts/dev-session-lock.test.mjs
bun run check:codemirror-runtime
cargo test -p nomifun-common
cargo test -p nomifun-cloud
cargo test -p nomifun-system --test work_dir_route
bun run typecheck
bun run check
bun run build:ui
bun run build
```

`nomifun-system` 中依赖 AWS rustls native trust store 的测试需要本机有效根证书；若只在
该初始化处失败，应记录为环境门禁，不要把它误判为 relocation 或配置回归。自动化通过
仍不等同于真实验收，最终还要人工验证首次云登录、同卷/跨卷工作目录切换、日志留在 data
root、默认模型恢复和设置页动态模块加载。

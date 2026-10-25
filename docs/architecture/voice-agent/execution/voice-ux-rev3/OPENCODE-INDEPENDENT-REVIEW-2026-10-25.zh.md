> **2026-10-25 宿主启动行为验收更新**：已补齐真实文件库、OS 服务锁和 `AppServices → create_router` 组合启动测试；旧构造期提前结算的临时退化触发 3/3 行为失败，源码逐字恢复后 boot 84 项通过。此前“仅有源码接线证据”的对应缺口已关闭，原生/设备与二进制冷启动仍单列；本节优先于下方历史描述，提交状态以 Git 为准。详见 [宿主启动验证记录](HOST-BOOT-VALIDATION-2026-10-25.zh.md).

# OpenCode 修复独立复审 · 2026-10-25

> 结论：本次 R1–R3 修复及两轮补修的定向代码验收通过，未发现本次范围内仍需阻断的缺陷。该结论不覆盖真实 DOM、原生桌宠、物理声学或完整宿主启动验收。未提交、未推送。
>
> 基准：`feat/voice-next`，HEAD `6663b1ad261b3e997e796cc5ffc75a6e8104c65a`。评审对象是此 HEAD 加当前未提交修改。原有脏工作区保留。

## 1. 修复与独立核查

| 项目 | 核查结果 |
| --- | --- |
| R1 终止证明 | 删除 AppServices 在 proof provider 安装前的无证明结算。恢复沿既有宿主证明路径执行；真实 SQLite 与真实 BootTerminalProofProvider/registry 测试验证 remote、无证明、过期 generation 不被结算。 |
| R1 原子清理 | exact epoch/operation 的生命周期、accepted receipt、thinking/work 消息和必要的 ACP session 清理共用一个 SQLite 事务。ACP、thinking、work 写失败都回滚；原代次仍可枚举，移除故障后可重试。 |
| R1 过期清理隔离 | 旧回执允许被吸收的返回契约与当前代次清理权限分开。旧 A 对后来运行中、已结束或 active-null B 都不能改 B 消息/session/epoch/owner；普通 exact-finalize 契约保持。 |
| R1 长历史 | 不再把 Asc 第一页 5000 条当作全历史；按已有分页完整收集，成功后才原子结算。任意页读取异常在写事务前返回错误。 |
| R2 生产入口恢复 | 测试真实调用 `start_call`，同一 `req-cache-recover` 创建后被 128 项缓存淘汰，关闭并重开文件 SQLite/journal、重建 service/recorder/manager，再经 `start_call` 恢复同一 Conversation。验证原消息与冻结工作区保留，closed 请求不复活、变更模式冲突。模拟边界仅在 daemon IPC，不依赖真实麦克风或模型。 |
| R3 设备 ID | Schema 负例分别绑定非法 linked_device_id + 合法 owner，以及合法 device ID + 非法 owner；不再因参数放错列产生假阳性。未修改迁移或生产 Schema。 |

## 2. 独立反例及最终结果

父审查 Agent 编写并保留了独立于执行者正式测试的真实 SQLite 探针。第二次补修结束后，重新从当前 production rlib 编译原探针，未修改其业务断言：

1. ACP clear 暂时失败：原先 `Reconciled → AlreadyTerminal` 后旧 session 永久残留；现在首次返回 Conflict 并回滚，移除故障后 `Reconciled`，session 清空。
2. 旧 A 对已结束 B 的迟到清理：原先 `AlreadyApplied` 并删除 B session；现在返回契约保持，B 的 `session-of-B` 保留。
3. 5001 条较早历史之后的 interrupted thinking：原先 `Reconciled` 但 session/work 残留；现在 session 清空、thinking 为 `finish`。

最终 **3 passed / 0 failed**。原始 RED 文件 `independent-cleanup-probe-red.txt`、`independent-review-second-probes.txt` 均保留；GREEN 为 `independent-review-final-probes.txt`，不是复用旧二进制或执行者报告。

## 3. 本次独立实跑

证据根目录：`build.noindex/voice-workspace-display-review/opencode-repair-20261025/`。脚本 `independent-final-verification.ps1` 串行执行，退出码和时间保存在 `independent-final-results.json`。

| 命令 / 测试 | 父审查实跑 |
| --- | --- |
| `build-review-probe.ps1`，重编译原始独立探针 | 3 passed，exit 0 |
| `cargo test -p nomifun-conversation --test boot_reconcile_sqlite` | 14 passed，exit 0 |
| `cargo test -p nomifun-db --test conversation_repository` | 91 passed，exit 0 |
| `cargo test -p nomifun-db --test id_schema_contract` | 23 passed，exit 0 |
| `cargo test -p nomifun-app --lib boot_terminal_proof --no-default-features` | 10 passed，exit 0；519 filtered，不是整个宿主包通过 |
| `cargo test -p nomifun-app --lib durable_voice_identity_survives_start_cache_eviction_and_file_reopen --no-default-features` | 1 passed，exit 0；528 filtered |
| `git diff --check`、暂存区检查 | exit 0；未暂存、未提交、未推送 |

合计 **139 项正式定向测试 + 3 项独立探针 = 142 passed**，无失败。日志分别为 `independent-final-{boot,repository,schema,host-proof,start-recovery}.txt` 与独立探针 GREEN 文件。

另外核对执行者最终日志 `second-followup-db-conversation-all.txt`：47 个 test-result 区块合计 **1677 passed / 0 failed / 0 ignored / 0 filtered**；`cargo check -p Flowy` 的执行者日志 exit 0。两项属于已核对的执行者实跑证据，本轮父审查未再重复全量执行。没有将报告数字当作独立复跑。

## 4. 范围与剩余边界

- 旧成果：6 个本次不需再改的既有业务/测试文件 SHA256 与接手快照完全一致；其余必要变更按 diff、调用链和回归核查。文档保留历史原文。没有改迁移、鉴权目录或 agent engine，没有提交、推送、切分支或 rebase。
- 三轮 CLI 事件日志均可解析，未记录派发子 Agent 或 Git commit/push/reset/clean/stash/checkout/switch/rebase/merge 操作。暂存区保持为空，HEAD 未变化。
- CLI exec 63250 与父审查验证 exec 83625 均已 exit 0；收尾进程核查无本任务 OpenCode CLI、Cargo 或 rustc 残留。现存 7 个 OpenCode.exe 属于原已打开的 `@opencode-aidesktop` 应用，未终止或干预。
- `router/state.rs` 的 `include_str` 断言仅是源码结构接线证据。真实 SQLite/proof provider 测试直接调用生产恢复方法，仍不等于完整 `AppServices → router` 启动行为集成，也不等于整机断电演练。
- 分页读取失败通过错误返回发生在写事务前的调用链审查确认；本轮未另外注入“第二页 SELECT 失败”的故障。ACP/thinking/work 写故障及整笔事务回滚有实际 SQLite 触发器反例。
- T4 真实 DOM、透明首帧/DPI/桌宠、麦克风/扬声器仍未签核；不因本轮后端修复通过而宣布 UI 或设备闭环。
- **AEC / 自然插话本轮未实现，属于非目标**，不能描述为已实现且只差硬件测试。
- 未修改 UI，不重跑其 TS/dead-css；此前全仓 TS 61 项及 dead-css 失败仍单列。rustfmt 配置 `disable_all_formatting=true`，配置检查 exit 0 不是排版质量证明。

## 5. 后续

本轮 R1–R3 和独立发现的两轮补修不再交回执行者重复开发。保留未提交工作区供用户审阅；若继续验收，沿现有 `NEXT-VALIDATION-PLAN-2026-10-24.zh.md` 对 T4 及完整宿主启动补行为证据。半小时 OpenCode 监控在本次独立复审完成后停止，不自动扩大任务。

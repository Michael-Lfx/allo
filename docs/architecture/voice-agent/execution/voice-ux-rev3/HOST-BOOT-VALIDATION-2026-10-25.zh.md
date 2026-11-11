# 宿主启动行为验收 · 2026-10-25

> `AppServices → create_router → boot 恢复 → HTTP 路由发布` 的行为证据缺口已补齐。新增三组测试通过；重放旧缺陷时三组全部失败，恢复源码后完整 boot 回归 84 项通过。本节优先于历史文档中“仅有源码接线断言、尚无完整服务/路由启动测试”的描述。
>
> 验证基准：`feat/voice-next`，本轮开始时 HEAD `6663b1ad261b3e997e796cc5ffc75a6e8104c65a` 加此前已复审的工作区修复；提交状态以 Git 为准。

## 1. 新增行为证据

入口：`crates/backend/nomifun-app/src/bootstrap/boot_recovery_tests.rs`，通过 bootstrap 的测试模块接入，无新增生产配置或公开测试构造 API。

测试先在临时文件 SQLite 中经真实 claim/admission/receipt 写入上一轮的运行状态，再关闭数据库、释放并重新取得 OS 服务锁、重新打开同一文件库，调用真实 `AppServices::from_config` 和 `create_router`。不注入 proof provider、不替换 Conversation 服务或运行时，也不设置鉴权绕过。路由返回后通过 `/health` 的真实 Axum 请求确认发布，随后关闭服务并再次打开数据库验证持久状态。

| 场景 | 预期与实测 |
| --- | --- |
| 有匹配的真实服务锁及有效 boot authority | Nomi/ACP 的中断代次恢复为 finished；receipt 以 `interrupted_by_restart` 完成；epoch +1、owner 清空；用户原文和已积累思考保留；断流消息终态且 ACP session 清空。 |
| remote 与无本地登记的 gateway | 即使宿主拥有服务锁，也不据此推断外部任务已终止；receipt、epoch、owner、消息及 session 全部不变。 |
| 只有 running marker，思考已经完成 | 允许证明后的生命周期收口，保留健康的 ACP session。 |
| fixture 持有 OS 锁但未向服务交付 boot authority | 五个 Conversation 全部隔离，不能仅凭数据目录、锁文件存在或运行时缺席结算。 |
| 进程登记文件无法解析 | 五个 Conversation 全部隔离，不因登记读取失败降级为“没有进程”。 |

三个独立测试各包含上述五种持久 Conversation：中断 Nomi、中断 ACP、remote、gateway、健康 ACP marker。所有场景同时检查构造期未提前结算、恢复结果经文件重开保留、用户输入不丢以及恢复没有创建模型运行时。每次收尾均关闭数据库并成功重新获取同一 OS 服务锁。

## 2. 测试敏感性验证

为了确认不是“永远通过”的接线测试，临时在 `AppServices` 构造期恢复旧的无证明 sweep，直接结算枚举到的运行代次；然后运行同一组三个测试。

- 退化运行：**0 passed / 3 failed**，cargo exit 101；失败断言均为 `AppServices must not settle turns before boot proof exists`。
- 使用 `finally` 将 `services.rs` 恢复为实验前的原始字节，并核对 SHA256 相同。
- 最终恢复运行：boot **84 passed / 0 failed / 0 ignored**，447 filtered；其中包含新增三组完整启动测试。
- 此前未提交变更中那项 `include_str` 检查 AppServices/provider 顺序的字符串断言已被行为测试替换；其他有独立目的的启动结构测试保留。原有 boot 82 项 + 新增 3 项 - 被替换的 1 项 = 84。

证据根目录：`build.noindex/voice-workspace-display-review/host-boot-verification-20261025/`。

关键文件：`composition-baseline.txt`（初次行为运行 3 passed）、`composition-mutation-red.txt`、`composition-mutation-exit.txt`、`mutation-source-restored.txt`、`boot-final.txt`、`boot-final-exit.txt`。最初 `composition-first.txt` 是测试导入路径编译错误，已更正，不计为行为 RED。

## 3. 最终回归

| 检查 | 本轮结果 |
| --- | --- |
| `cargo test -p nomifun-app --lib boot --no-default-features -- --test-threads=1` | **84 passed / 0 failed / 0 ignored**，exit 0 |
| `cargo test -p nomifun-app --lib voice_ --no-default-features` | **92 passed / 0 failed / 3 ignored**，既有忽略不计通过，exit 0 |
| `cargo test -p nomifun-app --test voice_dialogue_integration --no-default-features` | **32 passed / 0 failed**，exit 0 |
| `cargo check -p Flowy` | exit 0；桌面 feature 编译通过，未启动原生窗口 |
| error/support surface、process/browser boundary、Windows console、script registry | **6 项均 exit 0**，见 `gates.json` |
| 新测试真实 rustfmt 检查 | exit 0；仅对新文件显式启用格式化，不改变仓库全局配置 |

交付回执为 `delivery-checks.json`。既有 DB/Conversation **1677 项**和三项独立 SQLite 探针的证据沿用[前一轮独立复审](OPENCODE-INDEPENDENT-REVIEW-2026-10-25.zh.md)，本轮不重复计算为新通过数量：生产 `boot.rs`、`sqlite_conversation.rs` 和 `voice_companion_manager.rs` 的 SHA256 与前轮验收一致，临时退化修改已逐字恢复。本轮最终未新增生产启动逻辑修复，新增的是实际组合启动行为覆盖。

## 4. 交付与未覆盖范围

- 此前已审查的事务清理、代次隔离、长历史分页、持久 start_call 身份、技能失败收尾和 Schema 反例修复一并交付；不覆盖、回滚或丢弃接手时的成果。
- 本轮使用临时数据和真实 OS 锁，但仍是同进程的重开与组合启动验证；HTTP 请求通过 `Router::oneshot` 执行，没有监听真实 TCP 端口，不宣称完成二进制冷启动、系统断电或 Tauri 窗口验收。
- T4 DOM、原生透明首帧/DPI/桌宠、物理麦克风及扬声器仍未签核。AEC/自然插话仍未实现，非本轮目标。
- 无前端变更，不修改或掩盖既有 TS 61 条及 dead-css 失败基线；不把本轮定向回归描述为全仓所有门禁通过。
- 下一步若继续验收，沿现有 `NEXT-VALIDATION-PLAN-2026-10-24.zh.md` 处理剩余原生/设备范围，不从已完成的后端恢复任务重新开工。

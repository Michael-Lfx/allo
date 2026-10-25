> **2026-10-25 宿主启动行为验收更新**：已补齐真实文件库、OS 服务锁和 `AppServices → create_router` 组合启动测试；旧构造期提前结算的临时退化触发 3/3 行为失败，源码逐字恢复后 boot 84 项通过。此前“仅有源码接线证据”的对应缺口已关闭，原生/设备与二进制冷启动仍单列；本节优先于下方历史描述，提交状态以 Git 为准。详见 [宿主启动验证记录](HOST-BOOT-VALIDATION-2026-10-25.zh.md).

> **2026-10-25 第二次补修（最高优先，覆盖下方全部“已关闭”结论）**：独立复审确认上一补修仍有两个真实 SQLite 缺陷，均已修复。① 过期 A 清理曾可删除后来已结束 B 的 ACP session/消息：`apply_restart_orphan_artifacts` 在无 epoch 校验的 `AlreadyApplied` 分支执行；现只有精确 `epoch == expected_admission_epoch` 才允许清理，旧 A 只保留原回执吸收/返回契约，B 运行中或已结束均不被触碰（含 active-null/ABA）。② `boot.rs` 只取 Asc 第一页 5000 条，长历史尾部的断流 thinking/session 被永久遗漏；现按现有 `get_messages` 分页遍历完整历史，任一页失败仍在事务开始前 fail-closed。新增正式测试覆盖旧 A 对已结束 B/运行中 B/active-null B 不可改、以及 5001 条历史尾部清理。最新：`boot_reconcile_sqlite` **14 passed**；两 crate all-target **47 targets / 1677 passed / 0 failed / 0 ignored**；Conversation lib 648、DB lib 458、schema 23；host proof 10；R2 1；`cargo check -p Flowy` exit 0。`router/state.rs` 的 `include_str` 仍只是源码结构防线，不是完整 AppServices→router 行为集成。T4 真实 DOM/原生/设备与 AEC 仍未验。未提交、未推送。证据：`build.noindex/voice-workspace-display-review/opencode-repair-20261025/SECOND-FOLLOWUP-FINAL_REPORT.zh.md`。

> **2026-10-25 独立复审 follow-up（最高优先）**：首轮“生命周期先提交、transcript/ACP 再 best-effort 清理”的 cutpoint 已修复。现在精确旧 epoch/operation 的 CAS、accepted receipt 结算、thinking/work 消息终态与 ACP session 清理在**同一 SQLite 事务**中提交；清理失败或事务内崩溃会整体回滚，代次继续 `running` 并可被下一次 boot 重试，不再出现“Reconciled 但永久丢失清理”。新增 ACP clear、thinking 清理、stranded work 消息三组真实 SQLite RED→GREEN；`boot_reconcile_sqlite` **10 passed**，两 crate all-target **47 targets / 1673 passed / 0 failed / 0 ignored**，宿主 voice **92 passed / 3 ignored**，recorder **32 passed**，`cargo check -p Flowy` exit 0。`router/state.rs` 的 `include_str` 仅作源码结构接线防线，不是完整 AppServices→router 行为集成；行为证据为真实 SQLite 恢复/回滚与真实 host proof provider 测试。T4 真实 DOM/原生/设备与 AEC 仍未验。未提交、未推送。证据：`build.noindex/voice-workspace-display-review/opencode-repair-20261025/FOLLOWUP-FINAL_REPORT.zh.md`。

> **2026-10-25 独立审查修复（最高优先）**：R1 启动结算已统一到 proof-aware 恢复流程——`AppServices` 构造期不再结算 `running` 代次；只有真实 durable registry reap + 冻结快照证明后，才按精确旧 epoch/operation 结算，无证明时状态、回执、ACP session 与消息全部保留。R2 缓存淘汰演练已改为真实 recorder + service + manager 的生产 `start_call` 路径；R3 schema 反例已绑定正确列。all-target 47 targets / 1670 passed / 0 failed / 0 ignored；Conversation lib 648、DB lib 458、schema 23；宿主 voice 92 / 3 ignored；recorder integration 32；`cargo check -p Flowy` exit 0。真实 DOM/原生/设备与 AEC 仍未验。证据：`build.noindex/voice-workspace-display-review/opencode-repair-20261025/`。

目标：关闭剩余自动化失败，补齐语音会话与桌宠验证。先读本目录 PROGRESS.md 和 REPAIR-VERIFICATION-2026-10-24.zh.md 的 next-validation-execution 节；中断后从进度续跑。优先级：数据安全与身份隔离 > 契约正确 > 验证完整 > 速度。

## 1. 我替用户确定的执行方式

- 单 Agent 串行，不派发会话，不并行写 Cargo 缓存。
- 延续当前工作树，不重做已通过修复。未经本次用户另行授权，不提交、推送、切分支或 rebase。
- Agent 完成代码、自动化和文档；设备听感与主观满意度留人工。原生自动化可用则自行验证；能力缺失不冒称通过，不阻断独立项。

## 2. 修改界限

仓库 D:\workSpace\git_clone_test\allo；规范以 AGENTS.md、CONTRIBUTING.md 为准。白名单：
- crates/backend/nomifun-conversation/tests/boot_reconcile_sqlite.rs、acp_artifact_turn_history.rs、stream_relay_tool_call.rs、voice_placeholder_lifecycle.rs；src/boot.rs、service.rs、service_test.rs（仅本任务证实的恢复/接纳缺陷）。
- crates/backend/nomifun-db/tests/id_schema_contract.rs、conversation_repository.rs；src/repository/sqlite_conversation.rs（仅本任务证实的契约缺陷）。
- crates/backend/nomifun-app/src/voice_companion_manager.rs、voice_tasks.rs、voice_task_runtime.rs；tests/voice_dialogue_integration.rs。
- ui/src/renderer/pages/voiceSession/、pages/conversation/SessionList/、components/chat/、pages/companion/ 与 hooks/useCompanionWindowsSync*：限已有交互的验证与复现缺陷；国际化必要改动仅 services/i18n/locales/zh-CN、en-US 和生成类型。
- apps/desktop/src/ 内仅现有桌宠窗口相关文件及其测试；本目录和 voice-agent 两级 README/current-implementation-status 文档。
其他路径只读。迁移、鉴权、权限策略、发布、全局工具配置、harness、新依赖、ASR/TTS/AEC 不在授权内。保留旧改动。允许修正协议夹具、增加反例；schema 数字须先取证，原安全/隔离断言保持。

## 3. 现状与任务 0

2026-10-24 核验：feat/voice-next，HEAD 6663b1ad261b3e997e796cc5ffc75a6e8104c65a，领先 origin 2；接续前 17 项修改另加本任务书，未提交。先 git status --short --branch、git diff --check，保存差异/哈希到忽略目录；记录漂移，不 reset/stash 旧成果。开工写 ≤10 行目标、顺序、最大风险。
既有回执：Conversation lib 648；生命周期 15；宿主 voice 91+3 ignored；recorder 32；DB lib 458、仓储 91、备份 16；前端 164；桌宠逻辑/结构 38。技能加载卡住已修好。
扩大回归历史 1660 pass/4 fail；随后 tool retry 已 2/2 通过。当前剩 boot 两项和 schema 一项未关闭。TS 61 条逐条差分新增 0；dead-css 失败。DOM/原生效果未签核。日志在 build.noindex/voice-workspace-display-review/，不依赖它才能运行正式测试。

## 4. 执行顺序

T1 启动恢复：先运行 cargo test -p nomifun-conversation --test boot_reconcile_sqlite。当前 OWNER="user_0190..." 在 UUID parser 前失败；不能只删前缀。按真实 claim/admission/receipt 协议构造 running 崩溃状态，调用真实恢复入口，验证消息保留、思考终态、精确回执/运行状态、真实中断会话与健康会话的区别及恢复幂等。若现有 boot 入口与权威恢复不一致，追踪实际宿主调用链再最小修复，不能绕过触发器。保留 RED→GREEN，目标至少原 2 项通过。

T2 Schema：运行 cargo test -p nomifun-db --test id_schema_contract。实际 121、预期 117；列出新增 4 张表、创建来源和提交，逐表核对整数自增主键、业务 ID、逻辑引用和契约登记。核对无误才更新数量并保留逐表断言；有 schema 缺陷而需迁移则写 BLOCKED，不改旧迁移/关闭约束。使用隔离内存库验证错误约束仍被拒绝。

T3 恢复链路：扩展现有宿主/recorder 测试，把 >128 次缓存淘汰、文件 SQLite/journal 重开和持久创建身份串起来；已关闭请求不复活、配置改变冲突、unknown 不重发、已接受消息不丢失。模拟模型/daemon 边界须标注，不能 mock 掉被验证的存储和仲裁。

T4 交互与原生：沿 NEXT-VALIDATION-PLAN 第 4/5 节，验证 More 点击/键盘/Escape 焦点恢复、菜单不导航、窄屏不重叠、双语双主题/reduced-motion；首页新建与续聊、A 通话浏览 B 的归属、工作区隔离、输入框主键/胶囊和草稿。桌宠验证 owner、临时显示/结束恢复隐藏、正确形象/位置后显示、透明首帧与 DPI。只用合法 runner；已有本地文件探针被拒绝，不换协议/工具绕过。组件注入、真实宿主、纯几何、原生画面分记；缺工具留复现步骤，不把静态渲染当真实点击，不改 Web 通话能力边界。

T5 回归与收口：串行运行：
cargo test -p nomifun-db -p nomifun-conversation --no-fail-fast
cargo test -p nomifun-app --lib voice_ --no-default-features
cargo test -p nomifun-app --test voice_dialogue_integration --no-default-features
cargo check -p Flowy
前端沿 RESULTS.zh.md 现有 21 文件命令及修复记录的 6 文件命令复跑；修改 TS 时跑 typecheck 并逐条比对基线，按实际改动跑 i18n/theme/icons/button-layout/motion/dead-css 和 build:ui。没有改 UI 则不重复生产打包。命令/脚本有漂移先核实，不能编造通过。格式检查用真实 rustfmt 二进制；当前配置 disable_all_formatting=true，不能声称完成全仓格式化。

## 5. 执行规矩

Windows 已验证工具：C:\Users\38788\.rustup\toolchains\1.97.1-x86_64-pc-windows-msvc\bin\cargo.exe、同目录 rustfmt.exe；C:\Users\38788\.bun\bin\bun.exe。先校正本进程 PATH，避开 mise shim；此前 cargo-fmt 曾递归生成进程。无输出及时核对 PID/命令，限时并仅清理自己子树，确认无残留；不盲等/重复启动。Bun 从 ui/ 用明确 ./ 测试路径，避免递归匹配 debug 基线副本。
不删测、skip/todo、放松安全断言、伪造状态、更新 CSS/TS 基线、吞退出码或要求用户反复口述代跑逻辑验证。保留既有 ignored，不增加；通过数下降查原因。同一失败连续 3 次无新证据则记 BLOCKED，转做独立项；不能回滚接手前工作。无需扩展范围的修复直接完成，高风险/越界项保留证据待裁决。

## 6. 完成条件与交付

1. 原剩余 3 项取得真实 GREEN，且两个 crate 全目标 exit 0；或明确记录无法安全关闭的具体原因，状态只能 partial，不能宣称完成。原已通过专项不退化，每项有命令、退出码、pass/fail/ignored/filtered、耗时和日志，关键缺陷有 RED→GREEN。
2. 工作区旧改动完整、无越界修改/新增跳过/进程残留；文档与实际证据一致。更新 PROGRESS、RESULTS、BLOCKED（无则写无）和修复记录，历史原文保留；列出 DOM/原生/设备尚未签核项、人工最短步骤与下一步。最终回复贴关键真实输出摘要，区分已完成、阻断、人工验收；不提交/推送，交独立审查。

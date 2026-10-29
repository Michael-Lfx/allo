> **2026-10-25 宿主启动行为验收更新**：已补齐真实文件库、OS 服务锁和 `AppServices → create_router` 组合启动测试；旧构造期提前结算的临时退化触发 3/3 行为失败，源码逐字恢复后 boot 84 项通过。此前“仅有源码接线证据”的对应缺口已关闭，原生/设备与二进制冷启动仍单列；本节优先于下方历史描述，提交状态以 Git 为准。详见 [宿主启动验证记录](HOST-BOOT-VALIDATION-2026-10-25.zh.md).

> **2026-10-25 第二次补修（最高优先，覆盖下方全部“已关闭”结论）**：独立复审确认上一补修仍有两个真实 SQLite 缺陷，均已修复。① 过期 A 清理曾可删除后来已结束 B 的 ACP session/消息：`apply_restart_orphan_artifacts` 在无 epoch 校验的 `AlreadyApplied` 分支执行；现只有精确 `epoch == expected_admission_epoch` 才允许清理，旧 A 只保留原回执吸收/返回契约，B 运行中或已结束均不被触碰（含 active-null/ABA）。② `boot.rs` 只取 Asc 第一页 5000 条，长历史尾部的断流 thinking/session 被永久遗漏；现按现有 `get_messages` 分页遍历完整历史，任一页失败仍在事务开始前 fail-closed。新增正式测试覆盖旧 A 对已结束 B/运行中 B/active-null B 不可改、以及 5001 条历史尾部清理。最新：`boot_reconcile_sqlite` **14 passed**；两 crate all-target **47 targets / 1677 passed / 0 failed / 0 ignored**；Conversation lib 648、DB lib 458、schema 23；host proof 10；R2 1；`cargo check -p Flowy` exit 0。`router/state.rs` 的 `include_str` 仍只是源码结构防线，不是完整 AppServices→router 行为集成。T4 真实 DOM/原生/设备与 AEC 仍未验。未提交、未推送。证据：`build.noindex/voice-workspace-display-review/opencode-repair-20261025/SECOND-FOLLOWUP-FINAL_REPORT.zh.md`。

> **2026-10-25 独立复审 follow-up（最高优先）**：首轮“生命周期先提交、transcript/ACP 再 best-effort 清理”的 cutpoint 已修复。现在精确旧 epoch/operation 的 CAS、accepted receipt 结算、thinking/work 消息终态与 ACP session 清理在**同一 SQLite 事务**中提交；清理失败或事务内崩溃会整体回滚，代次继续 `running` 并可被下一次 boot 重试，不再出现“Reconciled 但永久丢失清理”。新增 ACP clear、thinking 清理、stranded work 消息三组真实 SQLite RED→GREEN；`boot_reconcile_sqlite` **10 passed**，两 crate all-target **47 targets / 1673 passed / 0 failed / 0 ignored**，宿主 voice **92 passed / 3 ignored**，recorder **32 passed**，`cargo check -p Flowy` exit 0。`router/state.rs` 的 `include_str` 仅作源码结构接线防线，不是完整 AppServices→router 行为集成；行为证据为真实 SQLite 恢复/回滚与真实 host proof provider 测试。T4 真实 DOM/原生/设备与 AEC 仍未验。未提交、未推送。证据：`build.noindex/voice-workspace-display-review/opencode-repair-20261025/FOLLOWUP-FINAL_REPORT.zh.md`。

> **2026-10-25 更新（优先）**：独立审查 R1/R2/R3 已修复并验证，详见 [独立审查修复记录](REPAIR-VERIFICATION-2026-10-24.zh.md#independent-review-repair-2026-10-25)。R1 不再由 `services.rs` 无证明结算；R2 已覆盖生产 `start_call` 恢复；R3 反例已绑定正确列。剩余范围仅为 T4 真实 DOM/原生/DPI/桌宠姿态/物理声学，以及既有 TS/dead-css 基线；AEC/自然插话仍非本轮目标。

# rev3 下一步验证与修复计划

> **2026-10-25 接续收口（最新）**：P0（技能卡住）、P1（持久身份组合链路）、P4 中原剩余 boot/schema 自动化项已关闭；`nomifun-db + nomifun-conversation` all-target 1666 passed / 0 failed，宿主 voice 92 passed / 3 ignored，recorder integration 32 passed，前端 164、桌宠逻辑 38 通过。P2 真实 DOM 仍被 Web 云端登录门禁阻断，P3 原生透明首帧/DPI/桌宠姿态与物理声学仍未签核；TS 61 条和 dead-css 仍是既有失败。精确命令、退出码和日志见 [2026-10-25 接续收口记录](REPAIR-VERIFICATION-2026-10-24.zh.md#continuation-2026-10-25)。下文计划原文保留为历史。

> **2026-10-24 接续执行状态（优先）**：已按用户授权执行 P0/P1 和 P4 定向验证，不再是待开工。技能事件契约/失败收尾死锁、迟到 view-only 序号与普通 extra 替换回归已修复；生命周期 15、recorder 32、宿主 91、Conversation lib 648、前端 164、桌宠逻辑 38 项通过。P2 真实 DOM 与 P3 原生首帧/DPI 因当前工具能力缺口仍未签核；扩大普通会话回归的失败另列，不能称全量通过。当前事实统一见 [接续执行记录](REPAIR-VERIFICATION-2026-10-24.zh.md#next-validation-execution)，下文待执行文字按原计划存档。当前修改未提交、未推送。

> **状态**：待执行；不是新增用例的通过记录。
> **已提交代码基准**：`8f62353ea386d483fa26f911b62000191d9e3d5a`，分支 `feat/voice-next`，未推送。
> **对照基准**：修复前 `4aca4cd11333a0d4151d03fb8941186c1017462b`。
> **事实入口**：[修复验证记录](REPAIR-VERIFICATION-2026-10-24.zh.md)。本计划在代码提交后编写；本轮只提交计划，不继续实施下一阶段。

## 1. 验收边界

目标是证明本轮修复可稳定运行，并消除剩余证据缺口，不继续扩大语音功能。保持当前 ASR/TTS、半双工边界、单通话、共享 Conversation、YOLO 优先级与用户显式模式、工作区/owner 冻结规则。

不新增流式字幕、ASR 模型、生产 AEC、权限模式、持久化 migration、独立语音历史或新状态管理框架；不把全仓历史 CSS/类型问题混入语音修复。所有缺陷先复现再最小修复，不通过删测试、放松断言或更新基线掩盖失败。

| 顺序 | 验证目标 | 当前事实 | 完成条件 |
| --- | --- | --- | --- |
| P0 | 3 项技能加载测试卡住归因 | 完整 suite 卡住；单项 45 秒仍未结束 | 定位具体等待边界，完成旧/新提交对照；修复后相关测试与完整 Conversation 通过，或证明为既有问题并单独跟踪 |
| P1 | 生命周期与持久身份完整链路 | 11 项生命周期、30 项 recorder 集成通过 | 补齐 send/task 竞争、真实重启/缓存淘汰与有效 preset 证据，不发生已接受消息丢失 |
| P2 | 组件真实交互 | 164 项前端通过，主要是静态渲染/适配器；浏览器探针未成功运行 | More/焦点/布局/归属获得合法环境下的实际交互断言与截图 |
| P3 | 桌面原生联动 | 桌面编译通过，窗口/桌宠未签核 | 系统级窗口事件、透明首帧、owner、位置及 DPI 回执可追溯 |
| P4 | 收口与提交 | TS 61 条无新增；dead-css 失败 | 每类结果分别记录；全量失败不伪装为通过；最小修复分提交 |

## 2. P0：先定位技能加载卡住

具体用例位于 `crates/backend/nomifun-conversation/src/service_test.rs`：

```text
service_test::first_turn_loads_source_qualified_preset_skills_without_filtering_the_launcher_request
service_test::send_message_allows_large_explicit_skill_snapshot
service_test::send_message_allows_load_only_catalog_skills_and_persists_the_snapshot_ledger
```

逐个串行执行，不先并发跑全仓：

```powershell
cargo test -p nomifun-conversation --lib service_test::send_message_allows_large_explicit_skill_snapshot -- --exact --nocapture --test-threads=1
```

另两项替换完整测试名。编译时间与测试运行时间分开；为测试子进程设置外层 60 秒 watchdog，超时保存 stdout/stderr 并只清理自己启动的进程，确认清理完成。不能只用 Tokio timeout，因为同步锁死或 runtime teardown 也可能让它失效。

1. 记录进入/退出边界：catalog resolve → runtime options/build → durable claim/admission → SQLite skill/user-message 提交 → stream/turn release → fixture/runtime teardown。临时诊断只输出测试身份、阶段和耗时，不输出私有内容。
2. 用独立临时 checkout 对照 `4aca4cd11` 与 `8f62353ea`；保持相同 Rust 工具链、Cargo feature、环境变量和测试过滤。不得 reset 当前工作树；如复用构建目录，两个 checkout 必须串行构建，禁止同时写同一 target。
3. 旧提交通过、新提交卡住：阻断交付，定位本轮回归并最小修复。两者都卡住：取得双份日志后才能归为既有问题；将修复独立提交，保留语音代码基准，不扩大为技能系统重构。仅在新提交复现而未跑旧提交，不得标注“基线问题”。
4. 涉及锁/事务时加入可控 barrier 的确定性回归，而不是增加 sleep 或把 timeout 改大。Fixture 缺少结束信号时补完整的 fixture 协议；实际业务死锁则修共享边界，不能只让测试绕开。
5. 完成后运行无过滤的 `cargo test -p nomifun-conversation --lib`；预期现有 647 项都有真实终态，按实际数量记录。保留 644 passed + 3 filtered 只能用于定位，不能作为关闭该项的证据。

## 3. P1：补齐服务与宿主边界证据

优先扩展已有两个正式测试文件，不另建平行实现：

- `crates/backend/nomifun-conversation/tests/voice_placeholder_lifecycle.rs`
- `crates/backend/nomifun-app/tests/voice_dialogue_integration.rs`

| 场景 | 必须断言 |
| --- | --- |
| close 与键盘 send / voice-bound task 同时到达 | 只能接纳并 retained，或先关闭再明确拒绝；消息、receipt、active turn 和队列 ledger 一致，没有成功回执但消息消失，也没有 discarded 行偷偷执行 |
| 真实文件 SQLite + journal 重启 | 在同一临时数据目录重建 service/recorder/manager；旧 request 不复活，未确认输入不丢弃，恢复只查回执而不重复发送 |
| start 缓存超过 128 项后重试 | 关闭的持久创建身份仍有效；改变 workspace/model/preset/mode 冲突；不是仅通过内存缓存返回旧结果 |
| 有效 preset | 使用既有真实 preset fixture/service，验证 normal-create 与 voice-create 的权威解析语义一致；冻结 resolved model/snapshot。失效 preset/不支持的 agent 明确拒绝 |
| 多工作区 registry 并发 | A 迟到消息发布与 B 解析/发布之间受同一锁保护，两项都存在；provisional 不覆盖 retained 指针；旧终态 tick 不覆盖较新交互 |
| 关闭后的查询 | 使用真实 list/search/count/pagination，关闭行不出现、total 一致；旧的无 lifecycle 字段会话不被清理 |
| 主窗口 view 与自动候选 | 去设置只改变 view，不清空上次工作区；失焦/乱序上报不覆盖较新页面；未上报 view、断线、unknown、任务/审批未决均不清理 |

每项先固化反例；避免用模拟 repo 返回目标状态代替真实 SQLite 竞争。跨服务边界可替换模型执行，但不能替换正在验证的存储、回执或仲裁层。测试每次使用独立临时目录，故意打断后的残留也应可恢复。

## 4. P2：真实交互验收，不冒充声学验证

入口是现有隔离探针 `ui/src/renderer/pages/voiceSession/tests/voiceComposerRev3.browser-probe.tsx`，不导入生产路由。只在工具明确允许的应用/组件测试环境执行；若当前环境仍不支持则保持阻断。不得把上次被拒绝的本地文件换协议/换工具加载以绕过策略。

补齐以下行为：

1. More：鼠标点击、Tab 聚焦、Enter/Space、Shift-F10/ContextMenu、Escape 后焦点恢复；点击菜单不触发行导航，根节点没有重新挂载；语音行 hovercard 与菜单互斥。
2. 布局：长中文/英文标题、最窄支持宽度、置顶/能力/未读/执行状态、折叠/批量/触屏；比较 hover/focus/menu 前后的标题与操作槽 bounding box，验证不跳动、不重叠。
3. 活动归属：A 通话时浏览 B、切工作区、重命名/置顶、虚拟行复用、多个订阅迟到 seed；始终只标记 A，B 启动返回明确冲突/返回通话，不能悄悄挂断 A。
4. 输入框：有字发送、无字通话、执行中停止、小挂断接替；胶囊不挡首行/技能/工具；syncing、offline、ended、无声学 readiness、审批/队列/unknown 均可区分。
5. 首页新建：默认/项目各连续两通有效对话得到不同 ID；空占位结束并离页后关闭；已有活动通话只返回原通话；未发草稿不丢失、不自动发送。
6. zh-CN/en-US × light/dark；reduced-motion 下声波静止但活动连接点/可访问名称仍存在。

录制测试请求/受控状态可用于组件验收；每份回执明确标注“组件注入”或“真实宿主”。截图只证明对应布局，不证明麦克风、ASR 或透明窗口工作。

## 5. P3：系统验证与桌宠联动

复用桌面启动与原生窗口机制，在隔离测试数据目录运行；不让测试写入用户真实 Conversation 库。

- 打开通话时临时显示匹配 owner 的桌宠，结束恢复用户隐藏偏好；首帧前形象已确定，不能团子闪成 Bolt。
- 原生窗口初始化即恢复有效保存位置，透明就绪前不 show；记录 show/位置/资源 ready 的时序并留实际画面，避免只凭布尔 flag 判通过。
- 100/125/150% DPI 及负坐标副屏、显示器移除后的可见区域恢复；没有第二块屏时只验证可计算布局，跨屏实测保持未签核。
- 原生 show/hide、主窗口导航/失焦和通话结束事件经真实桥接传播；挂断与后台任务取消独立。

由支持原生窗口 API 的系统测试 runner 取得回执；CLI 无法操作原生显示时保留未签核，不转成要求用户反复口述测试。当前目标不包含 AEC，不能借本轮 UI 结果宣称可自然插话。

## 6. P4：验证命令、记录和交付

按修改范围执行：

```powershell
cargo test -p nomifun-conversation --test voice_placeholder_lifecycle
cargo test -p nomifun-app --test voice_dialogue_integration --no-default-features
cargo test -p nomifun-app --lib voice_ --no-default-features
cargo test -p nomifun-db --lib
cargo test -p nomifun-conversation --lib
cargo check -p Flowy
bun run typecheck
bun run check:i18n
bun run check:theme
bun run check:icons
bun run check:motion
bun run check:dead-css
bun run build:ui
git diff --check
```

前端沿 `RESULTS.zh.md` §2.1 的 21 文件命令复跑；该文件旧结果只是历史，新增证据更新至当前验证记录。改过 bridge/进程边界时额外执行对应仓库 boundary gate；没有新变化或失败时不反复全仓编译。

类型错误逐条与 61 条基线比较，而非只比总数；dead-css 对照报告与修改文件，新增问题必须修，未动文件单列。不得降低门禁/扩大 baseline 让它变绿。

每个阶段记录：提交 SHA、命令、退出码、通过/忽略/过滤数量、日志路径、测试类型和未签核项。P0 修复、P1 必要代码补强、P2/P3 测试设施和纯文档分别小提交，保留现有自然人署名门禁。未收到推送或下一阶段实施指令前，不推送、不改业务、不运行本计划。

## 附录：保留的基线

代码提交前的真实证据为：前端 164、生命周期 11、recorder 集成 30、宿主 voice 90（另 3 ignored）、DB 458 通过；Conversation 644 通过但 3 filtered；桌面编译/UI 构建通过；TS 61 条无新增；dead-css 失败；浏览器/原生未签核。完整背景逐字保留于同目录历史报告与修复验证记录，不从未执行的计划推导通过结论。

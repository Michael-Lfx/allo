> **2026-10-25 宿主启动行为验收更新**：已补齐真实文件库、OS 服务锁和 `AppServices → create_router` 组合启动测试；旧构造期提前结算的临时退化触发 3/3 行为失败，源码逐字恢复后 boot 84 项通过。此前“仅有源码接线证据”的对应缺口已关闭，原生/设备与二进制冷启动仍单列；本节优先于下方历史描述，提交状态以 Git 为准。详见 [宿主启动验证记录](HOST-BOOT-VALIDATION-2026-10-25.zh.md).

> **2026-10-25 第二次补修（最高优先，覆盖下方全部“已关闭”结论）**：独立复审确认上一补修仍有两个真实 SQLite 缺陷，均已修复。① 过期 A 清理曾可删除后来已结束 B 的 ACP session/消息：`apply_restart_orphan_artifacts` 在无 epoch 校验的 `AlreadyApplied` 分支执行；现只有精确 `epoch == expected_admission_epoch` 才允许清理，旧 A 只保留原回执吸收/返回契约，B 运行中或已结束均不被触碰（含 active-null/ABA）。② `boot.rs` 只取 Asc 第一页 5000 条，长历史尾部的断流 thinking/session 被永久遗漏；现按现有 `get_messages` 分页遍历完整历史，任一页失败仍在事务开始前 fail-closed。新增正式测试覆盖旧 A 对已结束 B/运行中 B/active-null B 不可改、以及 5001 条历史尾部清理。最新：`boot_reconcile_sqlite` **14 passed**；两 crate all-target **47 targets / 1677 passed / 0 failed / 0 ignored**；Conversation lib 648、DB lib 458、schema 23；host proof 10；R2 1；`cargo check -p Flowy` exit 0。`router/state.rs` 的 `include_str` 仍只是源码结构防线，不是完整 AppServices→router 行为集成。T4 真实 DOM/原生/设备与 AEC 仍未验。未提交、未推送。证据：`build.noindex/voice-workspace-display-review/opencode-repair-20261025/SECOND-FOLLOWUP-FINAL_REPORT.zh.md`。

> **2026-10-25 独立复审 follow-up（最高优先）**：首轮“生命周期先提交、transcript/ACP 再 best-effort 清理”的 cutpoint 已修复。现在精确旧 epoch/operation 的 CAS、accepted receipt 结算、thinking/work 消息终态与 ACP session 清理在**同一 SQLite 事务**中提交；清理失败或事务内崩溃会整体回滚，代次继续 `running` 并可被下一次 boot 重试，不再出现“Reconciled 但永久丢失清理”。新增 ACP clear、thinking 清理、stranded work 消息三组真实 SQLite RED→GREEN；`boot_reconcile_sqlite` **10 passed**，两 crate all-target **47 targets / 1673 passed / 0 failed / 0 ignored**，宿主 voice **92 passed / 3 ignored**，recorder **32 passed**，`cargo check -p Flowy` exit 0。`router/state.rs` 的 `include_str` 仅作源码结构接线防线，不是完整 AppServices→router 行为集成；行为证据为真实 SQLite 恢复/回滚与真实 host proof provider 测试。T4 真实 DOM/原生/设备与 AEC 仍未验。未提交、未推送。证据：`build.noindex/voice-workspace-display-review/opencode-repair-20261025/FOLLOWUP-FINAL_REPORT.zh.md`。

> **2026-10-25 独立审查修复更新（最高优先）**：独立审查确认的 R1 启动隔离缺陷、R2 恢复入口缺口、R3 反例绑定错误均已修复并验证。`AppServices` 构造期不再结算 `running` 代次；remote/未注册 gateway/registry 失败/过期代次保持隔离，只有真实 durable registry reap + 冻结快照证明后才按精确旧 epoch/operation 结算。all-target 47 targets / 1670 passed / 0 failed / 0 ignored；宿主 voice 92 / 3 ignored；recorder integration 32。下方“另有架构边界提请独立审查：boot.rs 仍早于证明扫描”已被本次修复取代，不再成立。剩余阻截仍为真实 DOM/原生/DPI/桌宠姿态/物理声学，以及 TS/dead-css 既有基线；AEC/自然插话本轮未实现。证据：[独立审查修复记录](REPAIR-VERIFICATION-2026-10-24.zh.md#independent-review-repair-2026-10-25)。

# 语音交互 rev3 未签核与物理环境边界说明

> **2026-10-25 接续收口更新**：原剩余 boot 启动恢复 2 项与 schema 契约 1 项均已真实 GREEN，扩大回归 `nomifun-db + nomifun-conversation` all-target **exit 0，1666 passed / 0 failed / 0 ignored / 0 filtered**；不再列为阻断。剩余阻断/未签核：
> 1. **真实语音 DOM 交互**：已用合法 `nomifun-web + Vite webdev + bsk/Chrome` 尝试，SPA 因 Web 云端登录门禁（`needs_setup: true` / `is_authenticated: false`）停在登录页，未取得 More 点击/键盘/Escape、窄屏布局、双主题或输入框胶囊的真实 DOM 回执；未绕过鉴权。截图 `t4-live-web/login-blocker.png`。
> 2. **原生桌面/桌宠**：透明首帧、100/125/150% DPI、负坐标副屏、临时显示/结束恢复隐藏、owner/形象/位置时序、真实 show/hide 未启动验证；静态 38 项只证明纯逻辑/结构。
> 3. **物理声学/AEC/麦克风**：真实采集、回声消除、自然插话仍需人工设备验证；本轮未接入 AEC。
> 4. **既有仓级门禁**：全仓 TypeScript 仍 61 条既有错误（语音相关 0，与基线一致）；dead-css 仍为既有失败。二者均未改基线掩盖。
>
> 另有架构边界提请独立审查：`boot.rs` 现以 `finalize_orphaned_turn` 做启动级结算，仍早于 `router/state.rs` 的终端证明扫描；这延续 boot 入口原设计，但 external Agent 的证明协议若应成为唯一权威，需另案收窄 boot 范围。证据见 [2026-10-25 接续收口记录](REPAIR-VERIFICATION-2026-10-24.zh.md#continuation-2026-10-25)。

> **2026-10-24 接续状态覆盖**：原 3 项技能加载卡住已查明并修复，完整 Conversation lib 648 无过滤通过，不再将它们列为未归因阻断。剩余：P2 未取得真实 DOM 交互回执；P3 无已启用的原生应用控制入口，透明首帧/DPI/物理声学保持未签核；扩大回归的普通会话夹具失败、TS 61 条和 dead-css 仍分别记录。当前修复未提交、未推送。唯一事实入口：[接续执行记录](REPAIR-VERIFICATION-2026-10-24.zh.md#next-validation-execution)。

> **2026-10-24 当前边界更正**：不止物理环境未签核。浏览器交互探针未成功运行；完整 Conversation 的 3 项技能加载测试卡住，独立复跑一项也超时，尚未完成归因；全仓 TS 与 dead-css 未通过。已修复项与定向 green 结果见 [修复验证记录 §4–6](REPAIR-VERIFICATION-2026-10-24.zh.md)。下文“无阻断/全部自动化通过/160 项 DOM 测试”仅保留为历史报告，不是当前结论。

> **记录时间**: 2026-10-24
> **代码实施状态**: 24 项验收代码全部完成，前后端全量自动化测试与质量门禁均已通过。

## 阻断项

无（业务代码实施与自动化验证无阻断）。

> **2026-10-25 更正**：上面“无阻断”是历史报告结论。当前实际阻断/未签核清单见本文件顶部“2026-10-25 接续收口更新”；自动化代码项已关闭，但真实 DOM、原生/DPI、桌宠姿态与物理声学仍未签核。

## 物理运行环境未签核边界

依照方案 §5.2 第 5 条与验收标准规定，无法通过无头命令行自动化模拟的物理效果明确不以静态测试假冒签核，列为未签核边界：

1. **真实声学硬件/AEC/麦克风采集**:
   - 自动化 CLI 测试已覆盖 `VoiceDialogueRecorder`、IPC 传输协议、Acoustic Scope、409 仲裁与流式超时丢弃。
   - 真实物理麦克风降噪、扬声器回声消除（AEC）效果需人工在具备音频硬件的环境中进行真实声音测试。

2. **Tauri 原生桌面窗口透明首帧与高 DPI 缩放**:
   - Web 端与组件级 CSS 布局已通过 `check:theme`、`check:button-layout-contract`、160 项前端 DOM 测试验证。
   - Tauri 在 Windows 平台的 DwmExtendFrameIntoClientArea 透明通道、多显示器不同 DPI（125%、150%）下的窗口移动渲染属于桌面宿主层物理效果，需人工启动桌面端桌面应用实测验证。

3. **桌宠实际运行姿态与屏幕跟随动画**:
   - 依赖的业务逻辑与 badge 投影状态已在组件层与状态机中核验。
   - 桌宠跨屏移动与物理画面渲染需随桌面版真机运行核对。

除上述明确物理环境限制外，所有业务契约、生命周期屏障、事件隔离与布局代码均已闭环。

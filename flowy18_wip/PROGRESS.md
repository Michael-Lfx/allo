# flowy18_wip 三挡位模型选择器 — 进度

## 理解（≤10 行）
- 目标：把平铺 12 云模型改成三挡位 Flowy 2.0 Max / Flowy 2.0 / Flowy 2.0 Lite；按账号 effectiveModelTier 锁定 Pro 挡位；Flex 留档不可达；Lite 可发消息。
- 顺序：0 核对基线 → 1 修编译（TierView 导出、settings 三臂）→ 2 i18n → 3 测试+补测 → 4 UI 行为核对 → 5 门禁 fmt/clippy/test → 6 真实桌面实跑 + 恢复数据。
- 最大风险：授权判断写错导致 Free 账号能切 Pro（越权）；其次实跑环境（GPUI Enter 注入）与数据备份恢复失败。
- 让步顺序：正确 > 完整 > 好看/文案。
- 已知环境坑：Windows 下 TEMP/CARGO_TARGET_DIR 必须用 cygpath -w 转 Windows 路径，否则 cargo 拼出 D:/d/... 失败。
- 范围外既有改动：flowy18_wip/crates/flowy-ui/src/overlays.rs 已在工作区，不在白名单，只读不动，记 BLOCKED.md。

## 状态
- [x] 任务0：基线 check 复现 3 个错误（TierView 未导出 ×2、Plan::Paid 不存在）
- [x] 任务1：修编译 — studio/lib.rs 导出 TierView；settings.rs 改用 Plan::label_key（三态）；agent model.rs 新增 Plan::label_key；dock.rs 去掉写死 plan_free；check 0 error
- [x] 任务2：i18n 加 credits.plan_pro/plan_ultra、model.unavailable（仅 zh-CN/en，白名单限制）；settings/studio/dock plan 文案统一走 Plan::label_key
- [x] 任务3：cargo test 四 crate 全绿（agent 28+1 既有 manual ignore / app 23 / auth 12 / studio 11）；tier 解析锁定、prefs paid alias+Flex 回落测试均已存在且通过
- [x] 任务5（部分）：cargo fmt --all --check 绿；CI 同参 clippy 绿（修了 agent model_directory 生命周期、const、TierView Eq 等既有 lint）
- [x] 任务4：UI 行为核对（暗/亮双主题）：三行 label+detail+Pro 徽标；Free 账号 Max/Standard a11y disabled 且点击只提示不切换（chip 始终 Flowy 2.0 Lite）；Lite 可选中并实发消息收到回复；目录未加载时空菜单不 panic
- [x] 任务5：fmt 绿 / CI 同参 clippy 绿 / 四 crate tests 全绿（28+1 既有 manual ignore、23、12、11）
- [x] 任务6：暗/亮双主题实跑完成，数据已恢复并与备份逐字节一致，无 flowy-app 进程
- [x] 反向验证：is_locked 临时改恒 false → 单测红（is_locked(Max,0) 断言失败）；实跑 Free 账号点 Max 后 chip 变「Flowy 2.0 Max」、prefs.json model=max（越权证据）→ 还原源码（sha 与备份一致）→ 单测/门禁全绿
- [x] 收尾：sessions/{index,history}.json、auth/session.json、preferences.json 与备份 cmp 全部 IDENTICAL；分支 feat/flowy18-wip-credits 无 commit/staged
- [x] 追加（用户 2026-11-18）：i18n 补全其余 21 语种（3 key × 23 locales parity 541 keys）；`cargo test -p flowy-i18n` 4 passed；BLOCKED 第 2 条销项
- [x] 追加：重建并启动实测（应用已启动并停在模型菜单，三挡位可见，供用户接手测试）
- [x] 追加：审查当前代码修改（结论：无阻塞规格缺口；1 个失败路径 Required：不可用挡位分支会改选择不改路由；若干 Optional/FYI 见对话）
- [x] 追加（用户要求「全部修复」）：S1 不可用挡位不再改选择/不再假装应用；删除死状态 route_generation/applied_model_id；TierView 改为渲染时按 key 取文案（切语言即时生效）；账号切换清空 tiers；新增 `model.menu_title`（23 语种）替代菜单标题；移除无引用 `credits.plan_paid`（23 语种）；新增 app 级单测 `unavailable_or_locked_tiers_never_change_the_selected_route`
- [x] 修复后门禁：fmt 绿；clippy 绿；tests 28+1/24/12/11 全绿；i18n 4 passed（541 keys × 23 locales）

## 实跑证据（2026-11-18，Free 账号 effectiveModelTier=0，套餐显示「免费」）
- 暗色：三行菜单 Max[Pro]/Flowy 2.0[Pro]/Lite，Max/Standard 置灰且 a11y disabled=true；点 Max/Standard 只提示「升级尚未开通」，chip 保持「Flowy 2.0 Lite」；选 Lite 发送建议词，收到完整回复，积分 179,580→179,570。
- 亮色：外观切「浅色」后，菜单三行/Pro 徽标/置灰正常；点 Standard 仍只提示不切换。
- 反向：锁定失效时同上操作直接切到 Max（chip/落盘均变），证明锁定判断是唯一闸门。

## 已合入 main 的 Clippy 修复（#461）

- [x] 审查并修复共享控件、Library 和技能页的 9 条 Clippy 报错，保留运行逻辑和既有告警策略。
- [x] Windows 特性分支反向验证：移除 `TypeMetrics: Eq` 时仅出现 `derive_partial_eq_without_eq`，退出 101；恢复后退出 0。
- [x] PR 分支以 `9f51499af` 为基线，Windows Rust 1.100 下原 Clippy、fmt、四 crate 测试均退出 0；agent 24 / app 22 / auth 11 / studio 18，0 failed。
- [x] 特性分支原命令为 agent 28 / app 24 / auth 12 / studio 20，0 failed；另行重跑 1 个既有 ignored 离线性能用例并通过。
- [x] Library 示例使用现有锁文件离线构建。Windows Library Demo 已构建。原生截图与 UIA 交互未取得完整通过证据；Studio 既有 GPUI 排序菜单测试通过。

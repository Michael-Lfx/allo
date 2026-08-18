# nomi/main 独有提交 → mike/main 吸收判断

> 生成时间：2026-08-24（本地 `git fetch --all` 后）
> 对比基准：`remotes/nomi/main` vs `remotes/mike/main`
> merge-base：`fa493e1f2`（2026-08-02 `fix(file-picker): route directory requests through csrf bridge`）

## 0. 范围说明（必读）

完整 `mike/main..nomi/main` 非 merge 提交约 **2077** 条。其中大量来自 nomi 侧 merge 进来的第二父历史（含 2026-06 `Initial clean import` 孤儿根），**不能**当作「尚未合入 mike 的增量」整包吸收。

本文件以 **nomi/main first-parent 主线**（`git log --first-parent mike/main..nomi/main`）共 **203** 条作为吸收候选清单——这才是 nomi 主干实际落盘顺序。

判断原则：

1. **bugfix / 安全修复** 默认倾向吸收（仍需 cherry-pick 前 diff）。
2. **new feat** 仅在与 mike 产品线不重叠时吸收。
3. **remote mike 的 feat 优先**：尤其 vimax / video-canvas / Flowy Cloud / learning / session-observation 等；nomi 的 creative-studio/canvas 整包 redesign **不整包吸收**。
4. 已通过 `mike/feat/absorb-u-main`、`absorb/truncation-recovery` 吸收过的能力标 **skip**。
5. 发行元数据、nomifun 品牌文档、纯 style、基于 nomi 拓扑的死代码删除 → **skip**。

### Verdict 含义

| verdict | 含义 |
| --- | --- |
| `absorb` | 值得吸收：优先进入后续 cherry-pick 队列 |
| `careful` | 有价值但分叉大/高风险：需人工看 diff 或只移植片段 |
| `defer-mike-first` | 与 mike feat 重叠：默认不吸，除非抽出与产品无关的纯 bugfix |
| `follow` | 随主功能走，不单独吸收 |
| `skip` | 不值得吸收到 mike/main |

### 汇总（first-parent）

| verdict | count |
| --- | ---: |
| `absorb` | 8 |
| `careful` | 27 |
| `defer-mike-first` | 23 |
| `follow` | 11 |
| `skip` | 134 |
| **total** | **203** |

## 1. 建议优先吸收队列（absorb）

| hash | date | subject | 理由 |
| --- | --- | --- | --- |
| `da76ef6b7` | 2026-08-24 | fix(markdown): isolate code highlighting failures | 通用 markdown 高亮隔离，mike 未见等价提交 |
| `9cb0e2568` | 2026-08-24 | fix(ui): clarify optional model alias field | 可选 alias 字段文案澄清，小改动 |
| `180cabe08` | 2026-08-19 | fix(security): reject Windows-specific path constructs before joining | 安全修复，与产品线无关，优先吸收 |
| `2d1463fa5` | 2026-08-07 | fix(knowledge): 修掉新建入口的焦点不可见，并让对比度测试真的在测东西 | Bugfix 默认倾向吸收；落地前仍需 diff 冲突检查 |
| `12fd8b9fe` | 2026-08-05 | fix: paint the separators and focus rings, and close the dead-utility class for good | 主题分隔线/焦点环未渲染类 bugfix，产品无关 |
| `cd403baa2` | 2026-08-05 | fix: paint the borders and backgrounds that never rendered, and make Alerts legible | 边框/背景未渲染与 Alert 可读性，产品无关 |
| `16381ffd0` | 2026-08-05 | fix: make every written colour actually render, and freeze the reset plan shape | CSS 变量颜色未生效修复，产品无关 |
| `c80555cef` | 2026-08-05 | fix: repair the factory-reset registry regression and clear the red suites | factory-reset registry 回归修复，值得吸 |

## 2. 需人工看 diff（careful）

| hash | date | subject | bucket | 理由 |
| --- | --- | --- | --- | --- |
| `67da81075` | 2026-08-23 | 移除无用入口 | zh-misc | 中文简述提交，需看 diff 再定；可能与 canvas/模型相关 |
| `b0c01e27f` | 2026-08-22 | refactor(ui): streamline model call configuration | refactor | 重构需证明对 mike 有净收益且不破坏 mike feat |
| `6e34a6379` | 2026-08-22 | 优化模型配置 | models-ux | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| `c5e37e454` | 2026-08-22 | fix(db): preserve published migration 36 lineage | migration | 迁移血缘敏感——高风险区，需人工对照 mike migrations，勿盲吸 |
| `716a3b6d7` | 2026-08-22 | refactor(settings): nest advanced config in the order it actually resolves | refactor | 重构需证明对 mike 有净收益且不破坏 mike feat |
| `bb18f1579` | 2026-08-22 | feat(settings): declare supported tasks in a multi-select | models-ux | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| `57c23eab7` | 2026-08-22 | fix(settings): stop labelling provider creation as "添加模型" | models-ux | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| `8139a9390` | 2026-08-22 | fix(settings): say which capability config is missing | models-ux | 设置页能力缺失提示有用，但挂在 nomi 模型管理 redesign 上，宜抽文案/逻辑移植 |
| `d33a86fe1` | 2026-08-22 | style(settings): make the declared-task confirmation impossible to miss | models-ux | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| `48b41eba1` | 2026-08-22 | fix(settings): make the supported-task picker show what it accepted | models-ux | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| `b12228b43` | 2026-08-20 | feat(model-management): autofill custom protocols | models-ux | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| `4c47bdde7` | 2026-08-19 | fix(models): define the /v1 boundary and stop forking inherited config | models | 模型配置边界修复有价值，但 mike 模型层已分叉，需定向移植 |
| `939320710` | 2026-08-17 | fix(i18n): translate the two strings 77298e98 left untranslated | i18n | 补两处漏翻；依赖 FreeModelsContent 路径，确认 mike 仍有对应 UI 再吸 |
| `e0d340734` | 2026-08-16 | refactor(agent): drop unread provider and binary-path factory deps | refactor | 重构需证明对 mike 有净收益且不破坏 mike feat |
| `88d0f5509` | 2026-08-15 | fix(runtime): recover from failover teardown failures | runtime-large | failover 恢复有价值但 diff 极大（MCP/providers/conversation），需拆分移植 |
| `6e0ff191f` | 2026-08-14 | 优化执行过程，变更 nfagent 加载机制 | zh-misc | 中文简述提交，需看 diff 再定；可能与 canvas/模型相关 |
| `37fb2407d` | 2026-08-14 | fix(conversation): harden agent runtime stability | runtime-large | agent runtime 硬化有价值但触及 MCP/tool_execution 面广，需对照 mike 后拆分 |
| `4a2eacfd4` | 2026-08-13 | feat(channels): add configurable group access | feat-channels | 频道群组访问是独立 feat；体积大且碰 channel/api-types，人工评估后吸收 |
| `4a65b5eba` | 2026-08-12 | 优化模型类型与目录选择交互 | models-ux | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| `ffdb4f4f3` | 2026-08-12 | 模型管理改造 | models-ux | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| `82c94411f` | 2026-08-10 | feat(ui): unify update notification presentation | updater-ux | 更新提示 UI；对照 mike updater 状态后再定 |
| `96e0b5ab4` | 2026-08-08 | fix(settings): preserve provider edit save payload | models-ux | provider 编辑保存 payload 修复有价值；wire 层与 mike 可能已分叉 |
| `34de3b229` | 2026-08-08 | fix: make SkillHub expert package installs reliable | skills | Skill 安装可靠性；对照 mike skill 市场现状 |
| `2bf4eee03` | 2026-08-08 | feat: unify enhanced tools market controls | unclear | 需看 diff 才能定 |
| `54e01ad45` | 2026-08-06 | docs(modelhub): narrow the providers section to access and credentials | models-ux | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| `7c04a18e2` | 2026-08-06 | feat(models): add the shared TaskModelSelect and rebuild the companion chat model control on it | feat | 新功能；与 mike 重叠则 mike 优先，独立域可吸收 |
| `c71f30731` | 2026-08-06 | refactor(ui): delete the review surface and reshape the disposition control | refactor | 重构需证明对 mike 有净收益且不破坏 mike feat |

## 3. mike feat 优先 / 推迟（defer-mike-first）

| hash | date | subject | 理由 |
| --- | --- | --- | --- |
| `2db3f41c7` | 2026-08-24 | feat(canvas): refine floating canvas controls | canvas 浮层控件，与 mike video-canvas 重叠 |
| `fb4ecc0bb` | 2026-08-24 | feat(creative-studio): refine canvas zoom and minimap controls | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `35cdc43fb` | 2026-08-24 | fix(creative-studio): drop unreleased workflow projects | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `c7a9d4201` | 2026-08-24 | perf(creative-studio): load asset pages on demand | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `527b5218c` | 2026-08-24 | perf(creative-studio): cull offscreen canvas layers | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `6bc60f8ea` | 2026-08-24 | test(creative-studio): harden agent context projection coverage | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `690434df5` | 2026-08-24 | perf(creative-studio): build agent context only when visible | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `472b37431` | 2026-08-24 | perf(creative-studio): cache and deduplicate asset queries | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `15f4ab1d7` | 2026-08-24 | perf(creative-studio): preload studio routes and show navigation progress | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `eaafad34c` | 2026-08-24 | perf(creative-studio): keep canvas viewport updates local | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `72ef904bf` | 2026-08-24 | perf(creative-studio): coalesce canvas interaction persistence | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `4bf342e76` | 2026-08-24 | fix(creative-studio): keep agent panel flush right | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `abeeac1a1` | 2026-08-24 | refactor(creative-studio): replace workflow domain with templates | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `bc76c303c` | 2026-08-23 | fix(i18n): complete Creative Studio localization | Creative Studio 大规模 i18n，跟 mike vimax/video-canvas 重叠 |
| `5ab4812f8` | 2026-08-23 | fix(creative-studio): honor provider image size contracts | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `4b3a067f9` | 2026-08-23 | 素材模板功能上线 | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `038100f72` | 2026-08-23 | fix(creative-studio): unify my assets navigation label | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `cceed3874` | 2026-08-23 | 优化提示词交互逻辑 | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `7f6cf37c3` | 2026-08-23 | 画布agent功能完善 | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `80c119a93` | 2026-08-23 | feat(creative-studio): complete Canvas domain redesign | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `b5aa7cc37` | 2026-08-23 | fix(creative-studio): resume work and checkpoint canvas redesign | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `4d5ff3306` | 2026-08-22 | fix(creative-studio): unify navigation with app shell | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| `a1ff20e78` | 2026-08-22 | chore(creative-studio): retire stale canvas audit assets | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |

## 4. 随功能走（follow）

| hash | date | subject | 理由 |
| --- | --- | --- | --- |
| `43a266d0b` | 2026-08-24 | test: align manifest and canvas contracts | canvas/manifest 契约测试，随 canvas 决策走 |
| `78ee757df` | 2026-08-24 | test(cron): align conversation repository fixtures | 测试/CI 修复随对应功能吸收；单独价值低 |
| `698ae0598` | 2026-08-24 | test(agent): stabilize tracing log capture | 测试/CI 修复随对应功能吸收；单独价值低 |
| `8401455c0` | 2026-08-24 | test(browser): stabilize recovery log capture | 测试/CI 修复随对应功能吸收；单独价值低 |
| `b600c680a` | 2026-08-24 | fix(tests): align managed asset fixtures | 测试/CI 修复随对应功能吸收；单独价值低 |
| `d73f0203e` | 2026-08-24 | test(models): cover responsive advanced editor heights | 测试/CI 修复随对应功能吸收；单独价值低 |
| `dea748bc2` | 2026-08-24 | test(models): align call configuration tab intents | 测试/CI 修复随对应功能吸收；单独价值低 |
| `b27873cc1` | 2026-08-24 | test(knowledge): align retrieval modal with compact chrome | 测试/CI 修复随对应功能吸收；单独价值低 |
| `aa3071217` | 2026-08-19 | fix(test): remove two wall-clock races and one shared-env race | 测试/CI 修复随对应功能吸收；单独价值低 |
| `b6e61cbcd` | 2026-08-19 | fix(test): repair four suites masked behind the first failing binary | 测试/CI 修复随对应功能吸收；单独价值低 |
| `6aabb255f` | 2026-08-19 | fix(ci): unblock the check gate and repair the stale test suites | 测试/CI 修复随对应功能吸收；单独价值低 |

## 5. 不吸收（skip）

| hash | date | subject | bucket | 理由 |
| --- | --- | --- | --- | --- |
| `6672e2bc8` | 2026-08-24 | chore(release): v0.7.1 | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `de3fb8ab6` | 2026-08-24 | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | merge | 同步合并点，无独立变更可吸收 |
| `ca28973e9` | 2026-08-24 | merge: sync remote main release metadata | merge | 同步合并点，无独立变更可吸收 |
| `0feafe4c1` | 2026-08-24 | refactor(ui): remove unused compatibility paths | deadcode-nomi | 基于 nomi 拓扑的死代码清理，直接 cherry-pick 易误删 mike 仍在用路径 |
| `b0c8fc1ea` | 2026-08-24 | refactor(browser): retire unused mcp bridge | deadcode-nomi | 基于 nomi 拓扑的死代码清理，直接 cherry-pick 易误删 mike 仍在用路径 |
| `9055e6ceb` | 2026-08-24 | refactor(rust): remove verified dead paths | deadcode-nomi | 基于 nomi 拓扑的死代码清理，直接 cherry-pick 易误删 mike 仍在用路径 |
| `6b04da861` | 2026-08-24 | chore(release): v0.7.0 | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `cbe188bb8` | 2026-08-24 | Merge remote-tracking branch 'origin/main' | merge | 同步合并点，无独立变更可吸收 |
| `f5d36465c` | 2026-08-23 | Merge remote-tracking branch 'origin/main' | merge | 同步合并点，无独立变更可吸收 |
| `c8445a85d` | 2026-08-23 | fix(ui): move language control to titlebar leading edge | reverted | nomi 后续已 Revert（7441ba289），勿吸收 |
| `7d4d39264` | 2026-08-23 | Merge remote-tracking branch 'origin/main' | merge | 同步合并点，无独立变更可吸收 |
| `4928cb83e` | 2026-08-23 | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | merge | 同步合并点，无独立变更可吸收 |
| `0824f455a` | 2026-08-23 | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | merge | 同步合并点，无独立变更可吸收 |
| `bb5cef5e5` | 2026-08-23 | Merge origin/main: preserve Canvas domain redesign | merge | 同步合并点，无独立变更可吸收 |
| `e573ed08e` | 2026-08-22 | Merge remote-tracking branch 'origin/main' | merge | 同步合并点，无独立变更可吸收 |
| `b7ea1068a` | 2026-08-22 | style: refine creative studio layouts | style-only | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| `e7b6850dd` | 2026-08-22 | docs(md) update wechat-group | branding-docs | nomifun 品牌/社区/生态文档，mike 不需要 |
| `2b77045cd` | 2026-08-22 | Merge branch 'codex/infinite-canvas-rebuild' into main | merge | 同步合并点，无独立变更可吸收 |
| `3af42d040` | 2026-08-22 | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri . | merge | 同步合并点，无独立变更可吸收 |
| `db76510c1` | 2026-08-22 | Merge branch 'refactor/model-management-simplification' | merge | 同步合并点，无独立变更可吸收 |
| `33cd05064` | 2026-08-21 | docs(handoff): record merge verification and coverage accounting | docs | 文档/handoff/计划类，默认不吸收 |
| `6668e0124` | 2026-08-21 | Merge branch 'fix/truncation-finalize' | merge | 同步合并点，无独立变更可吸收 |
| `985651640` | 2026-08-21 | Merge branch 'fix/truncation-not-success' | merge | 同步合并点，无独立变更可吸收 |
| `682bfb4d2` | 2026-08-20 | style(ui): unify model and modal styling | style-only | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| `c603bd7fd` | 2026-08-19 | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | merge | 同步合并点，无独立变更可吸收 |
| `b8104b2ab` | 2026-08-19 | Merge remote-tracking branch 'origin/main' | merge | 同步合并点，无独立变更可吸收 |
| `58b595682` | 2026-08-19 | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | merge | 同步合并点，无独立变更可吸收 |
| `1c5f214c2` | 2026-08-19 | style(ui): unify modal visual contract | style-only | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| `da36a5e88` | 2026-08-18 | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | merge | 同步合并点，无独立变更可吸收 |
| `d4953a776` | 2026-08-18 | style(ui): refine knowledge library layouts | style-only | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| `59a7c844f` | 2026-08-18 | chore(release): v0.6.4 | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `c3c58f982` | 2026-08-17 | Merge origin/main into refactor/collapse-engines-to-nomi | merge | 同步合并点，无独立变更可吸收 |
| `af181d6e9` | 2026-08-17 | refactor(agent): collapse the engine set to the native nomi executor | engine-divergence | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| `9c82dfb90` | 2026-08-16 | refactor(ui): drop the openclaw conversation surface | engine-divergence | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| `f2f99a76f` | 2026-08-16 | refactor(agent): remove the openclaw-gateway engine and retire Star Office | engine-divergence | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| `344f808b0` | 2026-08-16 | refactor(agent): remove the remote-agent engine | engine-divergence | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| `b73e35f34` | 2026-08-16 | refactor(agent): remove the nanobot engine | engine-divergence | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| `c3c4580af` | 2026-08-16 | refactor(conversation): delete the ACP tool-call artifact machine | engine-divergence | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| `c87b8ee1c` | 2026-08-16 | refactor(agent): remove the ACP-only idle runtime scanner | engine-divergence | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| `02afa0652` | 2026-08-16 | refactor(conversation): remove the dead usage and openclaw-runtime routes | engine-divergence | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| `3177f52db` | 2026-08-15 | chore(release): add Linux updater entry to v0.6.3 latest.json | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `5c9b95710` | 2026-08-15 | docs(readme): add Gitee repository links | branding-docs | nomifun 品牌/社区/生态文档，mike 不需要 |
| `6458e57a8` | 2026-08-15 | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | merge | 同步合并点，无独立变更可吸收 |
| `092ad799a` | 2026-08-15 | Merge remote-tracking branch 'origin/main' | merge | 同步合并点，无独立变更可吸收 |
| `2d7e4b096` | 2026-08-15 | docs: add Net Infra to the product ecosystem | branding-docs | nomifun 品牌/社区/生态文档，mike 不需要 |
| `2a5286997` | 2026-08-15 | Merge branch 'codex/fix-glob-tool-hang' | merge | 同步合并点，无独立变更可吸收 |
| `f7cbae60e` | 2026-08-14 | merge: integrate latest origin/main | merge | 同步合并点，无独立变更可吸收 |
| `fae5658c2` | 2026-08-13 | fix(dev): invalidate relocated Cargo build cache | dev-tooling | 仅 scripts/prune-build.mjs 开发缓存清理，非产品 bugfix |
| `e701e5782` | 2026-08-13 | docs: refresh ecosystem and Docker guidance | branding-docs | nomifun 品牌/社区/生态文档，mike 不需要 |
| `120573262` | 2026-08-13 | docs: document the NomiFun product family | branding-docs | nomifun 品牌/社区/生态文档，mike 不需要 |
| `3d7f5a9b6` | 2026-08-12 | chore(release): add Linux assets for v0.6.1 | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `b3780e953` | 2026-08-12 | chore(release): add macOS assets to v0.6.1 latest.json | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `76f49d4fa` | 2026-08-12 | chore(release): v0.6.1 | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `e8fa02081` | 2026-08-12 | Merge remote-tracking branch 'origin/main' | merge | 同步合并点，无独立变更可吸收 |
| `11116a5be` | 2026-08-11 | chore(release): v0.6.0 | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `9c50985bf` | 2026-08-11 | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | merge | 同步合并点，无独立变更可吸收 |
| `97ade6be9` | 2026-08-11 | fix(enhanced-tools): prevent duplicate market additions | already-on-mike | mike 已有等价提交 765a653d8 fix(enhanced-tools): prevent duplicate market additions |
| `f5a5314af` | 2026-08-10 | fix(conversation): refine process and collaboration UI | already-on-mike | mike 已有等价提交 72f1dfd3d fix(conversation): refine process and collaboration UI |
| `d3a68d528` | 2026-08-10 | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | merge | 同步合并点，无独立变更可吸收 |
| `755cfb310` | 2026-08-10 | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | merge | 同步合并点，无独立变更可吸收 |
| `7361efd56` | 2026-08-10 | style(ui): compact tool and creation dialogs | style-only | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| `d6abd755d` | 2026-08-10 | style(markdown): unify document preview typography | style-only | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| `fffcb8901` | 2026-08-09 | 补充小智ai 接入文档内容， 项目仓库 nomifun-xiaozhi-yuntai | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `48fa2bbe2` | 2026-08-09 | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | merge | 同步合并点，无独立变更可吸收 |
| `0578d0107` | 2026-08-09 | 支持小智esp32 机器人连接，伙伴即机器人 | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `51427574b` | 2026-08-08 | chore(release): add Linux updater entry to v0.5.0 latest.json | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `48a9a0fa3` | 2026-08-08 | feat(robot): 补齐 StepFun 语音链路（目录+失败可见+侧栏分组+健康检查） | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `2ae77e220` | 2026-08-08 | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | merge | 同步合并点，无独立变更可吸收 |
| `ec9e31d62` | 2026-08-08 | 更新微信群二维码 | branding-docs | nomifun 品牌/社区/生态文档，mike 不需要 |
| `623b2fa6b` | 2026-08-08 | chore(release): v0.5.0 | release | nomi 发行元数据/版本号，与 mike 发布线无关 |
| `959d583f0` | 2026-08-07 | style: compact update modal layout | style-only | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| `cb6ca9aba` | 2026-08-07 | Merge branch 'feat/model-hub-restructure' into main | merge | 同步合并点，无独立变更可吸收 |
| `c10b1f2ec` | 2026-08-07 | Merge branch 'fix/updater-state-ownership' into main | merge | 同步合并点，无独立变更可吸收 |
| `2f53522f3` | 2026-08-06 | Merge remote-tracking branch 'origin/main' | merge | 同步合并点，无独立变更可吸收 |
| `628cd0130` | 2026-08-06 | fix(robot): make test-support implicit for the crate's own tests | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `73f4124f8` | 2026-08-06 | docs(deploy): explain how LAN robots reach a server deployment | docs | 文档/handoff/计划类，默认不吸收 |
| `6f192f464` | 2026-08-06 | feat(robot): advertise a reachable endpoint from headless hosts | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `894f4f131` | 2026-08-06 | docs(contributing): record the robot gateway's two build prerequisites | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `336838b3d` | 2026-08-06 | feat(robot): honour the companion profile's chosen VAD engine | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `adbcedff7` | 2026-08-06 | test(robot): add fake-device end-to-end integration test | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `6355a8dbd` | 2026-08-06 | feat(robot): add management REST face and host assembly | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `ca1bfe605` | 2026-08-06 | feat(robot): wire ASR, TTS and one-shot vision to the model layer | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `83beb385b` | 2026-08-06 | feat(robot): add vision explain endpoint for device photo understanding | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `492b37509` | 2026-08-06 | fix(conversation): keep robot threads out of the ordinary session list | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `97d754728` | 2026-08-06 | feat(robot): add tool registry and loopback MCP proxy for robot tools | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `278b4ad28` | 2026-08-06 | feat(robot): add the robot connection section to the companion remote tab | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `16d180923` | 2026-08-06 | feat(robot): add device MCP client with paging and tolerant error handling | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `90c24e742` | 2026-08-06 | feat(robot): project robot.status into a live per-device map | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `de7a60fc8` | 2026-08-06 | feat(robot): wire uplink, dispatch and downlink into the session loop | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `16fc6c1ee` | 2026-08-06 | feat(robot): add the robot management wire contract to the bridge | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `c23a15b0a` | 2026-08-06 | feat(modelhub): build the chat, vision and embedding modality sections | already-on-mike | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| `68507e92c` | 2026-08-06 | feat(modelhub): project provider_models rows into modality groups | already-on-mike | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| `17fb330fd` | 2026-08-06 | feat(modelhub): give the voice section a TTS default, a catalog-only ASR and the local VAD entry | already-on-mike | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| `91f86a37b` | 2026-08-06 | feat(robot): add downlink pacer with generation-based flush | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `468066133` | 2026-08-06 | feat(modelhub): make the hub a modality-first view with eight sections | already-on-mike | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| `17b943638` | 2026-08-06 | feat(robot): add uplink pipeline with VAD endpointing | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `a43b86412` | 2026-08-06 | feat(robot): add speech and dispatcher trait seams with mocks | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `d7019bad0` | 2026-08-06 | feat(robot): add incremental sentence splitter and emotion markers | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `aecd736ba` | 2026-08-06 | feat(robot): add silero ONNX VAD with energy fallback | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `61e52d200` | 2026-08-06 | feat(companion): rebuild the overview model section as five kinds of slot | already-on-mike | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| `44d5bb76f` | 2026-08-06 | feat(models): extract the shared task-model selector decision logic | already-on-mike | TaskModelSelect 共享选择器，mike 已有 task-filtered model selectors |
| `df0df6716` | 2026-08-06 | feat(robot): add VAD abstraction with energy engine | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `ae0a0e9a5` | 2026-08-06 | feat(companion): mirror the new model slots on the profile wire type | already-on-mike | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| `1d17e5d22` | 2026-08-06 | feat(robot): add opus codec, wav packing, resampling and container decode | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `35fdc1773` | 2026-08-06 | feat(tts): add the tools.textToSpeech install-wide preference | already-on-mike | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| `d64234a37` | 2026-08-06 | feat(robot): add websocket endpoint, LAN link source and session actor | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `25b896e66` | 2026-08-06 | feat(robot): add status registry and robot.status realtime event | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `a44b962d9` | 2026-08-06 | feat(robot): add OTA report and activation endpoints | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `9c7be042c` | 2026-08-06 | feat(robot): add endpoint advertiser with LAN implementation | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `6635c88b4` | 2026-08-06 | feat(robot): add v1 binary framing and transport-agnostic link traits | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `f7aba0dbc` | 2026-08-06 | feat(robot): add xiaozhi JSON message vocabulary | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `25afd5008` | 2026-08-06 | feat(companion): add fallback, vision and voice model slots to the profile | already-on-mike | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| `09fa2960e` | 2026-08-06 | feat(robot): add robot registry with token rotation and activation codes | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `d80c50a26` | 2026-08-06 | feat(robot): scaffold nomifun-robot crate | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `9a4d06e10` | 2026-08-06 | docs(plans): add robot bridge implementation plans A/B/C | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `abb75045c` | 2026-08-06 | docs(specs): add robot bridge design, supersede xiaozhi integration spec | already-on-mike | mike 已 absorb robot/StepFun/xiaozhi |
| `a9718894e` | 2026-08-06 | Merge origin/main into the write-back simplification | merge | 同步合并点，无独立变更可吸收 |
| `ab51a65b9` | 2026-08-06 | Merge remote-tracking branch 'origin/main' into refactor/knowledge-writeback-simplification | merge | 同步合并点，无独立变更可吸收 |
| `e5710c10b` | 2026-08-06 | fix: absorb a verbatim restatement instead of doubling the document | already-on-mike | knowledge writeback 简化链路，mike 已 absorb-u-main |
| `521eab10f` | 2026-08-06 | refactor: make the manual disposition real and finish retiring placement | already-on-mike | writeback disposition 重构，mike 已吸收 |
| `a233f5fd4` | 2026-08-06 | refactor(knowledge): make write-back direct-only, manual/auto, and non-destructive | already-on-mike | writeback direct-only，mike 已吸收 |
| `339145f1e` | 2026-08-06 | refactor(db): drop writeback_mode and move the disposition to manual/auto | already-on-mike | drop writeback_mode 迁移，mike 已吸收（见 bb63ab833） |
| `7dac8fa58` | 2026-08-05 | docs: lay out the write-back simplification task by task | already-on-mike | mike 已 absorb-u-main（knowledge writeback） |
| `80b528027` | 2026-08-05 | docs: pin the code-level approach for the write-back simplification | already-on-mike | mike 已 absorb-u-main（knowledge writeback） |
| `8632763db` | 2026-08-05 | docs: design the knowledge write-back simplification | already-on-mike | mike 已 absorb-u-main（knowledge writeback） |
| `9599ddc13` | 2026-08-05 | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | merge | 同步合并点，无独立变更可吸收 |
| `98844cfd9` | 2026-08-05 | docs: state the outline-2 failure as measured, not as inferred | docs | 文档/handoff/计划类，默认不吸收 |
| `49431c307` | 2026-08-05 | Merge branch 'feat/companion-workspace-redesign' | merge | 同步合并点，无独立变更可吸收 |
| `2b728bfc9` | 2026-08-05 | Merge branch 'feature/ssh-remote-session' | merge | 同步合并点，无独立变更可吸收 |
| `0ba785c42` | 2026-08-05 | Merge remote-tracking branch 'origin/main' | merge | 同步合并点，无独立变更可吸收 |
| `67e64b5d9` | 2026-08-03 | docs: fix 14 audit defects in bridge plans and protocol | docs | 文档/handoff/计划类，默认不吸收 |
| `3d3fad5c9` | 2026-08-03 | docs: add implementation plans for bridge, relay server and mobile app | docs | 文档/handoff/计划类，默认不吸收 |
| `6a25aaae6` | 2026-08-03 | docs: add bridge protocol v1 shared reference | docs | 文档/handoff/计划类，默认不吸收 |
| `d617c9474` | 2026-08-03 | docs: add nomifun mobile remote-control bridge design spec | docs | 文档/handoff/计划类，默认不吸收 |

## 6. 逐条总表（first-parent 时间倒序）

| # | hash | date | verdict | bucket | subject | 判断依据 |
| ---: | --- | --- | --- | --- | --- | --- |
| 1 | `da76ef6b7` | 2026-08-24 15:27 | `absorb` | bugfix | fix(markdown): isolate code highlighting failures | 通用 markdown 高亮隔离，mike 未见等价提交 |
| 2 | `6672e2bc8` | 2026-08-24 12:35 | `skip` | release | chore(release): v0.7.1 | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 3 | `de3fb8ab6` | 2026-08-24 12:14 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | 同步合并点，无独立变更可吸收 |
| 4 | `2db3f41c7` | 2026-08-24 12:14 | `defer-mike-first` | creative/canvas | feat(canvas): refine floating canvas controls | canvas 浮层控件，与 mike video-canvas 重叠 |
| 5 | `ca28973e9` | 2026-08-24 10:40 | `skip` | merge | merge: sync remote main release metadata | 同步合并点，无独立变更可吸收 |
| 6 | `43a266d0b` | 2026-08-24 07:13 | `follow` | test/ci | test: align manifest and canvas contracts | canvas/manifest 契约测试，随 canvas 决策走 |
| 7 | `78ee757df` | 2026-08-24 06:12 | `follow` | test/ci | test(cron): align conversation repository fixtures | 测试/CI 修复随对应功能吸收；单独价值低 |
| 8 | `698ae0598` | 2026-08-24 05:23 | `follow` | test/ci | test(agent): stabilize tracing log capture | 测试/CI 修复随对应功能吸收；单独价值低 |
| 9 | `8401455c0` | 2026-08-24 04:53 | `follow` | test/ci | test(browser): stabilize recovery log capture | 测试/CI 修复随对应功能吸收；单独价值低 |
| 10 | `0feafe4c1` | 2026-08-24 04:25 | `skip` | deadcode-nomi | refactor(ui): remove unused compatibility paths | 基于 nomi 拓扑的死代码清理，直接 cherry-pick 易误删 mike 仍在用路径 |
| 11 | `b600c680a` | 2026-08-24 03:57 | `follow` | test/ci | fix(tests): align managed asset fixtures | 测试/CI 修复随对应功能吸收；单独价值低 |
| 12 | `b0c8fc1ea` | 2026-08-24 03:47 | `skip` | deadcode-nomi | refactor(browser): retire unused mcp bridge | 基于 nomi 拓扑的死代码清理，直接 cherry-pick 易误删 mike 仍在用路径 |
| 13 | `9055e6ceb` | 2026-08-24 03:13 | `skip` | deadcode-nomi | refactor(rust): remove verified dead paths | 基于 nomi 拓扑的死代码清理，直接 cherry-pick 易误删 mike 仍在用路径 |
| 14 | `9cb0e2568` | 2026-08-24 02:36 | `absorb` | i18n-ux-fix | fix(ui): clarify optional model alias field | 可选 alias 字段文案澄清，小改动 |
| 15 | `6b04da861` | 2026-08-24 02:26 | `skip` | release | chore(release): v0.7.0 | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 16 | `fb4ecc0bb` | 2026-08-24 01:47 | `defer-mike-first` | creative/canvas | feat(creative-studio): refine canvas zoom and minimap controls | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 17 | `d73f0203e` | 2026-08-24 01:37 | `follow` | test/ci | test(models): cover responsive advanced editor heights | 测试/CI 修复随对应功能吸收；单独价值低 |
| 18 | `dea748bc2` | 2026-08-24 01:37 | `follow` | test/ci | test(models): align call configuration tab intents | 测试/CI 修复随对应功能吸收；单独价值低 |
| 19 | `b27873cc1` | 2026-08-24 01:37 | `follow` | test/ci | test(knowledge): align retrieval modal with compact chrome | 测试/CI 修复随对应功能吸收；单独价值低 |
| 20 | `35cdc43fb` | 2026-08-24 01:31 | `defer-mike-first` | creative/canvas | fix(creative-studio): drop unreleased workflow projects | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 21 | `c7a9d4201` | 2026-08-24 01:26 | `defer-mike-first` | creative/canvas | perf(creative-studio): load asset pages on demand | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 22 | `527b5218c` | 2026-08-24 01:26 | `defer-mike-first` | creative/canvas | perf(creative-studio): cull offscreen canvas layers | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 23 | `6bc60f8ea` | 2026-08-24 01:26 | `defer-mike-first` | creative/canvas | test(creative-studio): harden agent context projection coverage | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 24 | `690434df5` | 2026-08-24 01:26 | `defer-mike-first` | creative/canvas | perf(creative-studio): build agent context only when visible | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 25 | `472b37431` | 2026-08-24 01:26 | `defer-mike-first` | creative/canvas | perf(creative-studio): cache and deduplicate asset queries | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 26 | `15f4ab1d7` | 2026-08-24 01:26 | `defer-mike-first` | creative/canvas | perf(creative-studio): preload studio routes and show navigation progress | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 27 | `eaafad34c` | 2026-08-24 01:24 | `defer-mike-first` | creative/canvas | perf(creative-studio): keep canvas viewport updates local | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 28 | `72ef904bf` | 2026-08-24 01:24 | `defer-mike-first` | creative/canvas | perf(creative-studio): coalesce canvas interaction persistence | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 29 | `4bf342e76` | 2026-08-24 00:55 | `defer-mike-first` | creative/canvas | fix(creative-studio): keep agent panel flush right | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 30 | `cbe188bb8` | 2026-08-24 00:48 | `skip` | merge | Merge remote-tracking branch 'origin/main' | 同步合并点，无独立变更可吸收 |
| 31 | `abeeac1a1` | 2026-08-24 00:39 | `defer-mike-first` | creative/canvas | refactor(creative-studio): replace workflow domain with templates | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 32 | `f5d36465c` | 2026-08-23 22:33 | `skip` | merge | Merge remote-tracking branch 'origin/main' | 同步合并点，无独立变更可吸收 |
| 33 | `bc76c303c` | 2026-08-23 22:26 | `defer-mike-first` | creative/canvas | fix(i18n): complete Creative Studio localization | Creative Studio 大规模 i18n，跟 mike vimax/video-canvas 重叠 |
| 34 | `c8445a85d` | 2026-08-23 19:35 | `skip` | reverted | fix(ui): move language control to titlebar leading edge | nomi 后续已 Revert（7441ba289），勿吸收 |
| 35 | `7d4d39264` | 2026-08-23 19:24 | `skip` | merge | Merge remote-tracking branch 'origin/main' | 同步合并点，无独立变更可吸收 |
| 36 | `5ab4812f8` | 2026-08-23 19:21 | `defer-mike-first` | creative/canvas | fix(creative-studio): honor provider image size contracts | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 37 | `4b3a067f9` | 2026-08-23 19:11 | `defer-mike-first` | creative/canvas | 素材模板功能上线 | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 38 | `4928cb83e` | 2026-08-23 19:01 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | 同步合并点，无独立变更可吸收 |
| 39 | `038100f72` | 2026-08-23 18:27 | `defer-mike-first` | creative/canvas | fix(creative-studio): unify my assets navigation label | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 40 | `0824f455a` | 2026-08-23 17:51 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | 同步合并点，无独立变更可吸收 |
| 41 | `cceed3874` | 2026-08-23 17:51 | `defer-mike-first` | creative/canvas | 优化提示词交互逻辑 | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 42 | `67da81075` | 2026-08-23 16:29 | `careful` | zh-misc | 移除无用入口 | 中文简述提交，需看 diff 再定；可能与 canvas/模型相关 |
| 43 | `7f6cf37c3` | 2026-08-23 15:38 | `defer-mike-first` | creative/canvas | 画布agent功能完善 | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 44 | `bb5cef5e5` | 2026-08-23 12:02 | `skip` | merge | Merge origin/main: preserve Canvas domain redesign | 同步合并点，无独立变更可吸收 |
| 45 | `80c119a93` | 2026-08-23 11:30 | `defer-mike-first` | creative/canvas | feat(creative-studio): complete Canvas domain redesign | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 46 | `b5aa7cc37` | 2026-08-23 00:29 | `defer-mike-first` | creative/canvas | fix(creative-studio): resume work and checkpoint canvas redesign | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 47 | `e573ed08e` | 2026-08-22 22:43 | `skip` | merge | Merge remote-tracking branch 'origin/main' | 同步合并点，无独立变更可吸收 |
| 48 | `4d5ff3306` | 2026-08-22 22:39 | `defer-mike-first` | creative/canvas | fix(creative-studio): unify navigation with app shell | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 49 | `a1ff20e78` | 2026-08-22 22:03 | `defer-mike-first` | creative/canvas | chore(creative-studio): retire stale canvas audit assets | 与 mike vimax/video-canvas 重叠——mike feat 优先，仅挑可移植 bugfix 再议 |
| 50 | `b0c01e27f` | 2026-08-22 22:03 | `careful` | refactor | refactor(ui): streamline model call configuration | 重构需证明对 mike 有净收益且不破坏 mike feat |
| 51 | `6e34a6379` | 2026-08-22 22:03 | `careful` | models-ux | 优化模型配置 | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| 52 | `b7ea1068a` | 2026-08-22 21:08 | `skip` | style-only | style: refine creative studio layouts | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| 53 | `e7b6850dd` | 2026-08-22 18:00 | `skip` | branding-docs | docs(md) update wechat-group | nomifun 品牌/社区/生态文档，mike 不需要 |
| 54 | `c5e37e454` | 2026-08-22 16:52 | `careful` | migration | fix(db): preserve published migration 36 lineage | 迁移血缘敏感——高风险区，需人工对照 mike migrations，勿盲吸 |
| 55 | `2b77045cd` | 2026-08-22 14:59 | `skip` | merge | Merge branch 'codex/infinite-canvas-rebuild' into main | 同步合并点，无独立变更可吸收 |
| 56 | `3af42d040` | 2026-08-22 14:45 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri . | 同步合并点，无独立变更可吸收 |
| 57 | `716a3b6d7` | 2026-08-22 14:26 | `careful` | refactor | refactor(settings): nest advanced config in the order it actually resolves | 重构需证明对 mike 有净收益且不破坏 mike feat |
| 58 | `bb18f1579` | 2026-08-22 14:11 | `careful` | models-ux | feat(settings): declare supported tasks in a multi-select | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| 59 | `57c23eab7` | 2026-08-22 13:53 | `careful` | models-ux | fix(settings): stop labelling provider creation as "添加模型" | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| 60 | `8139a9390` | 2026-08-22 13:44 | `careful` | models-ux | fix(settings): say which capability config is missing | 设置页能力缺失提示有用，但挂在 nomi 模型管理 redesign 上，宜抽文案/逻辑移植 |
| 61 | `d33a86fe1` | 2026-08-22 10:06 | `careful` | models-ux | style(settings): make the declared-task confirmation impossible to miss | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| 62 | `48b41eba1` | 2026-08-22 09:29 | `careful` | models-ux | fix(settings): make the supported-task picker show what it accepted | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| 63 | `db76510c1` | 2026-08-22 08:57 | `skip` | merge | Merge branch 'refactor/model-management-simplification' | 同步合并点，无独立变更可吸收 |
| 64 | `33cd05064` | 2026-08-21 22:56 | `skip` | docs | docs(handoff): record merge verification and coverage accounting | 文档/handoff/计划类，默认不吸收 |
| 65 | `6668e0124` | 2026-08-21 21:14 | `skip` | merge | Merge branch 'fix/truncation-finalize' | 同步合并点，无独立变更可吸收 |
| 66 | `985651640` | 2026-08-21 05:14 | `skip` | merge | Merge branch 'fix/truncation-not-success' | 同步合并点，无独立变更可吸收 |
| 67 | `682bfb4d2` | 2026-08-20 19:09 | `skip` | style-only | style(ui): unify model and modal styling | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| 68 | `b12228b43` | 2026-08-20 13:56 | `careful` | models-ux | feat(model-management): autofill custom protocols | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| 69 | `aa3071217` | 2026-08-19 22:28 | `follow` | test/ci | fix(test): remove two wall-clock races and one shared-env race | 测试/CI 修复随对应功能吸收；单独价值低 |
| 70 | `c603bd7fd` | 2026-08-19 21:56 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | 同步合并点，无独立变更可吸收 |
| 71 | `b6e61cbcd` | 2026-08-19 21:03 | `follow` | test/ci | fix(test): repair four suites masked behind the first failing binary | 测试/CI 修复随对应功能吸收；单独价值低 |
| 72 | `6aabb255f` | 2026-08-19 19:59 | `follow` | test/ci | fix(ci): unblock the check gate and repair the stale test suites | 测试/CI 修复随对应功能吸收；单独价值低 |
| 73 | `b8104b2ab` | 2026-08-19 16:18 | `skip` | merge | Merge remote-tracking branch 'origin/main' | 同步合并点，无独立变更可吸收 |
| 74 | `4c47bdde7` | 2026-08-19 16:16 | `careful` | models | fix(models): define the /v1 boundary and stop forking inherited config | 模型配置边界修复有价值，但 mike 模型层已分叉，需定向移植 |
| 75 | `180cabe08` | 2026-08-19 02:45 | `absorb` | security-fix | fix(security): reject Windows-specific path constructs before joining | 安全修复，与产品线无关，优先吸收 |
| 76 | `58b595682` | 2026-08-19 02:05 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | 同步合并点，无独立变更可吸收 |
| 77 | `1c5f214c2` | 2026-08-19 01:23 | `skip` | style-only | style(ui): unify modal visual contract | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| 78 | `da36a5e88` | 2026-08-18 23:14 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | 同步合并点，无独立变更可吸收 |
| 79 | `d4953a776` | 2026-08-18 23:13 | `skip` | style-only | style(ui): refine knowledge library layouts | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| 80 | `59a7c844f` | 2026-08-18 00:53 | `skip` | release | chore(release): v0.6.4 | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 81 | `939320710` | 2026-08-17 16:50 | `careful` | i18n | fix(i18n): translate the two strings 77298e98 left untranslated | 补两处漏翻；依赖 FreeModelsContent 路径，确认 mike 仍有对应 UI 再吸 |
| 82 | `c3c58f982` | 2026-08-17 16:39 | `skip` | merge | Merge origin/main into refactor/collapse-engines-to-nomi | 同步合并点，无独立变更可吸收 |
| 83 | `af181d6e9` | 2026-08-17 02:53 | `skip` | engine-divergence | refactor(agent): collapse the engine set to the native nomi executor | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| 84 | `9c82dfb90` | 2026-08-16 20:34 | `skip` | engine-divergence | refactor(ui): drop the openclaw conversation surface | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| 85 | `f2f99a76f` | 2026-08-16 19:52 | `skip` | engine-divergence | refactor(agent): remove the openclaw-gateway engine and retire Star Office | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| 86 | `344f808b0` | 2026-08-16 17:58 | `skip` | engine-divergence | refactor(agent): remove the remote-agent engine | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| 87 | `b73e35f34` | 2026-08-16 15:55 | `skip` | engine-divergence | refactor(agent): remove the nanobot engine | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| 88 | `c3c4580af` | 2026-08-16 14:18 | `skip` | engine-divergence | refactor(conversation): delete the ACP tool-call artifact machine | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| 89 | `c87b8ee1c` | 2026-08-16 13:19 | `skip` | engine-divergence | refactor(agent): remove the ACP-only idle runtime scanner | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| 90 | `02afa0652` | 2026-08-16 12:51 | `skip` | engine-divergence | refactor(conversation): remove the dead usage and openclaw-runtime routes | 删除/折叠引擎路径与 mike 多引擎/ACP 现状冲突，勿整包吸收 |
| 91 | `e0d340734` | 2026-08-16 12:24 | `careful` | refactor | refactor(agent): drop unread provider and binary-path factory deps | 重构需证明对 mike 有净收益且不破坏 mike feat |
| 92 | `3177f52db` | 2026-08-15 16:07 | `skip` | release | chore(release): add Linux updater entry to v0.6.3 latest.json | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 93 | `5c9b95710` | 2026-08-15 15:59 | `skip` | branding-docs | docs(readme): add Gitee repository links | nomifun 品牌/社区/生态文档，mike 不需要 |
| 94 | `6458e57a8` | 2026-08-15 14:42 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | 同步合并点，无独立变更可吸收 |
| 95 | `88d0f5509` | 2026-08-15 14:40 | `careful` | runtime-large | fix(runtime): recover from failover teardown failures | failover 恢复有价值但 diff 极大（MCP/providers/conversation），需拆分移植 |
| 96 | `092ad799a` | 2026-08-15 01:23 | `skip` | merge | Merge remote-tracking branch 'origin/main' | 同步合并点，无独立变更可吸收 |
| 97 | `2d7e4b096` | 2026-08-15 01:22 | `skip` | branding-docs | docs: add Net Infra to the product ecosystem | nomifun 品牌/社区/生态文档，mike 不需要 |
| 98 | `2a5286997` | 2026-08-15 00:14 | `skip` | merge | Merge branch 'codex/fix-glob-tool-hang' | 同步合并点，无独立变更可吸收 |
| 99 | `6e0ff191f` | 2026-08-14 23:53 | `careful` | zh-misc | 优化执行过程，变更 nfagent 加载机制 | 中文简述提交，需看 diff 再定；可能与 canvas/模型相关 |
| 100 | `f7cbae60e` | 2026-08-14 18:23 | `skip` | merge | merge: integrate latest origin/main | 同步合并点，无独立变更可吸收 |
| 101 | `37fb2407d` | 2026-08-14 18:04 | `careful` | runtime-large | fix(conversation): harden agent runtime stability | agent runtime 硬化有价值但触及 MCP/tool_execution 面广，需对照 mike 后拆分 |
| 102 | `fae5658c2` | 2026-08-13 11:07 | `skip` | dev-tooling | fix(dev): invalidate relocated Cargo build cache | 仅 scripts/prune-build.mjs 开发缓存清理，非产品 bugfix |
| 103 | `4a2eacfd4` | 2026-08-13 11:07 | `careful` | feat-channels | feat(channels): add configurable group access | 频道群组访问是独立 feat；体积大且碰 channel/api-types，人工评估后吸收 |
| 104 | `e701e5782` | 2026-08-13 01:43 | `skip` | branding-docs | docs: refresh ecosystem and Docker guidance | nomifun 品牌/社区/生态文档，mike 不需要 |
| 105 | `120573262` | 2026-08-13 00:48 | `skip` | branding-docs | docs: document the NomiFun product family | nomifun 品牌/社区/生态文档，mike 不需要 |
| 106 | `3d7f5a9b6` | 2026-08-12 14:11 | `skip` | release | chore(release): add Linux assets for v0.6.1 | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 107 | `b3780e953` | 2026-08-12 13:39 | `skip` | release | chore(release): add macOS assets to v0.6.1 latest.json | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 108 | `76f49d4fa` | 2026-08-12 13:20 | `skip` | release | chore(release): v0.6.1 | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 109 | `4a65b5eba` | 2026-08-12 12:22 | `careful` | models-ux | 优化模型类型与目录选择交互 | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| 110 | `e8fa02081` | 2026-08-12 10:13 | `skip` | merge | Merge remote-tracking branch 'origin/main' | 同步合并点，无独立变更可吸收 |
| 111 | `ffdb4f4f3` | 2026-08-12 09:13 | `careful` | models-ux | 模型管理改造 | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| 112 | `11116a5be` | 2026-08-11 14:37 | `skip` | release | chore(release): v0.6.0 | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 113 | `9c50985bf` | 2026-08-11 14:12 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | 同步合并点，无独立变更可吸收 |
| 114 | `97ade6be9` | 2026-08-11 11:53 | `skip` | already-on-mike | fix(enhanced-tools): prevent duplicate market additions | mike 已有等价提交 765a653d8 fix(enhanced-tools): prevent duplicate market additions |
| 115 | `f5a5314af` | 2026-08-10 14:21 | `skip` | already-on-mike | fix(conversation): refine process and collaboration UI | mike 已有等价提交 72f1dfd3d fix(conversation): refine process and collaboration UI |
| 116 | `d3a68d528` | 2026-08-10 12:48 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | 同步合并点，无独立变更可吸收 |
| 117 | `82c94411f` | 2026-08-10 12:47 | `careful` | updater-ux | feat(ui): unify update notification presentation | 更新提示 UI；对照 mike updater 状态后再定 |
| 118 | `755cfb310` | 2026-08-10 01:16 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-desktop | 同步合并点，无独立变更可吸收 |
| 119 | `7361efd56` | 2026-08-10 01:15 | `skip` | style-only | style(ui): compact tool and creation dialogs | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| 120 | `d6abd755d` | 2026-08-10 00:25 | `skip` | style-only | style(markdown): unify document preview typography | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| 121 | `fffcb8901` | 2026-08-09 21:25 | `skip` | already-on-mike | 补充小智ai 接入文档内容， 项目仓库 nomifun-xiaozhi-yuntai | mike 已 absorb robot/StepFun/xiaozhi |
| 122 | `48fa2bbe2` | 2026-08-09 20:47 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | 同步合并点，无独立变更可吸收 |
| 123 | `0578d0107` | 2026-08-09 20:45 | `skip` | already-on-mike | 支持小智esp32 机器人连接，伙伴即机器人 | mike 已 absorb robot/StepFun/xiaozhi |
| 124 | `51427574b` | 2026-08-08 20:02 | `skip` | release | chore(release): add Linux updater entry to v0.5.0 latest.json | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 125 | `48a9a0fa3` | 2026-08-08 14:17 | `skip` | already-on-mike | feat(robot): 补齐 StepFun 语音链路（目录+失败可见+侧栏分组+健康检查） | mike 已 absorb robot/StepFun/xiaozhi |
| 126 | `2ae77e220` | 2026-08-08 12:03 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | 同步合并点，无独立变更可吸收 |
| 127 | `ec9e31d62` | 2026-08-08 12:02 | `skip` | branding-docs | 更新微信群二维码 | nomifun 品牌/社区/生态文档，mike 不需要 |
| 128 | `623b2fa6b` | 2026-08-08 02:45 | `skip` | release | chore(release): v0.5.0 | nomi 发行元数据/版本号，与 mike 发布线无关 |
| 129 | `96e0b5ab4` | 2026-08-08 02:00 | `careful` | models-ux | fix(settings): preserve provider edit save payload | provider 编辑保存 payload 修复有价值；wire 层与 mike 可能已分叉 |
| 130 | `34de3b229` | 2026-08-08 01:39 | `careful` | skills | fix: make SkillHub expert package installs reliable | Skill 安装可靠性；对照 mike skill 市场现状 |
| 131 | `2bf4eee03` | 2026-08-08 00:12 | `careful` | unclear | feat: unify enhanced tools market controls | 需看 diff 才能定 |
| 132 | `959d583f0` | 2026-08-07 20:13 | `skip` | style-only | style: compact update modal layout | 纯样式统一，非 bug/feat；mike 视觉体系不同，低优先 |
| 133 | `cb6ca9aba` | 2026-08-07 00:52 | `skip` | merge | Merge branch 'feat/model-hub-restructure' into main | 同步合并点，无独立变更可吸收 |
| 134 | `2d1463fa5` | 2026-08-07 00:13 | `absorb` | bugfix | fix(knowledge): 修掉新建入口的焦点不可见，并让对比度测试真的在测东西 | Bugfix 默认倾向吸收；落地前仍需 diff 冲突检查 |
| 135 | `c10b1f2ec` | 2026-08-07 00:03 | `skip` | merge | Merge branch 'fix/updater-state-ownership' into main | 同步合并点，无独立变更可吸收 |
| 136 | `2f53522f3` | 2026-08-06 20:19 | `skip` | merge | Merge remote-tracking branch 'origin/main' | 同步合并点，无独立变更可吸收 |
| 137 | `628cd0130` | 2026-08-06 20:19 | `skip` | already-on-mike | fix(robot): make test-support implicit for the crate's own tests | mike 已 absorb robot/StepFun/xiaozhi |
| 138 | `73f4124f8` | 2026-08-06 15:41 | `skip` | docs | docs(deploy): explain how LAN robots reach a server deployment | 文档/handoff/计划类，默认不吸收 |
| 139 | `6f192f464` | 2026-08-06 15:41 | `skip` | already-on-mike | feat(robot): advertise a reachable endpoint from headless hosts | mike 已 absorb robot/StepFun/xiaozhi |
| 140 | `894f4f131` | 2026-08-06 10:56 | `skip` | already-on-mike | docs(contributing): record the robot gateway's two build prerequisites | mike 已 absorb robot/StepFun/xiaozhi |
| 141 | `336838b3d` | 2026-08-06 10:55 | `skip` | already-on-mike | feat(robot): honour the companion profile's chosen VAD engine | mike 已 absorb robot/StepFun/xiaozhi |
| 142 | `adbcedff7` | 2026-08-06 10:51 | `skip` | already-on-mike | test(robot): add fake-device end-to-end integration test | mike 已 absorb robot/StepFun/xiaozhi |
| 143 | `6355a8dbd` | 2026-08-06 10:46 | `skip` | already-on-mike | feat(robot): add management REST face and host assembly | mike 已 absorb robot/StepFun/xiaozhi |
| 144 | `ca1bfe605` | 2026-08-06 10:20 | `skip` | already-on-mike | feat(robot): wire ASR, TTS and one-shot vision to the model layer | mike 已 absorb robot/StepFun/xiaozhi |
| 145 | `83beb385b` | 2026-08-06 09:55 | `skip` | already-on-mike | feat(robot): add vision explain endpoint for device photo understanding | mike 已 absorb robot/StepFun/xiaozhi |
| 146 | `492b37509` | 2026-08-06 09:54 | `skip` | already-on-mike | fix(conversation): keep robot threads out of the ordinary session list | mike 已 absorb robot/StepFun/xiaozhi |
| 147 | `97d754728` | 2026-08-06 09:52 | `skip` | already-on-mike | feat(robot): add tool registry and loopback MCP proxy for robot tools | mike 已 absorb robot/StepFun/xiaozhi |
| 148 | `278b4ad28` | 2026-08-06 09:50 | `skip` | already-on-mike | feat(robot): add the robot connection section to the companion remote tab | mike 已 absorb robot/StepFun/xiaozhi |
| 149 | `16d180923` | 2026-08-06 09:49 | `skip` | already-on-mike | feat(robot): add device MCP client with paging and tolerant error handling | mike 已 absorb robot/StepFun/xiaozhi |
| 150 | `90c24e742` | 2026-08-06 09:44 | `skip` | already-on-mike | feat(robot): project robot.status into a live per-device map | mike 已 absorb robot/StepFun/xiaozhi |
| 151 | `de7a60fc8` | 2026-08-06 09:42 | `skip` | already-on-mike | feat(robot): wire uplink, dispatch and downlink into the session loop | mike 已 absorb robot/StepFun/xiaozhi |
| 152 | `16fc6c1ee` | 2026-08-06 09:38 | `skip` | already-on-mike | feat(robot): add the robot management wire contract to the bridge | mike 已 absorb robot/StepFun/xiaozhi |
| 153 | `54e01ad45` | 2026-08-06 09:35 | `careful` | models-ux | docs(modelhub): narrow the providers section to access and credentials | 模型管理 UX/结构与 mike 已分叉；可吸收独立 bugfix，勿整包 redesign |
| 154 | `c23a15b0a` | 2026-08-06 09:33 | `skip` | already-on-mike | feat(modelhub): build the chat, vision and embedding modality sections | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| 155 | `68507e92c` | 2026-08-06 09:29 | `skip` | already-on-mike | feat(modelhub): project provider_models rows into modality groups | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| 156 | `17fb330fd` | 2026-08-06 09:26 | `skip` | already-on-mike | feat(modelhub): give the voice section a TTS default, a catalog-only ASR and the local VAD entry | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| 157 | `91f86a37b` | 2026-08-06 09:23 | `skip` | already-on-mike | feat(robot): add downlink pacer with generation-based flush | mike 已 absorb robot/StepFun/xiaozhi |
| 158 | `468066133` | 2026-08-06 09:20 | `skip` | already-on-mike | feat(modelhub): make the hub a modality-first view with eight sections | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| 159 | `17b943638` | 2026-08-06 09:20 | `skip` | already-on-mike | feat(robot): add uplink pipeline with VAD endpointing | mike 已 absorb robot/StepFun/xiaozhi |
| 160 | `a43b86412` | 2026-08-06 09:19 | `skip` | already-on-mike | feat(robot): add speech and dispatcher trait seams with mocks | mike 已 absorb robot/StepFun/xiaozhi |
| 161 | `d7019bad0` | 2026-08-06 09:17 | `skip` | already-on-mike | feat(robot): add incremental sentence splitter and emotion markers | mike 已 absorb robot/StepFun/xiaozhi |
| 162 | `aecd736ba` | 2026-08-06 09:16 | `skip` | already-on-mike | feat(robot): add silero ONNX VAD with energy fallback | mike 已 absorb robot/StepFun/xiaozhi |
| 163 | `61e52d200` | 2026-08-06 09:15 | `skip` | already-on-mike | feat(companion): rebuild the overview model section as five kinds of slot | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| 164 | `7c04a18e2` | 2026-08-06 09:12 | `careful` | feat | feat(models): add the shared TaskModelSelect and rebuild the companion chat model control on it | 新功能；与 mike 重叠则 mike 优先，独立域可吸收 |
| 165 | `44d5bb76f` | 2026-08-06 09:08 | `skip` | already-on-mike | feat(models): extract the shared task-model selector decision logic | TaskModelSelect 共享选择器，mike 已有 task-filtered model selectors |
| 166 | `df0df6716` | 2026-08-06 09:07 | `skip` | already-on-mike | feat(robot): add VAD abstraction with energy engine | mike 已 absorb robot/StepFun/xiaozhi |
| 167 | `ae0a0e9a5` | 2026-08-06 09:06 | `skip` | already-on-mike | feat(companion): mirror the new model slots on the profile wire type | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| 168 | `1d17e5d22` | 2026-08-06 09:01 | `skip` | already-on-mike | feat(robot): add opus codec, wav packing, resampling and container decode | mike 已 absorb robot/StepFun/xiaozhi |
| 169 | `35fdc1773` | 2026-08-06 08:56 | `skip` | already-on-mike | feat(tts): add the tools.textToSpeech install-wide preference | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| 170 | `d64234a37` | 2026-08-06 08:48 | `skip` | already-on-mike | feat(robot): add websocket endpoint, LAN link source and session actor | mike 已 absorb robot/StepFun/xiaozhi |
| 171 | `25b896e66` | 2026-08-06 08:45 | `skip` | already-on-mike | feat(robot): add status registry and robot.status realtime event | mike 已 absorb robot/StepFun/xiaozhi |
| 172 | `a44b962d9` | 2026-08-06 08:43 | `skip` | already-on-mike | feat(robot): add OTA report and activation endpoints | mike 已 absorb robot/StepFun/xiaozhi |
| 173 | `9c7be042c` | 2026-08-06 08:39 | `skip` | already-on-mike | feat(robot): add endpoint advertiser with LAN implementation | mike 已 absorb robot/StepFun/xiaozhi |
| 174 | `6635c88b4` | 2026-08-06 08:38 | `skip` | already-on-mike | feat(robot): add v1 binary framing and transport-agnostic link traits | mike 已 absorb robot/StepFun/xiaozhi |
| 175 | `f7aba0dbc` | 2026-08-06 08:37 | `skip` | already-on-mike | feat(robot): add xiaozhi JSON message vocabulary | mike 已 absorb robot/StepFun/xiaozhi |
| 176 | `25afd5008` | 2026-08-06 08:36 | `skip` | already-on-mike | feat(companion): add fallback, vision and voice model slots to the profile | companion/modelhub/robot/tts 批次多已由 absorb-u-main 覆盖，勿重复整包 |
| 177 | `09fa2960e` | 2026-08-06 08:35 | `skip` | already-on-mike | feat(robot): add robot registry with token rotation and activation codes | mike 已 absorb robot/StepFun/xiaozhi |
| 178 | `d80c50a26` | 2026-08-06 08:34 | `skip` | already-on-mike | feat(robot): scaffold nomifun-robot crate | mike 已 absorb robot/StepFun/xiaozhi |
| 179 | `9a4d06e10` | 2026-08-06 08:28 | `skip` | already-on-mike | docs(plans): add robot bridge implementation plans A/B/C | mike 已 absorb robot/StepFun/xiaozhi |
| 180 | `abb75045c` | 2026-08-06 06:27 | `skip` | already-on-mike | docs(specs): add robot bridge design, supersede xiaozhi integration spec | mike 已 absorb robot/StepFun/xiaozhi |
| 181 | `a9718894e` | 2026-08-06 04:15 | `skip` | merge | Merge origin/main into the write-back simplification | 同步合并点，无独立变更可吸收 |
| 182 | `ab51a65b9` | 2026-08-06 03:56 | `skip` | merge | Merge remote-tracking branch 'origin/main' into refactor/knowledge-writeback-simplification | 同步合并点，无独立变更可吸收 |
| 183 | `c71f30731` | 2026-08-06 02:57 | `careful` | refactor | refactor(ui): delete the review surface and reshape the disposition control | 重构需证明对 mike 有净收益且不破坏 mike feat |
| 184 | `e5710c10b` | 2026-08-06 02:24 | `skip` | already-on-mike | fix: absorb a verbatim restatement instead of doubling the document | knowledge writeback 简化链路，mike 已 absorb-u-main |
| 185 | `521eab10f` | 2026-08-06 02:05 | `skip` | already-on-mike | refactor: make the manual disposition real and finish retiring placement | writeback disposition 重构，mike 已吸收 |
| 186 | `a233f5fd4` | 2026-08-06 01:32 | `skip` | already-on-mike | refactor(knowledge): make write-back direct-only, manual/auto, and non-destructive | writeback direct-only，mike 已吸收 |
| 187 | `339145f1e` | 2026-08-06 00:48 | `skip` | already-on-mike | refactor(db): drop writeback_mode and move the disposition to manual/auto | drop writeback_mode 迁移，mike 已吸收（见 bb63ab833） |
| 188 | `7dac8fa58` | 2026-08-05 23:57 | `skip` | already-on-mike | docs: lay out the write-back simplification task by task | mike 已 absorb-u-main（knowledge writeback） |
| 189 | `80b528027` | 2026-08-05 23:51 | `skip` | already-on-mike | docs: pin the code-level approach for the write-back simplification | mike 已 absorb-u-main（knowledge writeback） |
| 190 | `8632763db` | 2026-08-05 22:27 | `skip` | already-on-mike | docs: design the knowledge write-back simplification | mike 已 absorb-u-main（knowledge writeback） |
| 191 | `9599ddc13` | 2026-08-05 20:07 | `skip` | merge | Merge branch 'main' of https://github.com/nomifun/nomifun-tauri | 同步合并点，无独立变更可吸收 |
| 192 | `98844cfd9` | 2026-08-05 14:38 | `skip` | docs | docs: state the outline-2 failure as measured, not as inferred | 文档/handoff/计划类，默认不吸收 |
| 193 | `12fd8b9fe` | 2026-08-05 14:04 | `absorb` | bugfix | fix: paint the separators and focus rings, and close the dead-utility class for good | 主题分隔线/焦点环未渲染类 bugfix，产品无关 |
| 194 | `cd403baa2` | 2026-08-05 11:24 | `absorb` | bugfix | fix: paint the borders and backgrounds that never rendered, and make Alerts legible | 边框/背景未渲染与 Alert 可读性，产品无关 |
| 195 | `16381ffd0` | 2026-08-05 09:09 | `absorb` | bugfix | fix: make every written colour actually render, and freeze the reset plan shape | CSS 变量颜色未生效修复，产品无关 |
| 196 | `c80555cef` | 2026-08-05 07:19 | `absorb` | bugfix | fix: repair the factory-reset registry regression and clear the red suites | factory-reset registry 回归修复，值得吸 |
| 197 | `49431c307` | 2026-08-05 05:14 | `skip` | merge | Merge branch 'feat/companion-workspace-redesign' | 同步合并点，无独立变更可吸收 |
| 198 | `2b728bfc9` | 2026-08-05 03:06 | `skip` | merge | Merge branch 'feature/ssh-remote-session' | 同步合并点，无独立变更可吸收 |
| 199 | `0ba785c42` | 2026-08-05 03:05 | `skip` | merge | Merge remote-tracking branch 'origin/main' | 同步合并点，无独立变更可吸收 |
| 200 | `67e64b5d9` | 2026-08-03 20:33 | `skip` | docs | docs: fix 14 audit defects in bridge plans and protocol | 文档/handoff/计划类，默认不吸收 |
| 201 | `3d3fad5c9` | 2026-08-03 19:53 | `skip` | docs | docs: add implementation plans for bridge, relay server and mobile app | 文档/handoff/计划类，默认不吸收 |
| 202 | `6a25aaae6` | 2026-08-03 18:36 | `skip` | docs | docs: add bridge protocol v1 shared reference | 文档/handoff/计划类，默认不吸收 |
| 203 | `d617c9474` | 2026-08-03 10:55 | `skip` | docs | docs: add nomifun mobile remote-control bridge design spec | 文档/handoff/计划类，默认不吸收 |

## 7. 执行记录（2026-08-24，`feat/absorb-nomi-priority-fixes`）

基于 `mike/main`（`d791691c6`）开分支执行 §1。

| 源 hash | 结果 | 落地说明 |
| --- | --- | --- |
| `2d1463fa5` knowledge 焦点 | **已吸收** → `28601a71b` | 冲突解决：保留 mike empty-state drop-zone；虚线卡片/胶囊 CTA 改用可编译的 `focus-visible:*-primary-6`（替换会丢弃的 `rgb(var(--primary-6))`） |
| `180cabe08` path 安全 | **已吸收** → `19b25756e` | `zip_safe` 取 nomi 更严 Windows colon/ADS 策略；其余 path_safety / memory / skill 一并合入 |
| `da76ef6b7` markdown 隔离 | **已吸收（适配）** → `d62064121` | **未**切换 Prism；保留 mike HLJS + Beautiful UI chrome；移植 `SyntaxHighlightBoundary`、`resolveSyntaxLanguage`、纯文本 fallback |
| `9cb0e2568` model alias 文案 | **跳过** | `ModelDefinitionEditor` 在 mike 已删除，相关 i18n key 亦不存在 |
| `c80555cef` / `16381ffd0` / `cd403baa2` / `12fd8b9fe` 主题/factory-reset | **本轮跳过** | 与 companion redesign / 数百 UI 文件缠死，整包 cherry-pick 冲突面过大；改标 careful 待拆 patch |

验证：

- `bun test`：Markdown 相关 3 文件 + knowledge CTA contrast → 13 pass
- `cargo test -p nomifun-common zip_safe` → 9 pass
- `cargo test -p nomifun-file path_` → path traversal 相关用例 pass

## 8. 后续任务建议

1. **§2 careful**：按 bucket 分批（`models` / `migration` / `feat` / `refactor` / `runtime-large`），每批先 `git show <hash>` 再决定整吸或手工移植。
2. **§3 creative/canvas**：默认关闭；若只要某条 bugfix，从 commit 抽 patch，不要跟 redesign。
3. **不要**对全量非 merge 独有提交做 merge。
4. **下一轮**：从主题/factory-reset 四条 mega commit 只抽确仍缺失的 CSS/脚本修复；评估 `4a2eacfd4` channels group access。

## 9. 元数据

- remote nomi: `nomifun/nomifun-tauri` → `nomi/main` @ `ae87ae915`
- remote mike: `Michael-Lfx/allo` → `mike/main` @ `d791691c6`
- 执行分支：`feat/absorb-nomi-priority-fixes`（ahead of `mike/main` by 3）
- 生成命令：`git log --first-parent --format="%h|%ci|%s" remotes/mike/main..remotes/nomi/main`
- 原始列表：`.absorb_tmp/nomi_first_parent.txt`（已 gitignore）
- 本判断为 subject 级启发式 + 已知吸收锚点；落地前仍以 `git show` / 冲突实测为准。

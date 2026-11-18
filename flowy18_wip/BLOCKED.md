# BLOCKED — 待裁决/越界事项（不阻塞已交付部分）

## 2026-11-18 · 后续审查已解除 Clippy 阻塞

- rebase 后的首次 Clippy 退出 101，恰好报任务书的 4 条 `flowy-ui` lint。应用四处修复后，原命令通过了 `flowy-ui` 检查，继续检查 `flowy-studio` 时退出 101。
- `crates/flowy-studio/src/library/view.rs:20`：`doc_markdown`，文档中的 `IconPark` 缺少反引号。
- `crates/flowy-studio/src/library/view.rs:38`：`map_unwrap_or`，对 `Option` 调用 `map(...).unwrap_or_default()`。
- `crates/flowy-studio/src/library/view.rs:634`：`trivially_copy_pass_by_ref`，通过引用传递 5 字节的 `ResolvedTheme`。
- 后续获准扩展到验收阻塞文件，已修复上述 3 条和继续编译发现的 Library 示例日志、技能页共享借用两条。最终 Clippy、反向验证、fmt 和原四 crate 测试均通过。
- 保留本地既有特性提交和只读方案文档。从 `origin/main` 建立独立 PR 分支，范围限于本轮修复和本轮验证记录。

## 2026-11-18 · Clippy 任务的整分支差异验收

- rebase 前实测 `git rev-list --left-right --count origin/main...HEAD` 为 `1 5`，rebase 到 `9f51499af` 后为 `0 5`，既有 5 个提交涉及 49 个文件。要求整分支对 `origin/main` 的差异仅有本轮四个白名单文件，与保留既有特性提交冲突。
- 工作区进入任务时已有两份未跟踪文档 `docs/plans/model-tiers-and-flex-final.zh.md`、`docs/plans/model-tiers-and-flex-recommendation.md`。按只读要求保留，两份文档会继续出现在 `git status`。
- 本轮使用 rebase 后 HEAD `3195aa82f` 核对新增提交范围，仍执行任务指定的整分支差异命令并如实报告。

## 三挡位模型选择器 — 历史记录

## 1. flowy-ui/src/overlays.rs 在改动集内但不在原白名单
- 现状：`flowy18_wip/crates/flowy-ui/src/overlays.rs` 有 9 行未提交改动（`menu_item` 把 label 渲染成可见文本），属上一轮遗留，本次未改动。
- 影响：菜单行文字依赖此改动才可见；回滚会导致菜单行空白，违反验收第 4 条。
- 待裁决：交付判卷时是否视为既有基线；若必须严格白名单，需要单独提交或说明。

## 2. i18n（已解决，2026-11-18）
- 23 个语种包含 `credits.plan_pro`、`credits.plan_ultra`、`model.unavailable`、`model.menu_title`；已移除无引用的 `credits.plan_paid`。
- 验证：`cargo test -p flowy-i18n` 4 passed（含 `every_locale_has_the_same_keys`）；脚本 parity `541 keys x 23 locales`。

## 3. 已记录、未顺手修的问题
- 目录未加载时菜单只剩标题+刷新项（无三行占位、无 panic）；按任务 4 只验证不 panic，未加空态文案。
- 已存 Pro 挡位但当前账号 Free（如付费期选过 Max 后降级）：启动时锁定不应用路由、无法发送，但 chip 仍显示该挡位为已选（`prefs.rs` 有意保留存储挡位，授权以 effectiveModelTier 为准）。属产品行为待裁决，不会越权发送。
- `crates/flowy-app/locales/` 与 `crates/flowy-i18n/locales/` 重复且无代码引用（已确认无 include_str/读取）；未同步、未删除。删除属不可回收操作，需确认后处理。

## 4. 环境限制（不影响结论）
- 本机 shell 的 curl/PowerShell TLS 不可用（schannel `SEC_E_NO_CREDENTIALS`），无法在命令行预查云端目录；实跑用应用自身 rustls 栈验证。
- `cargo` 在 Windows 下需把 `CARGO_TARGET_DIR`/`TEMP` 转成 Windows 路径（cygpath -w），否则拼出 `D:/d/...` 失败；关闭窗口后 flowy-app 可能仍驻留（托盘），重建前需确认进程已退出。

## 已合入 main 的 Clippy 修复（#461）

无本地门禁阻塞。Cargo 仍输出基线已有的 manifest 和 profile 元数据提示。Windows 示例的程序内截图接口返回拒绝访问，交互验证范围见 PROGRESS。

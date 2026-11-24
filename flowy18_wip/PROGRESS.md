# 迁移进度（gpui-ce → gpui-pre + gpui-kit）

## 2026-11-24 · Clippy 欠债清理（fix-flowy18-clippy-debt）

- 理解的目标：让 `flowy18-ui` 的 UI clippy 门禁在干净 main 上转绿，并修掉让 Windows 作业长期静默跳过的依赖预检。
- 顺序：任务 0 核对基线 → flowy-agent 8 条 → 量清并修 flowy-app → 修 CI 预检 → 全量验证 → 推分支开 PR。
- 最大风险：flowy-app 的确切条数在 flowy-agent 清零前量不出来，可能远超预期；`unused import: prelude` 删法不对会让 `div()`/`.w()` 全失效。
- 任务 0 实测（2026-11-24，本工作树）：HEAD `0d78e116b`；门禁命令退出 101，`flowy-agent` lib 8 条 error；测试 157 passed / 0 failed / 0 ignored（ui 30+2、i18n 8、studio 74、app 43）。与任务书一致，开工。

## 开工回执
- 理解目标：换依赖到 gpui-pre =0.3.14 + gpui-kit 0.11，重做 Flowy Studio 首页。
- 实际路线：**放弃**移植 `editable_text`（证明不可行），改用 `gpui_kit::base::input`
  的 `Input` / `Textarea`。`editable_text` 在本仓库任何提交中都不曾存在，
  `BLOCKED.md` 里记的是未开始的 TODO，不是被删除的代码。
- 基线（2026-11-23 23:20）：flowy-studio 74 通过、flowy-ui 30 通过。

## 状态

- [x] 任务 0：基线确认 74/30
- [x] 任务 1：改依赖到 `gpui-pre =0.3.14` + `gpui-kit 0.11`
- [x] 任务 2：`editable_text` 移植 —— **判定不可行并删除该方向**
- [x] 任务 3：适配剩余 API（ce 独有 API 全面清理）
- [x] 任务 4：接 gpui-kit 的 `Input` / `Textarea`
- [ ] 任务 5：首页重做（代码齐全，待运行时验证）
- [ ] 任务 6：全量验收

## 编译状态（2026-11-24）

| Crate | `cargo check` | `cargo check --tests` |
| --- | --- | --- |
| `flowy-ui` | 0 error | 0 error |
| `flowy-studio` | 0 error | 0 error |
| `flowy-shell` | 0 error | — |
| `flowy-app` | 0 error | 0 error |
| `flowy-desktop` | 0 error | — |

`cargo check --workspace` exit 0，`cargo build -p flowy-desktop` 成功。

### 验收记录（2026-11-24）

- 迁移 PR #479 已合并；lint 欠债 PR #480 已合并（合并提交 `d0852b004`）。
- CI `Flowy 1.8 UI` 的 Windows 作业第 9–15 步全绿：格式、flowy-ui / flowy-i18n /
  flowy-studio / flowy-app 测试、desktop 编译、UI clippy。
- 测试合计 **157 passed / 0 failed / 0 ignored**（ui 30 + 集成 2、i18n 8、studio 74、app 43）。
  旧记录写的 155 漏掉了 `flowy-ui/tests/anchor_measurement.rs` 的 2 个用例。

## 迁移期功能取舍（技术债，需后续补）

1. **popover / 菜单 / tooltip**：`gpui-pre` 无 `on_prepaint`，锚点测量一度丢失，
   正在修复中。
2. **多行输入**：`flowy-ui` 的 `TextField` 只接受 `InputState`，`about` /
   `instructions` / `import_paste` / `knowledge_body` 四个多行语义字段当前走单行引擎。
   要真多行需新增接受 `TextareaState` 的控件。
3. **逐键硬截断**：`set_value` 需要 `&mut Window`，`Context::subscribe` 拿不到，
   改为保存时截断。
4. **RTL 根方向**：`gpui-pre` 只有全局 `LayoutDirection`，未接线。
5. **a11y 属性**：`aria_live` / `aria_disabled` / `aria_modal` / `aria_hidden` 在
   `gpui-pre` 中不存在，生产代码中已删除，读屏播报与隐藏语义退化。
6. **系统托盘**：已恢复。`gpui-tray` 接 `gpui-kit` feature，加载的正是 `gpui-pre`，类型图一致；托盘有图标和 Show / Quit 菜单。关窗行为看托盘是否可用——可用时最小化（`gpui-pre` 无 `set_visible`），不可用时直接退出。
7. **Tab 语义**：输入框内 Tab 只移动焦点，不再插入缩进。
8. **光标闪烁间隔**：`caret_blink_interval_500ms` 无对应 API，交回引擎默认。
9. **letter_spacing**：`gpui-pre` 无此属性，字距回默认。
10. **启动行为**：新增 `session_opened_by_user` 守卫，启动固定落 Home，
    不再自动恢复上次会话（产品行为变更）。

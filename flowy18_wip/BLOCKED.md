# 迁移阻塞记录（gpui-ce → gpui-pre + gpui-kit）

## 结论（2026-11-24）

`gpui-pre =0.3.14` + `gpui-kit 0.11` 可以承载 `flowy18_wip`，但需要接受一组
能力缺口。本文件记录**已验证不可行**的方向和**已知的功能损失**，避免后来者
重复踩坑。

## 已验证不可行的方向

### 1. 移植 `editable_text`

`gpui-ce` 的 `editable_text` 是在 `gpui-pre` 的排版层之上加的一层封装
（`ShapedText`、`ShapedTextLayout`、`CaretPosition`/`CaretSelection`、
`UnicodeBidi`、`TextLayoutOptions`、`ColorExt`）。移植尝试后错误数长期在
120～182 之间震荡，无法收敛：

- `gpui-pre` 的 `shape_text` 返回 `SmallVec<[WrappedLine; 1]>`，ce 返回 `ShapedText`
- `WrappedLine` 不实现 `Clone`
- `closest_index_for_position` 返回 `Result<usize, usize>`，签名不同
- `WrappedLine::paint` 多出 `align` / `align_width` 参数

**结论**：不移植。直接用 `gpui_kit::base::input` 的 `Input` / `Textarea`，
`flowy-ui` 只保留样式壳。

### 2. 热补丁（hot-patching，已解决）

早期 `dx serve` 强制 `-Clinker=dx.exe` 导致两种模式均在链接阶段失败（`exit 1` / `LNK1104`）。
2026-11-26 排查并确认根因在 `dioxus-cli 0.7.10` 的 Windows 实现（未给含空格的响应文件
`/LIBPATH` 加引号、`DX_LINK_CUSTOM_LINKER` 绕过 `NoLink` 且捕获环境回灌、MSVC
`/INCREMENTAL` `@ILT` 偏移与 `libdeps-*.a` 冲突、x64 `stub.o` 破坏 `__chkstk` `RAX`
入参以及 1-hop 监听未覆盖传递 UI crate）。现已通过 `scripts/dev-hotpatch.ps1` +
`scripts/dx-capture-wrapper`（含 `dx-link-wrapper`）+ rustup `lld-link.exe` +
`flowy-desktop` 直接声明 UI crate 依赖全部修复，Windows 下 `flowy-app` 与 `flowy-studio`
原位热补丁均已实测跑通（详见 `docs/gpui-pre-and-kit.md`）。

### 3. `gpui-tray`（已解决）

`gpui-tray` 用一个 cargo feature 桥接三套 GPUI 类型图。工作区选的是 `gpui-kit`，
它的真包就是 `gpui-pre`，所以 `MenuItem` / `App` / `Action` 类型一致，托盘代码已接回：
图标、Show / Quit 菜单，`flowy_shell::tray_available()` 报告是否可用。

**从带标签的目录启动时建不出来**：`Shell_NotifyIconW` 返回 `os error 5`（拒绝访问）。
根因是强制完整性标签——从 Low 标签的目录执行，进程是 `S-1-16-4096`（Low），Windows
不允许低完整性进程添加托盘图标。同一份 exe 拷到普通目录（`S-1-16-8192`，Medium）
即正常，关窗最小化到托盘。安装包与正常安装目录不受影响。

## 能力缺口（`gpui-pre` 没有）

| 能力 | ce 的写法 | pre 的现状 |
| --- | --- | --- |
| 元素 prepaint 钩子 | `.on_prepaint(..)` | 无（popover 锚点测量需另想办法） |
| ARIA 属性 | `.aria_live` `.aria_disabled` `.aria_modal` `.aria_hidden` | 无，生产代码中已删 |
| a11y 测试 API | `window.a11y_tree()` 等 | 无，改用 `gpui-kit/test-support` |
| 颜色工具 | `gpui::ColorExt`、`rgb_to_hsla` | `Hsla::from(rgba)` 等价；`ColorExt` 无 |
| Enter action | `gpui::Enter` | `gpui_kit::base::input::Enter { secondary, shift }` |
| 元素级样式 | `AnyElement` 可链 `.min_h` 等 | `AnyElement` 无 `Styled`，须包 `div()` |
| `emplace` | `state.emplace(text, cx)` | 无；`set_value(text, window, cx)` 需要 window |
| 字母间距 | `letter_spacing(px(..))` | 无 |
| RTL 根方向 | `.rtl()` / `.ltr()` | 只有全局 `LayoutDirection` |
| 系统托盘 | `gpui-tray` | 代码已接回（`gpui-kit` → `gpui-pre`）；从低完整性目录启动时 shell 拒绝 |

## 当前已知遗留（不影响编译，影响行为或体验）

1. popover / 菜单 / tooltip 的锚点测量：**已修复**，见 `flowy-ui/src/overlays.rs`
   的 `PrepaintBounds` 与 `tests/anchor_measurement.rs`（2 个回归测试）
3. 逐键硬截断改为保存时截断
4. RTL 未接线，阿语/希语下根方向与局部方向不一致
5. a11y 播报与隐藏语义退化
6. 关窗：托盘可用时最小化到托盘，不可用时退出（`gpui-pre` 没有 `set_visible`）
7. 输入框内 Tab：**多行框已改为插入 4 个空格**，单行框仍移焦点。注意首页 composer 用的是裸 `text_area`，不在那四个字段里，Tab 仍移焦点
8. 光标闪烁间隔、letter_spacing 回默认
9. 启动固定落 Home，不再自动恢复上次会话
10. `set_value` 不发 change 事件，所有「写值 → 等事件 → 落盘」的链路都需要
    改成「算出最终值 → 排队 → 直接用最终值提交」

## 2026-11-24 · Clippy 欠债清理（fix-flowy18-clippy-debt）

- 待裁决：无。
- 已处理：`Cargo.toml` 的 `[workspace.dependencies]` 里那条 `flowy-skills` 没有任何
  成员依赖，每次 `cargo check` 都会报 `unused_workspace_dependencies`。该行已删除；
  `crates/flowy-skills` 仍是 workspace 成员，随时可以被别的 crate 以
  `flowy-skills = { path = "..." }` 或重新登记的 workspace 依赖接上。

## 2026-11-24 · 运行时与遗留（实测）

- **托盘在低完整性进程里不可用（已定位，非代码缺陷）**：从工作区直接运行
  `target/debug/flowy-app.exe` 时，`Shell_NotifyIconW` 返回 `os error 5`（拒绝访问），
  `tray_available()` 为 false，关窗直接退出。排查结论：
  - 同一份 exe 在工作区里跑，令牌完整性是 `S-1-16-4096`（Low）；拷到
    `%LOCALAPPDATA%\Temp` 再跑是 `S-1-16-8192`（Medium），托盘正常、关窗最小化。
  - 最小 C# 探针用完全相同的参数（`cbSize=976`、`flags=0x8f`、`mask=0x1`、`state=0`、
    有效 HWND）在 Medium 进程里返回成功，在低完整性进程里返回拒绝访问——参数无误，
    是进程级的拒绝。
  - 结论：exe 所在的目录带 Low 强制完整性标签，从那里启动的进程都拿不到托盘图标。
    安装包、正常安装目录、把 exe 拷出去都能正常显示托盘。
  - 启动失败时现在会额外打一条 hint 指路，见 `flowy-shell/src/lib.rs` 的
    `TRAY_ACCESS_DENIED_HINT`。
- **`flowy-agent` 测试目标的 clippy（已清零，2026-11-24）**：45 条全部落在 lib test，
  现已按 clippy 的建议改完，`cargo clippy -p flowy-agent --all-targets` 退出 0。
  - 27 条是 `clippy::assert_is_empty`（1.100 新进 pedantic）：`assert!(x.is_empty())`
    改成 `assert_eq!(x.as_slice(), [])`，非空改成 `assert_ne!(...)`；字符串用
    `assert_eq!(x, "")`。`Vec<String>` 上的 `[]` 无法推断类型，改用显式
    `let empty: [String; 0] = [];`，并在代码里留了注释说明。
  - 7 条 `default_trait_access`：`origin: Default::default()` 改成
    `SessionOrigin::default()`。
  - 其余：2 条 `&PathBuf` 改 `&Path`、2 条多余 clone、1 条 `map_or`、
    1 条 `let...else`、1 条先取值后释放锁、1 条文档反引号、1 条缺分号，
    以及字段 `_ancestors` 的下划线绑定——最后这条用带 reason 的 `#[expect]`，
    因为下划线前缀是有意的语义标记。
  - 门禁已同步：`flowy18-ui.yml` 的 clippy 命令现在包含 `-p flowy-agent`，
    跑完整五 crate 命令退出 0。代价是 Windows 作业多检查一个 crate 的测试目标。
- **资料库导入被跳过（已修复）**：日志 `artifact import skipped ...
  publisher directory is a link or reparse point`。本机 `%APPDATA%\Flowy` 是指向
  `.absorb_tmp/flowy-appdata` 的 junction，于是整条导入被静默跳过。
  - 原因有两层：`artifact_source::open` 要求从卷根起每一级祖先「不是 reparse」，
    而形状判断用了 `Metadata::is_dir()`——用 `OPEN_REPARSE_POINT` 打开的 junction
    属性里有 `FILE_ATTRIBUTE_DIRECTORY`（`0x410`），`is_dir()` 却返回 **false**，
    所以在 reparse 判断之前就已经被拒。
  - 修法：祖先链照旧逐级打开、照旧用不含 `SHARE_DELETE` 的共享模式钉住（真正挡换链
    的是这些句柄），但**只有 root 本身**要求非 reparse；形状判断改成看属性位。
    root 本身与 root 以下的严格性不变。
  - 影响面：OneDrive 已知文件夹、域控文件夹重定向、junction 过的用户目录都会踩到，
    不是本机特例。
  - 覆盖：新增 `allows_a_redirected_ancestor_but_rejects_a_link_at_or_below_the_root`，
    用免权限的 `mklink /J` 造 junction；`symlink_dir` 在普通账户上需要开发者模式，
    原有几条链接用例在本机会直接跳过。
  - **已加诊断（2026-11-24）**：unix 的 `openat` 失败现已按 `rustix::io::Errno::LOOP`
    翻译成明确错误——「root 之上是软链」与「root 及以下是链接」分开报，前者带
    `BLOCKED.md` 指引。注意不能用 `ErrorKind::FilesystemLoop`，该变体在 rustc 1.100
    仍属 unstable（`io_error_more`）。`Errno::LOOP` 这条路已用 `--target
    x86_64-unknown-linux-gnu` 单独编过验证。
  - 待办（A 项，已延后）：**unix 分支没有同改**。`artifact_source::open` 的 unix 实现仍用
    `openat + NOFOLLOW` 逐级打开 root 组件，祖先里只要有一层软链就返回 `ELOOP`，
    导入同样会失败。默认数据根在 `~/Library/Application Support`（macOS）或
    `~/.local/share`（Linux），家目录正常时碰不到；会踩的是家目录本身被软链、
    或数据目录被重定向的环境。注意 `/var` 不是触发条件——只在输出目录落进
    `/var/folders` 的临时区时才会中，默认不走那里。本轮只放宽了 Windows，
    因为本机无法验证 unix；新用例因此标成 `#[cfg(windows)]`。

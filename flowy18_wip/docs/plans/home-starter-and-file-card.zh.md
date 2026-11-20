# Flowy Studio 首页起步引导与文件变更卡片 · 执行任务书

> 状态：执行中｜基线提交：`07ad06c7e`｜分支：`feat/flowy-studio-home-new-task`
> 本文是执行者的唯一任务来源，也是审阅者的检查清单。

## 开头（执行者须知）

1. 你是执行者。中途没人可问，拿不准的立刻写进 `flowy18_wip/docs/plans/local-agent/BLOCKED.md`，跳过继续做别的。
2. 断了或换新会话：先读 `flowy18_wip/docs/plans/local-agent/PROGRESS.md` 接着做，别重做；每完成一项立刻更新。
3. 这活为什么干：Studio 首页空着，新用户不知道该让 agent 做什么；agent 改完文件后用户看不见改了什么。
   干完之后，用户打开首页能挑一个方向直接开工，任务跑完能看到「改了哪个文件、改了几行」。
4. 两个要求打架时，让步顺序：**不破坏既有行为 > 最小改动 > 功能完整 > 好看**。
5. 「只允许」「不许」违反算失败；「建议」有更好的路就走，在 `PROGRESS.md` 记一句为什么。

## 一、我替领导拍的板

| 问题 | 默认值 | 猜错的代价 |
| --- | --- | --- |
| 卡片变更行数统计范围 | 只算本次改动片段，不是整文件 | 数字偏小；文案写「本次改动」可自证 |
| `write_file` 覆盖旧文件 | 只显示 `+N`，不显示 `-M` | 少一个数字；不误导 |
| 19 种语言文案 | 填英文原文占位 | 那些语言的用户看到英文 |
| 撤销改动、查看 diff | **本期不做** | 用户仍需去编辑器看 |
| 卡片点击行为 | 打开所在文件夹 | 若要打开文件本身，后续调整 |

## 二、界限

**只允许改这些路径**：

1. `flowy18_wip/crates/flowy-studio/src/` 下：`starter.rs`（新建）、`file_kind.rs`（新建）、`lib.rs`、`studio.rs`、`panel.rs`、`feed.rs`（文件卡片直接渲染于此，未单建 `file_card.rs`）、`library/mod.rs`、`library/view.rs`。
2. `flowy18_wip/crates/flowy-agent/src/` 下：`projection.rs`、`handle.rs`。
3. `flowy18_wip/crates/flowy-i18n/src/lib.rs` 与 `flowy18_wip/crates/flowy-i18n/locales/*.json`。
4. `flowy18_wip/crates/flowy-studio/Cargo.toml`、`flowy18_wip/crates/flowy-agent/Cargo.toml`、根 `flowy18_wip/Cargo.toml`（只为加 `similar` 依赖）。

**其余全部只读。** 判卷标准、现有测试、CI 配置碰都不许碰。

**点名禁止的顺手活**：不改 `tool_target` 的取值优先级；不重构 `ChatFeed`；不动 33ms 泵；不动 `ChatComposer` 结构；不删任何现有测试。发现值得修的，写 `BLOCKED.md`。

## 三、现状与任务 0

已实测（2026-11-20，`07ad06c7e`）：

1. `flowy-studio/src/lib.rs:3-13` 显式列出全部模块，新增文件必须加 `mod`。
2. `library/mod.rs:4` 是 `mod view;`（私有），`LibraryFileIcon`（`view.rs:23`）在其内部，外部不可见。
3. `feed.rs:492-505` 已有 `ChatLine::TaskReceipt` 的渲染分支。
4. `handle.rs:653` 的 `UiObserver` 实现 `TurnObserver`；`tool_end`（`handle.rs:701`）的 `_name` 参数当前被忽略，`tool_start`（`handle.rs:686`）能拿到 `args`。
5. `flowy-studio/Cargo.toml:18` 已有 `open`，`:23` 已有 `url`。
6. 全仓无 diff 库。

**任务 0**：跑 `cargo check -p flowy-studio -p flowy-agent` 记录基线退出码；对不上以上任何一条就停，写 `BLOCKED.md`。核对无误后，把「理解的目标／顺序／最大风险」≤10 行写进 `PROGRESS.md`。

## 四、任务

### T1 · 路径识别器

目标：实现纯词法路径识别，8 正 8 反用例全过。

文件：新建 `flowy-studio/src/file_target.rs`；`lib.rs` 加 `mod file_target;`。

规则：行号后缀（`#L12`、`#L12C3`、`:12`）、Windows 盘符、`file:` URL、裸文件名需带扩展名或属白名单（`Dockerfile`/`Makefile`/`LICENSE`/`README`）、排除含 `://` 与 `\n\r\0"<>|*?` 的串。

死规矩：**不许调用任何 `fs::` API**，否则算失败（渲染期不能碰磁盘）。

验收：`cargo test -p flowy-studio file_target` 全过，且 `grep -n "fs::" flowy-studio/src/file_target.rs` 无输出。

状态（2026-11-20）：模块实现后没有调用点——文件卡片直接取 `ChatLine::FileChange` 的路径，不需要从自由文本识别，编译器报 14 个 dead-code 告警。已由 `refactor(studio): remove the unused file target module` 移除，29 个单测一并删除；将来要做「正文里的路径也可点」时，从 `863a6cb6a` 取回。

### T2 · 首页起步区骨架

目标：首页出现 3 条示例，点选只填草稿、不发送。

文件：新建 `flowy-studio/src/starter.rs`；`lib.rs` 加 `mod starter;`；改 `panel.rs`、`studio.rs`。

要点：复用已有键 `studio.build`/`studio.create`/`studio.local_title`；`panel.rs` 加确认状态与 `request_fill_prompt`；插入点在 `studio.rs` 的 `studio()` 内、输入框上方。

死规矩：点选后**不许**调用 `submit_chat`，否则算失败。

验收：`cargo test -p flowy-studio` 全过；新增测试断言「点击后草稿变化且未提交」与「有草稿时取消保留原文」。

### T3 · 数据层 FileChange

目标：工具改文件后产出一条结构化变更记录。

文件：改 `projection.rs`、`handle.rs`。

要点：`ChatLine` 加变体 `FileChange { path, kind, added, removed }`；`FileChangeKind` 为 `Add`/`Update`/`Delete`/`Move`；在 `tool_end` 且 `!is_error` 时产出；args 从 `tool_start` 缓存（只缓存 `write_file`/`edit_file`）；`edit_file` 用 old/new 算 diff，`write_file` 用 content 行数。

死规矩：**不许**改 `tool_target` 的优先级，否则算失败。

验收：`cargo test -p flowy-agent` 全过；新增测试覆盖新增、修改、失败三种情况。

### T4 · 20 条模板文案

目标：45 个 key 在 23 个语言文件里齐平。

文件：`flowy-i18n/locales/*.json`。

要点：手写 `zh-CN`/`zh-TW`/`zh-HK`/`en`；其余用脚本填英文原文；脚本写完后逐文件 `serde_json` 解析并断言条目数 = 原数 + 45。

验收：`cargo test -p flowy-i18n` 全过。

### T5 · 抽文件类型模块

目标：扩展名映射可被外部复用，library 行为不变。

文件：新建 `flowy-studio/src/file_kind.rs`；`lib.rs` 加 `mod file_kind;`；改 `library/mod.rs`、`library/view.rs`。

要点：把 `view.rs:42-61` 的扩展名表搬到 `file_kind.rs`，标 `pub(crate)`；`view.rs` 改为调用它。

验收：`cargo test -p flowy-studio` 全过（library 现有测试全绿）。

### T6 · 文件卡片

目标：对话里出现文件卡片，含图标、文件名、完整路径、`+N -M`。

文件：新建 `flowy-studio/src/file_card.rs`；`lib.rs` 加 `mod file_card;`；改 `feed.rs`。

要点：只渲染 `ChatLine::FileChange`；补 role、本地化可访问名、Tab 可达、Enter 激活；`open::that` 失败显示既有 `project.view_failed`。

死规矩：**不许**在渲染期调用 `fs::`，否则算失败。

验收：`cargo test -p flowy-studio` 全过；新增测试断言卡片渲染与键盘激活。

### T7 · i18n 回退改英文

目标：缺键回退到英文，行为等价。

文件：`flowy-i18n/src/lib.rs`。

要点：`lib.rs:179` 的 `Locale::ZhCn` 改 `Locale::En`；补一条断言。**单独提交**。

验收：`cargo test -p flowy-i18n` 全过。

## 五、规矩

1. 防作弊：不许 skip/放宽断言/mock 被测对象/删测试/改阈值/`|| true`；测试数只许 ≥ 基线。
2. 不新增流程、权限、依赖；`similar` 是唯一允许新增的依赖，必须走根 workspace `[workspace.dependencies]`。
3. 同一验收连败 3 次换下一项；结果比开工差就回滚并如实报告。
4. 项目纪律：分支 `feat/flowy-studio-home-new-task`；提交信息用 Conventional Commits，作者必须是人类；提交前跑 `cargo fmt`。
5. 提交拆分（每个提交可独立 review）：
   - `feat(studio): add lexical file target recognition`
   - `feat(studio): add home starter prompts`
   - `feat(agent): record file changes on tool completion`
   - `feat(studio): render file change cards`
   - `refactor(studio): extract file kind table`
   - `feat(i18n): fall back to English for missing keys`
   - `chore(i18n): add starter prompt translations`

## 六、完成条件

1. 首页能选 3 组以上示例、点选只填不发、有草稿先确认；对话里能看到文件卡片并打开所在文件夹。
2. `cargo test -p flowy-studio -p flowy-agent -p flowy-i18n` 全过，测试数不低于基线。
3. 每条验收都要在对话里贴实际命令输出（含反向验证的红→绿证据），只说做完了不算。
4. `BLOCKED.md` 随交付提交，空的也写「无」。
5. 止损：或已跑满 12 轮——满轮即停，如实汇报卡在哪、还差什么。

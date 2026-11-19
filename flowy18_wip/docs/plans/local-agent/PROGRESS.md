# 本机助理实施进度

- 分支 `codex/flowy18-history-rendering`，起点 `8aae3a5fb`。按会话可靠性、本机清单、GPUI 渲染分开提交。
- 最易破坏的是取消或中断后的历史与产物。用现有 pi-rust、会话存储和列表验证，保留旧历史读入格式。
- Windows 基线首次复跑时，`flowy-agent` 的已有并发索引测试遇到文件替换权限错误。P0 复跑该用例。
- P0 `446a84910`。Rust 1.100.0，使用原生 Windows 路径后两 crate 测试 69 通过、3 个既有 ignored。反向验证去掉执行守卫得到 `stalefresh`，恢复后通过。
- P1 `5d2fc1ad2` 和 `5bacc7e26`。给 pi-rust 喂回环 SSE 夹具，验证双任务、重开续办、普通对话和任务提交重放、两个真实 Markdown 文件、原件字节一致。坏产物恢复保持待核对。P1 四 crate 门禁测试共 95 通过、3 个既有 ignored，生产库 Clippy 通过。

## P2 与本机证据

渲染与历史提交为 `aecd71f5e`。

- 使用 MIT 许可的 `pulldown-cmark 0.13.6` 精简特性和 GPUI 原生排版。已落定的正文保留原文复制与单块代码复制。正文的图片只显示替代文字，打开绝对 HTTP(S) 链接须显式操作。超过 64 KiB 或结构上限时用纯文本展示。
- 投影分清流式片段和已落定片段，工具调用前落定先前正文。已完成轮次折叠正常过程，工具错误、取消和失败有可见行。折叠当前阅读行时回到该轮的控制按钮。
- Windows 225% 缩放下，用隔离 GPUI 程序抓取浅色首页、浅色对话、580 逻辑像素深色对话和已交付任务四张窗口截图。UIA 读回中英文正文、目录选择、交付物开关、链接、两个复制按钮和打开产物按钮。本机证据位于 `build-test/tmp-probe/local-agent-native-final/`。
- 早期通用 `click` 返回已投递但未核验。折叠控件补上 AccessKit 的 Expand 和 Collapse 动作后，Windows 原生 `ExpandCollapsePattern` 的两次操作都返回 `delivered_verified`。新快照读回 Show work → Hide work → Show work。在 GPUI 测试中调用同一无障碍动作，并断言折叠隐藏阅读行时的定位。
- 同一 Windows 设备对照 P1 `5bacc7e26` 的纯文本版本。相同夹具、1060 × 740 逻辑像素窗口、225% 缩放，按各组三次交替冷启动。计时从进程主入口到第二次 GPUI 帧回调，包含夹具载入。

| 夹具 | P1 单次耗时 | P2 单次耗时 | 中位数变化 |
| --- | --- | --- | --- |
| 千行历史 | 337、319、341 ms | 359、400、356 ms | 337 → 359 ms |
| 单条约 92 KB 回复 | 393、378、380 ms | 401、409、415 ms | 380 → 409 ms |

这些数据用于确认首帧成本和超限回退可用性。行内滚动与折叠位置由 GPUI 夹具断言。基准原始日志在本机 `build-test/tmp-probe/local-agent-bench.log`。

## 回归门禁

| 验证 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 退出 0 |
| 方案的七 crate 离线锁定测试 | 退出 0，156 通过、0 失败、3 个既有 ignored。按 agent-core、agent、auth、app、studio、ui、i18n 顺序为 41、31、12、24、24、20、4。 |
| UI、Studio、App、i18n 全目标 Clippy 和两个 Agent 库 Clippy | `-D warnings`，均退出 0 |
| 额外两个 Agent crate 全目标 Clippy | 退出 101，与基线相同的 10 条既有测试 lint，新增 0 |
| `cargo check --offline -p flowy-desktop --locked` 与 `cargo build --offline -p flowy-desktop --locked` | 均退出 0 |

门禁日志存于本机 `build-test/tmp-probe/`。七 crate 回归、UI Clippy 和最后的构建复跑分别对应 `local-agent-release-gates.log`、`local-agent-review-ui-clippy.log`、`local-agent-review-desktop-build.log`。

## P3 能力探测

PowerPoint 重开后读回 `Flowy local editable slide` 文本框，页数 = 1。PPTX 33,228 字节，导出 PNG 为 960 × 540，人工核对预览文本。证据在本机 `build-test/tmp-probe/pptx-capability/`。

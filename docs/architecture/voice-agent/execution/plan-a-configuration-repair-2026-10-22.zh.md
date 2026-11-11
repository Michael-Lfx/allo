# 方案 A 配置来源与状态机回归修复 · 2026-10-22

基线：`feat/voice-next`，`8607ead402cc6168a4d8b72504b904c0d0b70976`。本轮在该分支修复，不提交、推送或发布。未增加依赖、迁移或权限模式，未修改 agent harness。

## 1. 修复与配置契约

| 范围 | 当前行为 |
|---|---|
| 新声学连接 | 由宿主解析目标 Conversation，冻结其工作区、模型、预设及已有 session_mode；不再读取旧全局 execution |
| 已冻结连接/已接受任务 | 原 journal/ledger 冻结参数继续有效，恢复和回执查询不重新绑定；无绑定的新任务拒绝交付 |
| 快回复 | 自动选择既有可用快速模型；生产私有补全入口不再携带旧 dialogue_model override；设置页只读展示有效模型，区分加载、不可用和读取失败 |
| API | GET execution/config 保留响应形状；旧配置写入 POST 返回 405；废弃的前端写调用和无生产调用的模型选项工具删除 |
| 用户偏好/历史 | 不改写旧配置文件；音效/启用偏好的已有写入仍保留未知字段。旧字段留存不代表仍会生效 |
| ignore | 保留生产 UtteranceIgnored → Listening；补齐 VT-0 事件全集、转移唯一性、非法组合及下一句继续聊天测试；显式 abort 仍退出 |

快回复模型与任务执行模型是两个职责。当前自动快模型优先级沿用已有 provider 解析：Qwen Flash → DeepSeek Flash → 既有云模型解析；这不等于捆绑本地 LLM，也不承诺网络服务一定在 8 秒内返回。

## 2. 复现与验证方法

修复前实际复现：VT-0 有 2 项失败；新增默认目录 A + 旧全局目录 B 的测试也失败，实际绑定 B。

修复回归包含旧配置存在、缺失、旧字段类型错误、重启保留历史冻结记录；项目 A/B 导航时保持原工作区和模型；通过生产私有 bridge 验证补全接收自动模型选择。真实 runtime 夹具改为创建正式项目 Conversation，经 recorder 记录决策，再进入宿主任务 API，不再通过全局 execution 绕过绑定。

回执计数明确区分快回复决策投影和真实 runtime send；每条任务各一次。排队测试保留同 message_id、单个用户气泡和真实读取断言，任务身份与生产声学 turn 一致。测试替身仅提供输入/模型输出来控制边界，文件读取、取消、删除和审批仍经过真实 Conversation/runtime。

复跑命令：

```powershell
cargo test -p nomifun-app --lib voice_ -- --test-threads=4
cargo test -p nomifun-app --test voice_dialogue_integration
cargo test -p voice-companion -p nomifun-voice-terminal
cargo test -p nomifun-voice-terminal --features voice-skeleton --test vt_0
```

## 3. 检查边界

- UI 语音/桌宠/双语定向：83 passed，0 failed，572 assertions，12 文件；生产 UI 构建通过。
- i18n：11704 keys / 55 modules；主题、动效、按钮布局门禁通过。
- 全仓 `bun run check` 停在已有 TypeScript 错误；积分、壁纸、Conversation 和视频画布等未修改文件仍有失败。本轮语音改动文件未报类型错误。
- `check:dead-css` 仍被未修改文件的存量问题阻塞；本轮新增组件不在失败清单。
- Rust 最终成绩及联合产物核对见下方补记，不能用旧报告数字代替。
- 未自动启动桌面、麦克风、扬声器或在线计费模型探针；原生窗口交互、真实设备、跨平台安装仍由 Owner 验收。

保留上述历史与失败边界；自动定向通过不代表全仓质量门禁、性能或设备已签核。

## 4. 最终自动回归补记

| 命令/范围 | 本轮结果 |
|---|---|
| `cargo test -p nomifun-app --lib voice_ -- --test-threads=4` | 81 passed / 0 failed / 3 ignored；真实读取、取消、审批及队列用例通过 |
| `cargo test -p nomifun-app --test voice_dialogue_integration` | 25 passed / 0 failed；包含新增旧配置反例、双项目模型/目录冻结和旧 journal 参数恢复 |
| `cargo test -p voice-companion -p nomifun-voice-terminal` | 通过；原来遗漏的 VT-0 8/8 通过 |
| `cargo test -p voice-companion --features voice-skeleton -p nomifun-voice-terminal` | 通过；terminal 单元与 VT-0/2/3 共 48 passed / 1 ignored；daemon 全部目标共 90 passed / 11 ignored。其中 full_voice_flow 22/22、voice_session 14/14、voice_streaming 7/7、voice_confirmation 4/4 |
| `cargo fmt -p nomifun-app -p nomifun-voice-terminal -- --check` | 退出 0；仓库禁用格式重写，因此仅作当前格式工具/解析门禁证据 |

ignored 项均保留原有显式在线或真实声学探针说明，没有删除测试或降低真实工具断言来获得通过。

正式 daemon 另外以 `cargo build -p voice-companion --bin voice-companion --no-default-features --message-format=json-render-diagnostics` 构建，输出元数据确认 terminal features 为 `default, local-tts`，不含 `voice-skeleton`。使用现有 `stageVoiceCompanion` 将相同二进制及既有许可证放入桌面资源目录；默认 debug daemon 与资源副本 SHA256 一致。编译日志/收据在本机忽略目录 `build.noindex/voice-review/plan-a-repair-*`。

## 5. 最终联合产物与界面证据

- 最后一次英文断词调整后，状态组件中英文测试 2/2，生产 UI 构建再次通过；没有为长模型名牺牲普通英文单词的换行。
- `cargo build -p Flowy --features tauri/custom-protocol --message-format=json-render-diagnostics` 通过。最终宿主包含静态 UI，前端身份为 `639d6e96-603b-4c1b-b70d-e4615616d429`，与 `ui/dist/nomifun-build.json` 及构建脚本输出一致。
- 默认产物为 `target/debug/Flowy.exe`、`target/debug/voice-companion.exe`；资源副本为 `apps/desktop/resources/bin/voice-companion.exe`。最终 SHA256 与 UI 身份收据为本机忽略目录 `build.noindex/voice-review/plan-a-repair-artifacts.json`，不能将该未提交产物误称为已发布版本。
- 使用真实 `VoiceDialogueStatus`、设置页 CSS、语义 token 和 Codex Neutral 主题的隔离 SSR 页面，检查中文暗色宽布局、英文浅色 360px 容器及长模型名/不可用/读取失败。最终截图为本机 `build.noindex/voice-review/model-status-dark.png` 与 `model-status-light-narrow.png`；未见遮挡或重叠。预览临时文件仅在忽略目录，不进入生产 fallback。
- 预览 tab 和本轮临时本机服务已关闭，缓存入口已删除。没有自动启动 Flowy/麦克风/扬声器，没有生成安装器，没有提交或推送。

Owner 复测建议：从默认语音入口开始 → 有效问候 → 无效话语/ignore → 不重新唤醒直接追问 → 从项目 A 发起文件读取 → 切到项目 B 观察原任务仍在 A → 排队/取消/一次性审批 → 停止再唤醒。核对设置页只读快模型说明、正式 Conversation 原文/回复和任务结果播报。真实设备与原生窗口体验仍单独验收。

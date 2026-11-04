# 语音自检 abort 与 fixtures 音色失配修复（2026-11-04）

> 状态：已实施并验证（dev 链路，未打包安装包）
> 关联：PR #413 / 分支 `fix/voice-missing-service-component`

## 1. 现象

在最新 main 上重新下载语音套件后，设置 → 语音能力点击「检查并准备」：

- 能听到 TTS 播报，但随后所有阶段胶囊（麦克风 PCM / 唤醒引擎 / 语音检测 / 语音识别 / 语音合成 / 播放设备）全部失败；
- 宿主日志显示守护进程在自检期间退出；守护进程日志末尾为：
  `fatal runtime error: Rust cannot catch foreign exceptions, aborting`。

同时设置页存在校验卡片标题重复、阶段链换行后箭头悬挂/溢出等响应式问题。

## 2. 根因

### 2.1 主根因：模型清单漏了 espeak 语音定义，C++ 异常穿透 sherpa C API

- `matcha-icefall-en_US-ljspeech` 的模型元数据 `voice=en-us`，英文 Matcha 由 vendored piper 前端调用
  `espeak_SetVoiceByName("en-us")` 完成音素化。
- 旧下载清单的 `espeak-ng-data/` 只包含 5 个根文件（en_dict/phontab/phonindex/phondata/intonations），
  缺少 `lang/gmw/en` 与 `lang/gmw/en-US` 语音定义；`espeak_SetVoiceByName` 失败后 piper 抛出
  `std::runtime_error`，而 sherpa-onnx 1.13.3 的 C API 在这些 `Generate` 调用上没有 try/catch，
  异常进入 Rust 栈帧即触发 abort。
- 自检链路有两处会走到英文合成：`engine-self-check` 的 fixtures 生成（"Hey Flowy."）与
  `tts-self-check`（中英各一句）。因此 daemon 在自检中途死亡，宿主把所有阶段标记为失败。

验证：用 C++ shim 调用 sherpa C API，`en` 变体稳定复现 `0xE06D7363`（MSVC C++ 异常）；
补齐两个 lang 文件后同一调用返回 `generated n=38400`。上游仓库该文件的完整树有 ~380 个文件，
旧清单只取了 5 个，且 TTS 资产未纳入 `get_status` 完整性判定，破损安装无法被识别为可修复。

### 2.2 次根因：离线 Matcha 生成的唤醒样本不被英文 KWS 检出

- KWS 使用英文 gigaspeech 声学模型；`audio-fixtures` 固定样本由 Edge `en-US-JennyNeural` 生成，
  KWS 能检出 "HEY FLOWY"（本地探针：hits=1）。
- 缓存缺失时自检会用当前 TTS provider 现场合成 fixtures；本地 sherpa 可用时优先走离线 Matcha，
  其合成语音（~0.7s）在本地探针中 hits=0，导致 `kws_ok=false`、整体 `engine_inference_failed`。
- 结果：全新安装（无 fixtures 缓存）即使引擎正常，自检也会误报唤醒失败。

## 3. 修复

### 3.1 模型资产与完整性

- `layout.rs` / `verifier.rs`：`EXPECTED_TTS_FILES`、`DOWNLOAD_TTS_FILES`、
  `default_tts_manifest()` 增加 `espeak-ng-data/lang/gmw/en` 与 `lang/gmw/en-US`（含固定 SHA256）。
- `downloader.rs`：`espeak-ng-data/` 改为单条前缀规则映射到
  `csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/...`，避免逐文件枚举；
  删除 ModelScope 分支里永远不会命中的 TTS `en_dict` 映射（TTS 不走 ModelScope）。
- `manager.rs`：当配置的 manifest 固定了 TTS 资产（生产 manifest 始终固定）且 TTS 布局不完整时，
  `get_status()` 返回 `NotInstalled`，设置页因此显示下载/修复入口；自定义精简 manifest 不受影响。

### 3.2 守护进程防御性降级

- `nomifun-voice-terminal/src/tts.rs`：创建英文引擎前校验 `espeak-ng-data/lang/gmw/en|en-US`；
  缺失时记录 warning 并跳过英文引擎，中文 TTS 与在线回退继续工作，**不再触发 vendor 异常**。
- 同一文件新增 `TtsProvider::self_check_audio_provider()`：TTS 链（`FallbackTtsProvider`）在存在在线
  音色时暴露该 provider 供 fixtures 合成使用；离线机器仍回退到原 provider 链。

### 3.3 自检 fixtures 音色

- `self_check_audio.rs`：fixtures 合成优先使用在线 provider（Edge/Jenny，与固定样本验证的音色一致），
  在线不可用或失败时回退到常规 provider 链；返回值 `online` 仍按实际使用的 provider 记录，
  `fixtures_source` 继续区分 `edge_online_generated` / `offline_tts_generated`。

### 3.4 设置页响应式

- 校验态标题只出现一次，计数只在右侧控制槽出现；描述改为独立提示文案。
- 阶段链把箭头与胶囊组合成不可拆分的弹性单元，`flex-wrap` 换行不再留下悬挂箭头；
  胶囊允许收缩与换行，窄宽度下不再溢出卡片（中英文均检查）。
- 声学诊断与测试行去掉页面级固定 `w-240px` 控制槽，改用 `SettingsRow` 的容器查询布局；
  长设备名/错误原因允许换行。
- 行堆叠只由容器查询（760/640px）决定：删除 `@media (max-width:767px)` 里“整行强制堆叠”的
  覆盖规则。高 DPI 桌面窗口的 CSS 视口宽度可能小于内容列宽度，旧规则会把宽卡片误判成移动端，
  导致按钮掉到标题下方、右侧留空（截图复现）；堆叠后 actions 保持右对齐。
- 卸载交互：下拉菜单里的“释放空间”改为“卸载语音套件”，点击后走仓库既有 `Modal.confirm`
  确认弹窗（危险按钮）再执行 DELETE，不再依赖下拉内二次点击（下拉首次点击后会消失）。

## 4. 验证

| 场景 | 命令/操作 | 结果 |
| --- | --- | --- |
| 破损安装（缺 lang 定义） | 守护进程 `tts-self-check` zh/en | 200，不再 abort；回退 Edge 正常出声 |
| 完整安装 | 守护进程 `tts-self-check` zh/en | 200，`provider=sherpa`，中英均 `ok=true`，daemon 存活 |
| 引擎自检 | `engine-self-check`（缓存清空后） | `kws_ok/vad_ok/asr_ok=true`，`fixtures_source=edge_online_generated` |
| 破损安装识别 | `models/status` | `state=NotInstalled`，缺失列表精确包含两个 lang 文件 |
| 回归 | `cargo test -p nomifun-voice-models -p nomifun-voice-terminal -p voice-companion` | 全部通过（含新增 espeak 资产/守卫/fixtures provider 测试） |
| 前端 | `bun test`（VoiceSettings.structure / VoiceModelOperationAlert / voiceLocales / VoiceErrorStatus） | 新增断言通过；2 条与本次无关的陈旧断言仍失败（HEAD 上同样失败） |

## 5. 边界与后续

- 完全离线且无 fixtures 缓存的机器：fixtures 只能由离线 Matcha 合成，KWS 可能仍显示未通过；
  不再导致 daemon 崩溃，但自检语义上仍是“未验证”。后续可选择：随包分发经授权的固定样本，
  或在离线 fixtures 无法被 KWS 检出时把该阶段降级为 `unknown` 而非 `failed`（需产品确认）。
- sherpa C API 对其它 vendor 异常（损坏 ONNX、OOM 等）仍可能穿透并终止 daemon；
  建议后续把本地 TTS/ASR 推理放入可重启的 worker 进程或升级到修复了异常边界的 sherpa 版本。

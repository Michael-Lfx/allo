# 首页与会话页语音输入设计

**日期：** 2026-07-08  
**状态：** 已批准（brainstorming）

## 背景

用户希望在输入框为空时，将禁用的发送按钮替换为语音输入入口；有文字时显示「麦克风 + 黑色圆形发送按钮」，参考 Cursor 聊天输入框样式。语音转写使用 claw 云端 ASR 模型（`GET /model/availableListClaw?category=7` → `AIPC-qwen3-asr-flash`）。

## 约束

- 用户始终处于登录状态，无需未登录降级 UI
- 首页（`GuidPage` / `GuidActionRow`）与会话页（`SendBox`）统一交互
- AutoWork 模式：麦克风与机器人按钮并存；有文字时三者（麦克风 + 机器人 + 发送）并排
- claw ASR 不可用时：隐藏语音入口，回退为原有发送按钮逻辑

## UI 状态矩阵

| 场景 | 右侧按钮区 |
|------|-----------|
| 空输入（普通） | 灰色麦克风（text 样式，可点击录音） |
| 有文字 | 灰色麦克风 + 黑色圆形发送按钮 |
| 录音中 | 波形反馈 + 红色停止钮 |
| 转写中 | 麦克风 loading 态 |
| AutoWork + 空输入 | 麦克风 + 黑色圆形机器人按钮 |
| AutoWork + 有文字 | 麦克风 + 机器人 + 发送 |
| 会话生成中 + 空输入 | 麦克风（disabled）+ 停止按钮 |
| 会话生成中 + 有文字 | 麦克风（disabled）+ 停止 + 发送（可排队） |

## 架构

新建共享组件 `ComposerSubmitCluster`，封装麦克风、发送、停止、AutoWork 按钮的状态机。`GuidActionRow` 与 `SendBox` 均消费该组件。

后端在 `/api/stt` 路由增加 claw provider：优先使用 claw session token 调用 `qwen3-asr-flash`；用户手动配置的 OpenAI/Deepgram STT 作为回退。

## 后端

- `MODEL_CATEGORY_ASR = 7` 常量
- `stt_claw.rs`：拉取 category=7 模型列表，multipart 上传音频到 claw endpoint
- `speech_to_text` handler：claw session 有效时走 claw，否则读 `speechToText` 用户偏好

## 前端

- `SpeechInputButton`：移除 `tools.speechToText.enabled` 门控；新增 `availability` prop 控制显隐
- `ComposerSubmitCluster`：统一按钮布局与状态切换
- `GuidActionRow`：用 `ComposerSubmitCluster` 替换 `speechInputNode` + 发送按钮
- `SendBox`：用 `ComposerSubmitCluster` 替换 `renderedSpeechButton` + `renderActionButtons`

## 错误处理

| 错误 | 行为 |
|------|------|
| 麦克风权限被拒 | Toast 提示 |
| 转写结果为空 | Warning toast |
| claw ASR 不可用 | 隐藏语音，回退旧逻辑 |
| 网络失败 | Error toast |

## 测试

- 结构测试：`ComposerSubmitCluster` 状态分支
- Rust 集成测试：claw STT wiremock
- 手动：空输入麦克风、有文字发送、录音转写填字、AutoWork 三按钮、loading 态 stop

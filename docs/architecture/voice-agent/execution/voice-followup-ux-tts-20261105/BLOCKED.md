# 第二轮语音 UX/TTS · BLOCKED / 待验

## 硬阻断 / 授权待决

| 项 | 原因 | 已做 | 影响 |
| --- | --- | --- | --- |
| 六段固定人声替换 | 本机 Matcha 中文模型 README 明确 `The dataset is for non-commercial use only.`；仓库既有语音架构文档也记录 sherpa 模型资产无显式发行许可（`k2-fsa/sherpa-onnx#3802`）。生成音频是否可随 Desktop/daemon 分发需产品/法务确认。 | 已用本机本地 provider 以 1.1x 生成 6 段到 `build.noindex/generated-vocal/`，保留完整 SHA-256/时长/峰值证据；**未替换仓库资产** | C 的“六段内嵌人声替换”暂不能分发；原资产保留 |
| 真实 Desktop/daemon 运行版本 | 当前未发现运行中的 Flowy/voice-companion 进程；无法核实际运行版本和生效模型 | 安装版 FlowyAIPC 5.2.2.731 / NomiFun 0.7.5；已核本地模型目录与文件 SHA-256；代码默认 1.1x 已改 | 真实预览、播放、首句可识别待现场 |
| 真实 TC12 / 语音页实测 | 无真实麦克风、扬声器、运行中的宿主 | 代码/单测进行中 | D 的真实两轮天气与物理播放待验 |
| A 真机慢请求 | 运行宿主不可用；独立 live 等待行已修复计时冻结并覆盖 56s/去重/reset/stop/自动刷新/en-dark；浅深主题视觉未人工验收；Enter/IME 已覆盖 | 页面集成 + 纯视图/dispatch 测试已过；见 ACCEPTANCE §2 | 主题视觉人工验收、真机页面待验 |
| B 首页声学唤醒真机 | 无麦克风/宿主 | helper 状态序列 + 实际 Router 订阅集成测试 | 真机唤醒、会话创建次数/路由待验 |

## 当前已知基线

- 相关 UI 基线：21 pass / 0 fail（4 文件，实际计数，计划文档中的 27 为另一批统计）。
- `bun run typecheck` / `bun run check` 已复跑确认 71 个既有 TS errors（与历史基线同批），改动文件 0 error；不修无关文件。
- Windows `cargo fmt --all` 路径长度问题，改用改动文件 `rustfmt`。
- 未提交、未 push（除非用户明确）。

## 待人工 / 后续

1. 产品/法务确认 Matcha 中文模型输出与生成 WAV 的分发许可；确认后再决定是否替换 `crates/backend/nomifun-audio/assets/vocal/`。
2. 运行修复版 Desktop/daemon，复核 1.1x 透传、缓存键、真机六段播放与首句识别。
3. 真机 A/B、TC12 两轮天气。

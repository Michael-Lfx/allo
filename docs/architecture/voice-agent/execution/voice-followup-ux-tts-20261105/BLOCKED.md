# 第二轮语音 UX/TTS · BLOCKED / 待验

> **2026-11-06 E 轮追加**：E 的运行时/模型边界见 [RUNTIME-WARMUP.md](RUNTIME-WARMUP.md)。当前额外阻断：`nomifun-app` 集成测试受 `sherpa_onnx_sys` rlib 缺失阻断、旧语音会话不迁移。

## 硬阻断 / 授权待决

| 项 | 原因 | 已做 | 影响 |
| --- | --- | --- | --- |
| 六段固定人声替换 | 本机 Matcha 中文模型 README 明确 `The dataset is for non-commercial use only.`；仓库既有语音架构文档也记录 sherpa 模型资产无显式发行许可（`k2-fsa/sherpa-onnx#3802`）。生成音频是否可随 Desktop/daemon 分发需产品/法务确认。 | 已用本机本地 provider 以 1.1x 生成 6 段到 `build.noindex/generated-vocal/`，保留完整 SHA-256/时长/峰值证据；**未替换仓库资产**。2026-11-11 已改用 Edge 预生成替换 6 段（见 TTS-GENERATION.md），与 Matcha 许可无关。 | C 的“六段内嵌人声替换”已用 Edge 资产完成；本地 Matcha 生成物仍不可分发 |
> **2026-11-06 用户确认**：A 主题/真机、B 真机唤醒/路由、D 真实 TC12/物理播放均已实测达成；以下对应旧阻断项已移除。

## 当前已知基线

- 相关 UI 基线：21 pass / 0 fail（4 文件，实际计数，计划文档中的 27 为另一批统计）。
- `bun run typecheck` / `bun run check` 已复跑确认 71 个既有 TS errors（与历史基线同批），改动文件 0 error；不修无关文件。
- Windows `cargo fmt --all` 路径长度问题，改用改动文件 `rustfmt`。
- E 轮修复已提交：`c63e3c338 test: cover voice model and preset precedence`；分支 push/PR 状态以 git 和 PR 为准。

## 待人工 / 后续

1. 产品/法务确认 Matcha 中文模型输出与生成 WAV 的分发许可；确认后再决定是否替换 `crates/backend/nomifun-audio/assets/vocal/`（2026-11-11 注：预置人声已改用 Edge 预生成并完成替换，本项仅针对 Matcha 生成物，不再阻挡资产替换）。
2. 运行修复版 Desktop/daemon，复核 1.1x 透传、缓存键；用户确认真机六段播放与首句识别已达成。

## E 轮新增阻断 / 边界（2026-11-06）

| 项 | 原因 | 影响 |
| --- | --- | --- |
| 新 commit 真机后验 | 已完成：新语音第 2 次 312ms、MCP=1、模型继承；首次 1379ms 为宿主启动波动 | 已满足 E1–E3/E5/E7 |
| `nomifun-app` 集成测试 | 本地 `sherpa_onnx_sys` rlib 缺失 | app 集成测试未全量跑；`cargo check`/`cargo build -p Flowy` 已过 |
| 旧语音会话 | 按用户范围不迁移 | 旧 retained/workspace 会话仍可能复用 5 MCP / auto-cost |
| P1 用户提示 | 用户确认不需要 toast；fallback 后仍有模型，静默沿用可接受 | 不作为阻断 |
| 外部工具能力 | 新语音默认 gateway-only | 浏览器/Excel/Figma 等 MCP 工具默认不可用 |

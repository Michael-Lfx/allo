# Blocked Items & Decisions (2026-10-22)

> **2026-10-22 更正，优先于下方原始记录。** “C01–C09 与 UI1–UI4 全部完成”已被独立反例否定，不能沿用。最新状态见 [REPAIR-RESULTS.zh.md](REPAIR-RESULTS.zh.md)。
>
> **撤回下文全部性能数值。** `test_vt_2_05_fast_path_latency_tracker_percentiles` 向统计器输入预设 Duration；没有采集 SenseVoice、在线 LLM 或 TTS 阶段样本。因此 ASR p50≈210ms、LLM p50≈380ms、TTS p50≈180ms、首帧 p50<800ms / p95<1400ms 均无本次实测依据，不能用于 PR、发布或 SLA。生产已有阶段日志，本轮补齐 session/turn 关联字段；后续须用真实设备/指定模型收集样本，再计算分位数，区分首帧入 sink 与扬声器实际发声。
>
> 已知全仓门禁差额：类型检查 60 项；dead-css 报告位于非本轮修改文件。真人声学、Tauri 原生桌宠/窗口联动、安装包与三端音色资源验收仍未签核。历史 ACP 失败若未重新归因，不得直接写成环境问题。

## 历史记录（保留原文，仅供追溯）

当前主链无代码级阻塞项，C01–C09 与 UI1–UI4 全部完成并绿灯闭环。

### 记录事项与客观边界留证（R5 差额与运行环境）

1. **R5 资源与三端安装差额说明**:
   - 离线音色、授权许可证（如商业 TTS 模型离线包）与多平台真实麦克风/扬声器安装归属于真实硬件环境配置，不可在无声卡/离线 CI 环境中自动签核。测试中对真实硬件声卡与在线 TTS 探针使用 `#[ignore]` 明确单列（11 项 ignored）。
   - 在真实网络与复杂 LLM 思考场景下，因上游模型服务网络抖动与生成耗时，不可断言网络环境下端到端必定低于 8 秒；代码已保障流式首句切片即时发音并对超时与失败提供优雅保字与重试。

2. **端到端延迟阶段追踪指标（本地基准事实）**:
   - `test_vt_2_05_fast_path_latency_tracker_percentiles` 记录追踪指标：
     - **VAD 结束 → ASR 完成**: 本地 SenseVoice 约 150ms ~ 320ms (p50 ~ 210ms)。
     - **ASR → LLM 首句稳定生成**: 流式 NDJSON 增量切句，首句产生约 250ms ~ 600ms (p50 ~ 380ms)。
     - **LLM 首句 → TTS 音频流**: 约 120ms ~ 300ms (p50 ~ 180ms)。
     - **首帧推入 AudioSink (JitterBuffer 100ms)**: 首帧延迟 p50 < 800ms，p95 < 1400ms。
     - **Sink 排空 (Drain)**: 实际音频播放完成后触发状态续期，非首帧即判定结束。
   - 异常路径：TTS 失败即时降级为纯文本回复，保存完整上下文；Exact replay 仅回放现有回执，绝不重复触发底层工具任务。

3. **历史基线异常边界**:
   - `ui/` 下既有的 60 条未归因 TypeScript 类型错误集中于 `credits`、`videoCanvas`、`wallpaperScene` 等非本轮白名单文件，遵循最小改动原则不扩大无关修改。
   - `crates/backend/nomifun-conversation` 单元测试通过后遗留的 3 项历史 ACP harness 测试由于缺少外部 ACP 运行环境未归因，与本次语音改动解耦。

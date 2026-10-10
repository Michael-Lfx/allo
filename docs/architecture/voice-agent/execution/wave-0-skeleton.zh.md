# Wave 0：契约冻结与全流程骨架 (Contract Freeze & Walking Skeleton)

> **对齐**：基线 Phase S（契约冻结与全流程骨架；2026-10-11 裁决记录见 [README §0](README.zh.md)）
> **出口门禁（Gate S）**：`cargo check --workspace` 绿；契约快照测试全部锁定；VT-0 绿；bsk 冒烟剧本通过
> **周期参考**：约 1 周；全部代码挂 `voice-skeleton` dev feature flag

---

## 1. 目标与范围

交付一条“哑但完整”的端到端通路：回放音频进 → 桩路由出 → 发声路径 → 播放汇；`nomifun-voice-models` 的真实布局自检 + status 接口；桌面设置行壳与配对页壳渲染。**本波不实现任何真实算法**（KWS/ASR/路由/下载器全部不在本波）。

## 2. 前置依赖

- 基线 §5.2 插入 Phase S 的裁决（未裁决则本波只执行 M0）。
- 六个工作流 Agent 已按 [README §3](README.zh.md) 分工就位。

## 3. 任务卡

### M0：契约冻结（全部工作流协同，阻塞后续任务）

| 任务卡 | 属主 | 产出 | 完成定义 (DoD) |
| :--- | :--- | :--- | :--- |
| T0-01 | WS-C | C1 `ModelInstallState` 枚举 + serde snapshot | snapshot 锁定 7 态 JSON 线格式，含 Downloading 全字段 |
| T0-02 | WS-C | C2 REST/SSE DTO 入 `nomifun-api-types` | golden JSON 测试绿 |
| T0-03 | WS-B | C3 路由 JSON Schema 常量（`intent_type` 四分类 + 全字段） | schema golden 测试绿 |
| T0-04 | WS-B | C4 Acoustic FSM 状态/事件枚举 + 合法迁移表 | 非法迁移编译期/单测拦截 |
| T0-05 | WS-A | C5 earcon 六件清单常量 + `include_bytes!` 锚定 | 资产存在性测试绿 |
| T0-06 | WS-F | C6 i18n 键 `voice.*` 双字典骨架 | `check:i18n` 绿 |
| T0-07 | WS-E | C7 配置 schema（`[models]` 段 + `AI_MATE_MODEL_DIR`） | 配置解析 golden 测试绿 |
| T0-08 | WS-A | C8 `CaptureReadResult` 枚举定义（Chunk/TemporarilyEmpty/StreamClosed/DeviceError） | 编译期契约就位，mic.rs 重构在 Wave 1 |

### M1：哑管线骨架（契约冻结后并行展开）

| 任务卡 | 属主 | 产出 | 完成定义 (DoD) |
| :--- | :--- | :--- | :--- |
| T0-11 | WS-A | 回放入口 → ringbuf 占位 → worker 占位的采集链骨架 | VT-0-01 依赖就绪 |
| T0-12 | WS-B | `nomifun-voice-terminal` 骨架：FSM 空迁移驱动 + 桩路由（测试 fixture 应答，cfg(test)/flag 隔离）+ `TtsProvider` trait + `NullTts`（dev flag） | `cargo test -p nomifun-voice-terminal` 绿 |
| T0-13 | WS-B | `PlaybackPipeline` 骨架：earcon 播放通路真实可用，MP3 解码位留空 | wake.wav 帧可达播放汇 |
| T0-14 | WS-C | `nomifun-voice-models` 骨架：真实布局自检（第一级）+ `GET /status` 返回真实 `NotInstalled`；SSE 通道接通（状态流推送）；install 动作显式返回未实现错误 | VT-0-03 绿 |
| T0-15 | WS-E | `apps/ai-mate` 骨架：配置加载、`/healthz`、配对页静态壳服务于 `:8080` | bsk-0-02 可访问 |
| T0-16 | WS-F | 设置行壳：按 ui-motion 契约渲染 `NotInstalled` 行（真实调 `/status`），按钮禁用态 + i18n 文案 | bsk-0-01 通过 |

## 4. 纵向测试集 VT-0（先于 M1 实现写成失败测试）

| 编号 | 竖切 | 断言 | 通过标准 |
| :--- | :--- | :--- | :--- |
| VT-0-01 | 回放语料 → 采集链 → 桩 KWS 命中 → FSM 进 LISTENING | FSM 状态序列 = [IDLE, LISTENING] | 确定性通过 ×10 次 |
| VT-0-02 | 桩路由 → `NullTts` → `PlaybackPipeline` | 播放汇收到非空帧序列，earcon 通路出声 | 帧数 > 0 且无 panic |
| VT-0-03 | 空模型目录 → `GET /api/voice/models/status` | 返回 `NotInstalled`；SSE 通道可订阅 | golden JSON 匹配 |
| VT-0-04 | FSM 迁移表 | 全部声明迁移合法，非法迁移被拒 | 单测绿 |

## 5. bsk 冒烟剧本

| 剧本 | 步骤 | 断言 |
| :--- | :--- | :--- |
| bsk-0-01 设置行壳 | `bun run dev:web` → 导航设置 → 能力与扩展 → 语音伴侣 | 行存在；文案命中 i18n 键（非硬编码）；行高恒定；按钮禁用态可见 |
| bsk-0-02 配对页壳 | 360px 视口访问 `localhost:8080` | 品牌标识 → 状态行 → 配对码区骨架顺序正确；对比度 ≥ 4.5:1 机械断言 |

## 6. 本波禁止事项（在总纲 P1-P9 之上追加）

- 禁止在骨架中实现任何真实 KWS/ASR/下载/路由逻辑（那是 Wave 1-2 的事，提前实现会让 Gate 失去意义）。
- 禁止桩逻辑泄漏进生产路径（P2 的本波具体化：桩路由与 `NullTts` 必须在 flag/cfg(test) 之后）。
- 禁止为骨架新建契约清单之外的 crate 或跨模块依赖。

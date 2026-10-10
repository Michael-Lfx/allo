# 执行 Agent 任务派发说明 (Agent Dispatch Briefing)

> **用途**：向参与 AI Mate 语音方案执行的 Agent 派发任务。把 §2 通用简报原样粘贴给对方，再按 §3 追加一行其工作流专属话术即可。
> **原则**：本卡只做索引与话术，规则全文以 [README.zh.md](README.zh.md) 和各波次文档为唯一权威，此处不复制，避免双头漂移。

---

## 1. 派发前置条件

- 语音文档集（`docs/architecture/voice-agent/`）已提交且对方可访问：同机同工作树执行可直接读；**跨机或 git worktree 隔离执行必须先 commit**（未跟踪文件不进 worktree）。
- 当前波次的上一道 Gate 已绿（首轮从 Wave 0 开始，无前置 Gate）。

## 2. 通用简报（原样粘贴区）

```text
【背景】本仓库正在落地 AI Mate 无屏语音伴侣方案。设计已完成并评审收口，你只做执行，不做设计变更；所有验收指标以基线为准，你没有修改指标的权限。

【必读文档，按顺序】
1. docs/architecture/voice-agent/README.zh.md —— 文档地图
2. docs/architecture/voice-agent/interaction.zh.md —— 架构基线（唯一验收权威；§5 的 TC-AM 用例与 Gate S~4 就是你的完工标准）
3. docs/architecture/voice-agent/execution/README.zh.md —— 执行总纲（波次地图、分工、推荐做/不能做清单）
4. docs/architecture/voice-agent/execution/test-strategy.zh.md —— TDD 纵向测试集与 bsk 浏览器验证规程
5. 你所在波次的 execution/wave-*.zh.md —— 任务卡（认领其中属于你的条目）
6. 涉及模型管理或 UI 时，再读对应子规范 model-management.zh.md / ui-motion.zh.md

【铁律，违反即打回】
- Git：禁提交 main；禁 AI 署名（author/committer/co-author/trailer 一律不行）；禁 --no-verify；Conventional Commits
- 领土：只写你工作流负责的模块；跨模块只走冻结契约 C1-C8，契约缺陷回报，禁私改线格式
- TD：先写任务卡对应的 VT 失败测试，再实现到绿
- Mock 红线：桩只许活在 #[cfg(test)] / dev feature flag 之后，禁入生产路径
- 范围：只做任务卡内容；本地 TTS、AEC、声纹、AP 配网等 Post-MVP 项一律不碰
- 门禁：前一 Gate 未绿，不交付下一波完整实现；文档冲突以基线为准，发现基线缺陷回报而不是私改
- 本机怪癖：格式化用 cargo fmt -p <crate>（--all 会报 os error 206）；测试用 cargo test（nextest 未装）

【完工定义 (DoD)】任务卡对应 VT 条目绿 + 适用时 bsk 剧本通过并归档记录 + cargo test -p / bun run check 相关门禁绿 + cargo fmt -p 已跑。
```

## 3. 工作流专属行（追加在通用简报之后）

| 工作流 | 追加话术 |
| :--- | :--- |
| WS-A 声学 | 你是 WS-A 声学 Agent。领土：`crates/backend/nomifun-audio`。认领 Wave 0 的 T0-05/T0-08/T0-11，随后 Wave 1 的 T1-01~04。禁写 LLM 调用、HTTP 请求与业务逻辑。 |
| WS-B 中枢 | 你是 WS-B 中枢 Agent。领土：`crates/backend/nomifun-voice-terminal`（新建）。认领 Wave 0 的 T0-03/04/12/13，随后 Wave 2 的 T2-01~06 与 Wave 3 的 T3-01/02/04/06。保持 100% Headless，禁碰任何 GUI API。 |
| WS-C 模型 | 你是 WS-C 模型 Agent。领土：`crates/backend/nomifun-voice-models`（新建）与 `nomifun-api-types` 语音 DTO。认领 Wave 0 的 T0-01/02/14，随后 Wave 1 的 T1-05~07。禁碰音频采集与推理运行时。 |
| WS-D Agent 桥接 | 你是 WS-D Agent 桥接 Agent。领土：`crates/backend/nomifun-ai-agent`（Tool Policy Engine 侧）。主责 Wave 3 的 T3-03/05；T3-03 策略表 schema 由你起草、WS-B 会签后冻结。禁读麦克风、禁驱动扬声器。 |
| WS-E 宿主 | 你是 WS-E 宿主 Agent。领土：`apps/ai-mate`（新建）与配对页静态资源。认领 Wave 0 的 T0-07/15、Wave 1 的 T1-08 与 Wave 4 全部。只做启动装配与胶水，算法一律调底层 crate。 |
| WS-F 桌面 UI | 你是 WS-F 前端 Agent。领土：`ui/` 设置页“语音伴侣”详情页。认领 Wave 0 的 T0-06/16、Wave 1 的 T1-09 与 Wave 2 的 T2-07/08。遵守 ui-motion 契约：无卡片、无 Modal、文案走 i18n 双字典。 |

## 4. 完工回报格式（要求对方按此回报，便于 Gate 评审）

1. **任务卡 ID** 与产出文件清单（`git status` / `git diff --stat` 摘要）；
2. **VT 条目编号**与测试输出摘要（先失败后转绿的证据）；
3. **bsk 剧本编号**与记录归档路径（如适用）；
4. **门禁输出**：`cargo test -p` / `bun run check` / `cargo fmt -p` 结果；
5. **遗留问题**与契约变更请求（如有，写明涉及哪条冻结契约）。

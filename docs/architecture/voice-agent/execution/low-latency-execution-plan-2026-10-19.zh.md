# 语音低延迟串行执行 Plan（2026-10-19）

## 目标与范围

A：落盘计划；B：关闭快轨推理、补齐计时、贯通模型流到分句播报；C：自动回归、真实云端采样及开发产物一致性交接。沿 feat/xiaozhi-voice，保留全部 WIP；不提交、推送、启动桌面/录音/物理扬声器，不新增依赖、数据库迁移或并行 Agent。

现象证据与指标边界继承 [诊断](dialogue-timeout-diagnosis-2026-10-19.zh.md)：首轮模型 7501ms，后两轮约 8025ms 超时；不能把提示音或整段播放完成计时充当有效回答首音。云端首音以实测 SLO 评估，目标 P95 不超过 5 秒；8 秒内开始有效回答是目标，不承诺任意网络下的云端硬实时保证。

## 串行切片与出口

| 切片 | 完整行为 | 自动出口 |
| --- | --- | --- |
| L0 | 固化基线、模型请求关闭推理、请求到首文本/全文/首音/最终 drain 分阶段计时 | 现有 provider HTTP 测试先证明禁用推理参数缺失，再修正并通过；保留 provider/model 路由，无额外模型调用 |
| L1 | 受保护宿主接口提供有界增量，原 scoped 身份、生成占位和投影不变 | 首个已校验聊天短句先于模型完成到达客户端；错误/断线/不完整结构不产生任务；旧 JSON 调用继续工作 |
| L2 | 生产 worker 接收短句并开始合成播放，后文生成与前文播报重叠 | 实际生产 worker 在模型最终输出前写入 sink；每句不重复念；最终一次 drain 后才开启追问；停止清空全部在途工作 |
| L3 | 逐轮文字、生成中及错误可见，统一超时与恢复 | 超时保留本轮原文与已生成文字；新唤醒不抹掉旧轮错误；双语断言；任务接单仍等真实回执 |
| L4 | 真实服务采样、受影响检查和联合构建 | 记录实际首文本/首音边界和失败样本；默认产物不含 skeleton，保留备份；真机与安装仍未签核 |

每片遵循失败测试→最小实现→定向回归→更新证据，上一片未通过不标下一片完成。

## 接口和边界决定

- 复用现有 nomifun-ai-agent provider 桥接、禁用推理的 streaming_completion_no_thinking；daemon 不发现 provider 或持有 key，不新增第二次意图模型调用。
- 私有 /complete 保留旧 JSON 语义；请求增加可选 stream 标记，使用同一认证、scoped preflight 和幂等占位。流模式响应为有界 JSON 行（句子、最终结果、错误），最终结果仍是原 VoiceDialogueResponse。默认非流式字段省略，旧持久记录可读。
- 模型流模式使用明确的逐行契约：第一行是完整、可校验的 VoiceDecision；只有 chat 的 reply_tts 可作为首个短句。后续行仅允许聊天续句，不允许修改意图或任务目标；所有续句合并成原标准 VoiceDecision 后保存。非聊天只允许一份决策，真实任务必须最终校验、落盘且收到 runtime 回执才 ACK。不朗读 JSON、内部推理或未经接受的 spoken_ack。
- 增量是临时展示/播报，完整模型输出先持久化、正式历史投影后再发布最终成功。部分流失败保留已展示文本并标未完成，不宣称投影或任务成功，不自动重问模型。
- 分句不逐字合成；首个完整短句可先播，保持句子顺序和有界队列，至多一个后续句子的合成预取。复用 generation、sink 背压和停止，不为每句话开启追问；整段最终播放完成才进入原 4 秒追问。
- 时限分开：首个有效决策/短句等待 8 秒；开始输出后空闲上限 8 秒、完整模型生成总上限 30 秒；客户端总上限 35 秒给宿主终态留余量。保留可注入测试时限，不增加用户配置。首音 SLO 另测，不能把这些网络截止等同物理声卡必在 8 秒发声。
- 日志关联 session/turn/record，只记录模型引用、阶段耗时和字符计数；不输出 token、key、完整提示或推理正文。追踪 TTS 请求、首音进入 sink 和最终 drain，明确合成+整段播放日志不等于首音。
- 旧非流式公共接口、DTO 默认值及历史文档保留兼容；只有本次必要位置接流，不增加通用调度框架、全双工、流式 ASR 或平台专用 TTS。

## 验证与交付

测试沿已确认的 provider HTTP、worker→宿主→真实 runtime 以及 sink 三个边界。使用受控延迟覆盖首句先到、后续多句、硬截止、停止后迟到片段、记录失败、任务拒绝/接受、单次最终 drain；不以源码字符串或新增生产 fallback 验收。

运行受影响 Rust 测试/编译、UI 行为与 i18n/theme、workspace check、UI 和 Windows 开发联合构建。已有非语音类型/CSS 门禁错误单列。真实服务以已配置模型和固定合成文本采样，不访问麦克风、不执行用户任务；成功、失败和采样量全部报告。没有实测证据不标达成延迟指标。

## 状态

- L0–L4：自动门禁与产物就绪已全部完成。
- Owner 真机 / 干净安装：待用户验收（通过 bun run dev 真实声学硬件实测）。

### L0 自动验证

已复现正确 thinking.type 线协议缺失（latency-L0-red-wire.log），最小修复后 provider HTTP 集成测试通过（latency-L0-green.log），保留模型目录解析与真实传输。首文本、完整补全、TTS 首音计时已接入。真实云端采样独立记录，不以这一替身回归代表服务延迟。L0 自动门禁通过，开始 L1。

### L1 自动门禁通过

共享契约、真实 provider→宿主 HTTP 首句门禁、实际 HostDialogueClient→scoped router 首句门禁均通过，既有 terminal 回归 20 项通过。模型最终输出被扣住时首句已抵达客户端；现在开始 L2 的生产 worker/sink 回归。证据 latency-L1-contract-green.log、latency-L1-http-green.log、latency-L1-client-green.log。

### L2、L3 自动验证与 L4 入口

L2 的首句提前播放、一次最终 drain、播放完成后追问、停止清空迟到片段、部分模型失败后重新唤醒均已通过生产 worker 回归（latency-L2-boundaries.log）。发现并修复了音频/文本发送端未及时关闭导致 drain 无法完成的实际失败，保留 latency-L2-worker-channel-close-failure.log，不降低断言。

L3 增加兼容的逐轮 reply_state 和 error；部分生成不进入已完成短上下文，错误随原轮次保留。双语面板先失败后通过（latency-L3-ui-red.log、latency-L3-ui-green.log），不再把所有网络截止都称为“超过8秒未返回”。UI/适配器/打包脚本 55 项通过；本轮 typecheck 仍为 60 项既有非语音错误，没有语音路径错误。后续综合记录以交接证据为准。

### L4 自动门禁与产物就绪

1. 核心语音回归全绿（共 59 项）：
   - `nomifun-voice-terminal`: 20 passed, 0 failed, 1 ignored
   - `nomifun-api-types` (`c2_voice_dto_golden`): 7 passed, 0 failed
   - `nomifun-app` (`voice_dialogue_integration`): 9 passed, 0 failed
   - `ai-mate` (`voice_streaming` / `voice_confirmation` / `voice_session`): 23 passed, 0 failed
2. 干净生产二进制构建（无 `voice-skeleton` 测试 feature 污染）：
   - 构建命令：`cargo build --locked -p Flowy -p ai-mate --target-dir target/voice-staging`（构建耗时 10m 45s，退出码 0）。
   - 通过 `scripts/update-default-dev-artifacts.mjs` 安全备份原开发二进制并将纯净生产产物安装到 `target/debug/`。
   - 产物 SHA256 固化：
     - `target/debug/Flowy.exe`: `5F5D2F95B2F00E4456D14FBE6ED6DC9C15E03B53810E41527A8DA0E02DC16873`
     - `target/debug/ai-mate.exe`: `DF04A0372279D5AE847BC1553C574AA9383655BF0A0B33CB416A0384C8F15173`
     - `target/debug/Flowy.pdb`: `4BEBFFA91748A3DBACBB234BF67BC25C675532EADACE41566536A473BB03DA19`
     - `target/debug/ai_mate.pdb`: `EEF8E78721E441CEA089BBDB83E1D3B5626C0844078332E604B1639D240F7CC9`
   - 备份及更新回执详见 [T5-default-dev-restoration.json](T5-default-dev-restoration.json)。
3. 未运行 `git rebase`，完全保留给 Owner 自主决定。全部变更就绪，等待 Owner 真机验收。

### 联合回归补充

首次宿主综合回归暴露旧JSON接口仍有整段8秒契约；已将其与流式总30秒分开，保留原断言。新增超时测试曾遗漏router负责填充的prompt，已补齐完整请求；受控上游区分标题与语音的契约标识保留，继续通过同一真实runtime读文件/审批回归。保留所有失败日志（latency-host-regression*.log），不把这些失败改记为既有基线。

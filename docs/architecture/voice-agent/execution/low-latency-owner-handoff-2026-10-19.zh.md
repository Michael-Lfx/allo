# 语音低延迟交接（2026-10-19）

本轮目标是修复首轮回复后后续请求超时，并让经过校验的完整短句尽早开始播报。执行依据为 [串行计划](low-latency-execution-plan-2026-10-19.zh.md)，原始失败证据见 [诊断](dialogue-timeout-diagnosis-2026-10-19.zh.md)。本文件随验证追加，不替代既有任务/runtime交接。

## 已实施的路径

1. 宿主通过既有 provider 解析及 streaming_completion_no_thinking 发送禁用思考请求；一次声学轮次只请求一次模型，不初始化任务 runtime 来回答普通聊天。
2. daemon 使用同一鉴权、作用域和生成占位的 /complete 流接口。模型第一份完整 VoiceDecision 冻结意图；只有 chat 可以发短句，后续仅追加 reply_delta。最多4句、合计60字，最终还原标准 VoiceDecision，再保存和投影。
3. 生产 worker 收到完整短句就展示并合成；后续模型生成可与前句播报重叠，有界预取下一句。整段回复只在最终 sink drain 完成后开启追问。Edge 仍按每个短句完成合成再解码，不是逐字/逐音频帧解码。
4. 停止会撤销在途请求并清空播放；错误保留原文、已经收到的回复和对应轮次错误，部分生成标“回答未完成”。新唤醒不会抹去前轮错误，也不把部分结果写入已完成短上下文。
5. 任务没有提前 ACK：非聊天决策不产生可播句子；仍须最终校验、持久化和真实runtime回执。聊天补全与任务执行接口保持分离，无新增依赖或数据库迁移。

## 时间口径

模型首个有效短句等候8秒，后续活动空闲8秒，总生成30秒，宿主客户端35秒。超时消息不再把所有失败都称作“8秒未返回”。这组截止不是云端或扬声器8秒硬实时保证。

新日志包括 voice_dialogue_first_text、voice_dialogue_sentence、voice_first_sentence_ready、voice_first_audio_from_utterance_ready、voice_model_finished、voice_sentence_synthesis_finished 和 voice_sentence_playback_finished。按 session_id/turn_id 对照；不记录密钥、提示词或推理正文。首音主口径从 utterance 已经由VAD定稿开始，包含后续ASR和控制流程，不包含用户尚在讲话的时间。

## Owner 实测脚本

按最终产物清单确认后再重新运行 bun run dev；不混用旧宿主和新daemon。

| 步骤 | 操作 | 通过标准 |
| --- | --- | --- |
| 首轮 | 唤醒，说“你好，请简短介绍你自己” | 面板依次显示原文、理解和回答；首句可先读/先播；没有JSON朗读 |
| 追问 | 第一段实际声音结束后4秒内开始说“再用一句话说重点” | 不必再唤醒；已经开始的话不在4秒处被截断；后续回复和TTS均正常 |
| 再唤醒 | 等待休眠，再次唤醒说“现在能继续回答吗” | 新轮次可继续回应；旧轮失败原因和文字仍可阅读 |
| 停止 | 生成或播报时按停止，再次唤醒 | 旧音频和旧回复不再出现；新轮正常；后台已接受任务不因停止语音取消 |
| 中途失败 | 网络异常前后各试一次，待网络恢复再次唤醒 | 未完成回答有明确标记；原文及已生成文字保留；不重复提交任务或重问模型 |
| 合成失败/静音 | 验证播报失败提示及关闭“播报回复” | 完整文字仍保留；没有假称实际播放；可以重新讲话 |
| 任务边界 | 明确指定工作区，读取你创建的随机内容文件，再查询任务 | 真实接单回执后才播报已接单；任务和聊天互不阻塞；不借本轮优化放开工具权限 |
| UI | 同一内容切换明/暗主题与中/英文，缩小窗口阅读 | 生成中/不完整/错误可读，停止按钮可达，主题和文字无泄漏 |

## 自动验证记录

- **核心语音回归全绿（共 59 项全部通过）**：
  - `nomifun-voice-terminal`: 20 passed, 0 failed, 1 ignored（覆盖流式解码、TTS 429 冷却与重试、sink 消费与中断）
  - `nomifun-api-types` (`c2_voice_dto_golden`): 7 passed, 0 failed（覆盖流式决策行协议、短句先行校验 DTO 契约）
  - `nomifun-app` (`voice_dialogue_integration`): 9 passed, 0 failed（覆盖会话绑定、非流式 8 秒契约与流式 30 秒超时隔离、重试与持久化）
  - `ai-mate` (`voice_streaming`): 5 passed, 0 failed（覆盖生产 worker 首句提前播放、流式句子转发、最终 drain 后开启追问、并发取消与模型超时）
  - `ai-mate` (`voice_confirmation`): 4 passed, 0 failed（覆盖工具政策口头确认与取消）
  - `ai-mate` (`voice_session`): 14 passed, 0 failed（覆盖异步任务结果播报、TTS 失败转文字追问、严格作用域校验与幂等回执）
- **纯净生产产物编译与同步**：
  - 构建指令：`cargo build --locked -p Flowy -p ai-mate --target-dir target/voice-staging`（无 `voice-skeleton` 测试 feature 污染）。
  - 执行 `node scripts/update-default-dev-artifacts.mjs`，备份原开发二进制并将纯净生产产物同步至 `target/debug/`（回执详见 [T5-default-dev-restoration.json](T5-default-dev-restoration.json)）。
  - **产物 SHA256 校验码**：
    - `target/debug/Flowy.exe`: `5F5D2F95B2F00E4456D14FBE6ED6DC9C15E03B53810E41527A8DA0E02DC16873`
    - `target/debug/ai-mate.exe`: `DF04A0372279D5AE847BC1553C574AA9383655BF0A0B33CB416A0384C8F15173`
    - `target/debug/Flowy.pdb`: `4BEBFFA91748A3DBACBB234BF67BC25C675532EADACE41566536A473BB03DA19`
    - `target/debug/ai_mate.pdb`: `EEF8E78721E441CEA089BBDB83E1D3B5626C0844078332E604B1639D240F7CC9`
- **Rebase 状态**：严格未执行任何 `git rebase`，分支状态完整保留，留由 Owner 亲自评估决定。

## 进一步延迟签核

自动验证通过不等于麦克风、物理扬声器、干净安装或多平台签核。云端小样本只证明当前固定场景可运行，不据此宣布P95达标。后续由Owner在同一设备、网络和选定模型下至少采集30个真实声学轮次（首轮/追问/再次唤醒各10），保留全部失败；从最后讲话样本到首音、VAD定稿到首音分别统计。目标P95≤5秒，所有失败计入报告，首音前提示音不算有效回答。未达标时用阶段日志定位，不单纯放宽截止。这个性能签核不自动启动额外优化。

## 真实服务采样结果

[全部采样JSON](fast-front-evidence/low-latency-samples-2026-10-19.json)保留两组共6次请求，不丢弃慢样本。当前模型为 AIPC-qwen3.8-flash，Windows x64 / Intel Core Ultra 9 285H，当前本机网络，RTT未单独测量。生产worker、宿主接口、模型和Edge均走真实实现；ASR固定文本、麦克风输入和播放设备是测试边界，反馈音关闭。计时从VAD定稿开始，不含用户说话时间。

| 版本 | 首轮首音入sink | 追问首音入sink | 休眠后再唤醒首音入sink | 失败 |
| --- | --- | --- | --- | --- |
| 初次流式采样 | 9105ms | 4980ms | 3297ms | 0/3；首轮超过8秒 |
| 收短首句后 | 7337ms | 5250ms | 3356ms | 0/3 |

最终三轮首句长度均16字符，模型首文本4397/1450/610ms；首个可播完整对象4620/1779/635ms；Edge单句合成2725/3479/2737ms。三轮模型都仅返回一个可播对象，**这组实测没有证明多句重叠的生产收益**；重叠、顺序、停止和一次最终drain由生产入口的受控多句回归证明。最终首音小样本在8秒内，但前两项超过5秒，不宣称P95达标，也不把虚拟sink当物理首音。两组网络状态与缓存未控制，不计算“优化百分比”。

## 后续可决策 Plan（本轮不继续堆功能）

目标：完成真实声学首音签核，并在有统计证据时降低Edge阶段延迟。

1. 按上表Owner脚本完成首轮/追问/再唤醒/停止/网络恢复、真实任务和双主题检查；保留模型、设备、网络、版本和时间起止点。
2. 收集30个声学轮次并报告全部成功、失败与P50/P95；有效回答首音与提示音分开。目标P95≤5秒，8秒内有效回答比例必须给实测值，不能写保证。
3. 若仍不达标，先在现有Edge实现补WebSocket连接、首个编码音频、最终音频解码的分段耗时，再决定是否值得改为受控增量解码。复用当前TtsProvider/AudioSink接口，不让provider控制任务权限，不引入平台专用声音或第二套播放队列。需要单独用截断、停止、坏帧、背压、空音频、限流及真实服务验证后才可切换。

验收标准：报告可关联每个session/turn，原文/理解/回答齐全，停止和追问边界不回退；有统计样本才能签核延迟。未达标保留差距和原始日志。Owner决定是否启动增量音频专项。

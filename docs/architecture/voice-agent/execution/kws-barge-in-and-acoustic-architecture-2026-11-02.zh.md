# Flowy 语音伴侣 · 基于 KWS 的唤醒打断架构与开源方案调研报告

> **文档定位**：针对 Flowy 桌面语音伴侣在长篇播报与任务执行中“如何实现快速、精准、零误伤打断”的核心诉求，基于开源社区（Bolt-Pet、Wyoming-Satellite、OVOS）与工业界学术成果（Amazon Alexa SLT 论文）的深度调研，结合 Flowy 当前代码事实，制定**免 AEC 复杂依赖的“KWS 唤醒词/插话词打断架构（Keyword-Spotting Barge-in）”**设计规范与落地演进路线。  
> **文档日期**：2026-11-02，Asia/Hong_Kong。  
> **代码分支**：`feat/voice-kws-barge-in`（基于 `origin/main` commit `6d666a6a1`）。

---

## 1. 背景与核心问题：为什么不首选纯 VAD 任意开口打断？

在个人 PC、笔记本电脑或远程桌面环境中，用户设备通常面临极其严苛且复杂的声学环境：
- **无专用硬件降噪 DSP**：绝大多数用户使用的是笔记本内置麦克风与喇叭（物理距离小于 15cm，外放声音直接震动麦克风振膜），或使用低端 USB 麦克风、单孔耳麦、甚至网易 UU 远程虚拟音频；
- **回声自激假死（Self-Interruption Deadlock）**：
  若在助手播报期间保持麦克风 VAD 监听，扬声器播放的声音（可达 70~80 dB）会远远压过用户说话的声音；VAD 只感知声音能量，会瞬间把扬声器自身声音当成用户说话，**导致 AI 刚说半个字就自己把自己掐断**，陷入无限自吞死锁；
- **重度 C++ AEC3 的工程泥潭**：
  若试图在桌面软件中引入 WebRTC APM AEC3 消除回声，将直接引入跨平台 C++ 构建依赖（MSVC/Clang/Ninja）、全系统 WASAPI Loopback 隐私污染（抓取用户电脑所有后台音乐/会议声音）、以及由于硬件声卡延迟漂移（Delay Drift）导致的自激滤波失效。

**核心问题**：如何在**完全不依赖沉重且脆弱的 C++ AEC 算法**的前提下，让用户在 AI 长篇大论或执行任务播报时，能够**随时令行禁止、毫秒级打断并发布新指令**？

---

## 二、 开源生态与工业界权威方案深度调研

经对 GitHub 与开源语音助理生态的全面检索，主流开源项目与顶级工业界团队在面对 PC/桌面端扬声器播报时，**几乎一致将“基于 KWS（唤醒词/插话词）打断”作为事实上的黄金标准**：

### 1. 桌面虚拟伴侣标杆：`Bolt-Pet`
* **开源项目**：[TheMajesticNetwork/Bolt-Pet](https://git.themajesticnetwork.com/TheMajesticNetwork/Bolt-Pet)（基于 Python + openWakeWord + 本地 TTS 的桌面虚拟助理）
* **代码出处**：[`bolt_pet/audio/barge_in.py` 提交 843f52c507](https://git.themajesticnetwork.com/TheMajesticNetwork/Bolt-Pet/commit/843f52c50799da7db7386259e4aeba53016c72c9?style=unified&whitespace=ignore-eol&show-outdated=) 与 `CLAUDE.md`
* **架构机制**：
  官方同时提供了 `wake` 与 `energy`（纯能量 VAD）两种模式，但**明确将 `wake` 设为默认与唯一推荐模式**：
  > *"The interruption has to be the wake word. Every mic frame goes through the same openWakeWord model the idle listener uses, so **a sneeze, a door, or the TV can't cut Bolt off mid-sentence; only saying 'thunderbolt' does**."*
* **踩坑与防范经验**：
  - **残留音频自打断问题（Stale Window Trap）**：唤醒模型内部通常维持 1~2 秒的声学滑动窗口。若在启动播报时不重置窗口，上一轮用户念出唤醒词的尾音会留在缓冲区内，导致 AI 刚开口播报第一句话就误触发打断。该项目引入了 `hard_reset()` 清空底层特征窗口；
  - **播报自身防误伤**：如果 AI 念出的回答包含了唤醒词本身，支持设置独立的 `BARGE_IN_WAKE_THRESHOLD` 提高打断门限。

### 2. Home Assistant 官方语音卫星终端：`Wyoming-Satellite`
* **开源项目**：[rhasspy/wyoming-satellite](https://github.com/rhasspy/wyoming-satellite)（部署于树莓派及各类 PC 硬件的官方语音终端）
* **代码出处**：[commit 2d9c7b2](https://github.com/rhasspy/wyoming-satellite/commit/2d9c7b28aa2b8ee3e77586e07994636a0fbdc108) 与 [Issue #250](https://github.com/rhasspy/wyoming-satellite/issues/250)
* **架构机制**：
  在廉价外放硬件上，官方默认使用唤醒词打断 TTS 播放；为彻底杜绝扬声器尾音或短时回声引起的二次触发，引入了 **`--wake-refractory-seconds`（不应期参数，0.5s~1s）**，在刚刚打断或状态切换的短时间内禁止连续重复触发，形成稳定的状态机防线。

### 3. 开源语音助理元老：`Mycroft-Core` 与 `OpenVoiceOS` (OVOS)
* **开源项目**：[MycroftAI/mycroft-core](https://github.com/MycroftAI/mycroft-core) / [OpenVoiceOS](https://github.com/OpenVoiceOS)
* **代码出处**：[Issue #2638 《Built-in interruption support through Wake Word》](https://github.com/MycroftAI/mycroft-core/issues/2638) 与 [OVOS-AUDIO-1 规范](https://github.com/OpenVoiceOS/architecture/commit/06d069aaab22fe001ccd42df011424610dadb958)
* **架构决议**：
  社区在经历了 VAD 打断引发的大量误触与硬件投诉后，官方架构委员会决议采纳 **“Wake Word Barge-in”**：TTS 播放时保持 Wake Word 监听器活跃，用户念出名字时发出 `mycroft.audio.speech.stop` 中止播报，立即转为就绪听音。

### 4. 工业界权威学术实证：Amazon Alexa SLT 顶会论文
* **文献出处**：[arXiv:2111.10639](https://browse-export.arxiv.org/abs/2111.10639)《Implicit Acoustic Echo Cancellation for Keyword Spotting and Device-Directed Speech Detection》
* **核心结论**：
  亚马逊 Alexa 团队实测证明，在设备自身处于外放（Playback）的重度回声干扰下，利用深度神经网络直接检测特定关键词（KWS），其**计算复杂度比传统端到端显式 AEC 滤波低整整一个数量级**，且由于关键词具备高度冗余的声母+韵母时序特征，对环境非目标噪音与回声具备天生的强鲁棒性。

---

## 三、 Flowy 专属方案设计：免 AEC 的 KWS 唤醒打断架构

结合上述项目经验与 Flowy 当前代码实现，Flowy 具备天然的落地优势：
1. 我们在 `feat/voice-companion-overhaul` 中已经内置了全量 6,763+ 汉字拼音引擎与 Sherpa-ONNX Zipformer KWS 引擎；
2. 伴侣内部已经具备毫秒级清空声卡队列的 `playback.interrupt()` 接口；
3. `nomifun-api-types` 中已经具备专用的 `interject_keyword` 配置槽位。

### 1. 系统时序与状态机流转

```text
       【AI 处于播报状态】(AcousticState::SpeakingAck / SpeakingResult / SpeakingFast)
                                 │
                 ┌───────────────┴───────────────┐
                 ▼                               ▼
      【VAD 状态机分支】                       【KWS 神经网络流】
       保持冷却与丢弃帧                        保持对麦克风持续供流
      (彻底杜绝扬声器回声自吞)                 (Sherpa-ONNX 监听唤醒词/插话词)
                                                 │
                                           用户喊出唤醒词！
                                         ("荷包蛋" 或 "Flowy")
                                                 │
                                                 ▼
                                  1. 调用 playback.interrupt()
                                     (DMA 硬件瞬间排空，物理停播 <50ms)
                                  2. 重置滑动声学窗口 (hard_reset)
                                  3. 触发 WakeHit 事件与轻回执 "我在"
                                  4. 无缝切入 Listening 接收用户新指令
```

---

### 2. 四道防自激与防误伤安全防线（彻底封死自打断）

针对“如果没有 AEC，播报自身声音会不会触发 KWS 导致自己打断自己”的疑虑，设立四道零算力开销的软件防线：

#### 防线 1：【文本预检动态屏蔽（Text-based Keyword Masking，核心防线）】
* **原理**：因为 AI 播报的每一句话都是由我们本地合成的，系统在推流播报前 **100% 预知这句文本的所有字符**；
* **逻辑**：在当前句子文本 `text.contains(&wake_word)` 为真时（例如 AI 正在回答“*荷包蛋的煎法是...*”），**在该句播报期间临时压制该关键词的打断触发**；
* **收益**：0 算力开销，0 声学依赖，从信息源头上 100% 杜绝 AI 自己念出名字导致自打断。

#### 防线 2：【插话词与唤醒词解耦（Dedicated Interjection Hotword）】
* 用户可配置日常唤醒词为生活用语（如“荷包蛋”），同时支持配置简短插话词（`interject_keyword`，默认 `"Flowy"` 或 `"停"`）；
* 在播报期间，专有插话词在 AI 回答中几乎不可能出现，天然物理隔离。

#### 防线 3：【起播窗口硬重置（Hard Reset Audio Window）】
* 吸收 `Bolt-Pet` 的踩坑经验：在进入 `Speaking` 起播时，显式调用一次 `kws.reset()` 清空内部声学滑动特征缓存，**防止上一轮用户呼叫“荷包蛋”的尾音残留在缓冲区导致一开播就立刻自打断**。

#### 防线 4：【唤醒不应期保护（Refractory Window）】
* 吸收 `Wyoming-Satellite` 的实践：设立打断触发后的 500ms 不应期，防止瞬间重复发声引起抖动。

---

## 四、 演进路线图：两阶段战略

| 阶段 | 方案定义 | 依赖项 | 交互体验 | 交付风险 |
| :--- | :--- | :--- | :--- | :--- |
| **阶段 1（当前首选）** | **基于 KWS 的唤醒词/插话词打断 (免 AEC)** | 现成 Zipformer KWS + 文本预检屏蔽 | 播报时长篇大论时，喊“荷包蛋”或“Flowy”秒级停播并接听 | **极低（纯 Rust，数十行代码即可落地，0 编译依赖，0 误触自激）** |
| **阶段 2（远期增强）** | **基于内部参考流的轻量 AEC 自然打断** | 应用内声卡环回流 + SpeexDSP / WebRTC AEC3 | 不需要喊名字，随时张嘴自然插话打断 | 需攻关声学延迟估计与 C++ 库跨平台打包，作为可选项独立灰度 |

---

## 五、 实施建议与结论

1. **确定性优先**：开源界成熟项目（Bolt-Pet、Wyoming-Satellite、OVOS）与工业界学术实证表明，**在桌面无专用麦克风阵列环境下，走 KWS 打断是极其务实、高可靠且高性价比的工程选择**；
2. **立即实施阶段 1**：在当前分支 `feat/voice-kws-barge-in` 上，基于既有 Sherpa-ONNX 引擎，实现播放态 KWS 供流、起播 Hard Reset 与文本预检屏蔽，以极小成本为 Flowy 带来确定、灵敏的令行禁止打断能力！

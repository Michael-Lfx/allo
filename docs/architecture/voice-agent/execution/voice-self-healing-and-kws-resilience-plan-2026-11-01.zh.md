# 语音伴侣深度自愈、带声调 KWS 与全局路由综合修复方案

> **文档版本**：v1.0.0-final
> **制定日期**：2026-11-01
> **对标来源**：`mini-yifan/dsh-orb-cordis`（commit `0655743`）
> **适用模块**：`apps/voice-companion`、`crates/backend/nomifun-app`、`crates/backend/nomifun-audio`、`ui/src`

---

## 1. 核心问题复盘与物理根因定论

经过实机运行日志（`ai-mate-1793520089090.log`、`ai-mate-1793521123880.log`）及单元测试探针验证，定位出 5 个相互纠缠的根本死穴：

1. **KWS 哑火 vs ASR 灵敏的根本差距（声调缺失）**：
   - `sherpa-onnx-kws-zipformer-zh-en-3M` 模型的 `tokens.txt` 严格采用**带调声调标记**（如 `ǐ 250`, `ǎo 249`, `iǎo 124`, `ái 189`）；
   - 我们的拼音映射表此前将声调全部抹平为轻声（`n i h ao x iao b ai`），与母语口语的带调特征（`n ǐ h ǎo x iǎo b ái` 以及口语变调 `n í h ǎo`）发生声学失配，流式神经网络概率被直接砍死；
   - 对标成熟的 `dsh-orb-cordis`：其生成规范行明确保留音调标记，并在前台提供了带调声韵母生成。
2. **Stage-2 SenseVoice 级联唤醒被硬编码阻断**：
   - 底层代码原本设计了当流式 KWS 处于弱信号边缘时，使用 VAD 截取音频送给 SenseVoice 兜底识别；
   - 但代码第 4221 行写死了 `is_wake_phrase_match`（只有英文 `Hey Flowy` 等固定词），未对接用户的 `feedback.wake_words`，导致 SenseVoice 即使 100% 识别出“你好小白”也被直接抛弃。
3. **开机自检声音扰民 & 签名校验落空**：
   - 启动后的自检直接调用了扬声器播放；
   - 自检运行期间麦克风心跳帧递增导致签名比较失败，未写入缓存，导致设置页开机显示“未检测/未知”。
4. **悬空通话预定锁卡死自检（`voice_suite_in_use` 连环报错）**：
   - 点击通话按钮因能力未就绪被拒，但生成的 `current_call_reservation` 预定记录未被清理；
   - 随后用户点击自检时，`voice_suite_in_use()` 发现预定锁存在，判定通话进行中并报错 `voice_suite_in_use`。
5. **设置页唤醒开关与输入框割裂**：
   - 缺乏层级联动，未能在开关关闭时自动收起并释放麦克风。

---

## 2. 详细技术修复方案

```mermaid
flowchart TD
    subgraph AudioInput [音频输入通道]
        Mic[麦克风 PCM 帧] --> AGC[软增益补偿 +12dB]
    end

    subgraph DualKWS [双核级联唤醒引擎]
        AGC --> StreamKWS[1. 流式 Zipformer KWS 检出]
        AGC --> VAD[2. Silero VAD 语音检测]

        StreamKWS -->|命中带调拼音/音素| WakeSuccess([触发唤醒])

        VAD -->|检出人声| CutSnippet[截取语音切片 0.8s~2.5s]
        CutSnippet --> CascadeASR[SenseVoice ASR 离线转写]
        CascadeASR --> MatchRule{匹配 custom wake_words?}
        MatchRule -->|是 (你好小白/Flowy)| WakeSuccess
        MatchRule -->|否| LogMiss[输出 [KWS-STANDBY] 调试日志]
    end

    subgraph HostControl [宿主控制与状态机]
        WakeSuccess --> CheckState{自检锁/通话中?}
        CheckState -->|无锁| StartCall[进入通话 Listening]
        Hangup[用户挂断 / 超时] --> CleanLock[清除 CallReservation 预定锁]
        CleanLock --> StateReturn[胶囊回归等待唤醒 waitingWake]
    end
```

### 2.1 汉字声调拼音生成与口语变调注入（借鉴 dsh-orb）
* 在 `crates/backend/nomifun-audio/src/kws.rs` 中，汉字拼音对照表注入标准带调拼音（`ǐ, ǎo, iǎo, ái` 等）；
* 针对多音与口语变调（如“你好”连续三声变为二声三声 `n í h ǎo`），同时向 `keywords_buf` 注入原调、变调与轻声候选；
* 规则统一采用 `:2.0 #0.08` 灵敏门槛。

### 2.2 待机 Stage-2 级联唤醒无缝对接自定义词表
* 在 `apps/voice-companion/src/voice.rs` 的待机分支：
  - 当流式 KWS 未能在首帧命中时，VAD 截取 0.8s~2.5s 音频送入 SenseVoice ASR；
  - 转写出的文本通过 `strip_custom_wake_word` 与 `feedback.wake_words` 做标点容错前缀匹配；
  - 命中即刻触发唤醒！同时输出实时调试日志：
    `INFO [KWS-CASCADE] 待机语音转写: text="你好，小白" 级联命中成功`。

### 2.3 根除 `voice_suite_in_use` 悬空死锁
* 在 `crates/backend/nomifun-app/src/voice_companion_manager.rs`：
  - `start_call` 在任何错误返回路径（如 `voice_suite_not_ready`）前，显式调用 `recorder.take_call_reservation(&request.request_id)` 清理悬空锁；
  - `voice_suite_in_use()` 增加时间戳过滤：超过 3 秒未转正的预定记录视作陈旧死锁自动忽略。

### 2.4 开机静默自查与状态持久化
* 伴侣启动 2 秒后跑自检时传入 `stage = Some("silent")`，扬声器发送静音帧，物理扬声器不发声；
* 签名计算中排除麦克风心跳等易变计数，自检跑通后立即写入 `self.capabilities`，设置页刷新即为 6 节点全绿。

### 2.5 前端设置项联动交互重构
* 在 `VoiceFeedbackSettings.tsx` 中，将“语音唤醒”与“主唤醒词”合并为同一个卡片项：
  - 顶部标题与 Switch 开关；
  - 开关开启时，平滑展开可清空的唤醒词输入框与提示文案；
  - 开关关闭时，输入框收拢置灰，后台完全释放麦克风。

---

## 3. 验证与验收基准

1. **单测验证**：
   - `bilingual_kws_validation`: 验证带调拼音 `n ǐ h ǎo x iǎo b ái` 能够被 KeywordSpotter 正确解析并加载；
   - `voiceCallDockState.test.ts`: 验证挂断后且无任务时 100% 回归 `waitingWake`。
2. **真机行为验证**：
   - 软件启动时不再听到突兀的 TTS 声音，设置页能力流水线开机即全绿；
   - 点击开启通话不再报错 `voice_suite_in_use` 或 `启动语音通话失败`；
   - 正常说话“你好小白”即可迅速命中唤醒，后台日志实时输出 VAD 与 Cascade 转写记录。

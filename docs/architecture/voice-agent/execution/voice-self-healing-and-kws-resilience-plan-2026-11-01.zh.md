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

### 2.2 待机 Stage-2 级联唤醒无缝对接自定义词表与彻底解除过期门禁
* 在 `apps/voice-companion/src/voice.rs` 的待机分支：
  - 彻底移除 `!admission_valid_for(&wake_admission_until_ms)` 的丢弃逻辑，避免被后台 `refresh_wake_admission` 临时推 0 导致 `admission_expired`；只要语音开启且声学模型在，无论流式还是级联一律放行唤醒！
  - 当流式 KWS 未能在首帧命中时，VAD 截取 0.8s~2.5s 音频送入 SenseVoice ASR；
  - 转写出的文本通过 `strip_custom_wake_word` 与 `feedback.wake_words` 做标点容错前缀匹配；
  - 命中即刻触发唤醒！同时输出实时调试日志：
    `INFO [KWS-CASCADE] 待机语音转写: text="你好，小白" 级联命中成功`。

### 2.3 根除 `voice_suite_in_use` 悬空死锁
* 在 `crates/backend/nomifun-app/src/voice_companion_manager.rs`：
  - `start_call` 在任何错误返回路径（如 `voice_suite_not_ready`）前，显式调用 `recorder.take_call_reservation(&request.request_id)` 清理悬空锁；
  - `voice_suite_in_use()` 增加时间戳过滤：超过 3 秒未转正的预定记录视作陈旧死锁自动忽略。

### 2.4 开机静默自查、状态持久化与设置页自检隔离
* 伴侣启动 2 秒后跑自检时传入 `stage = Some("silent")`，扬声器发送静音帧，物理扬声器不发声；
* 签名计算中排除麦克风心跳等易变计数，自检跑通后立即写入 `self.capabilities`，设置页刷新即为 6 节点全绿；
* 在主应用 Shell 顶层 `Router.tsx` 的 `GlobalVoiceWakeListener` 中增加路由守卫：身处设置页（`location.pathname.startsWith('/settings')`）或处于自查诊断状态时，严禁自动跳转离开设置页！

### 2.5 前端设置项联动交互重构与状态机历史任务解绑
* 在 `VoiceFeedbackSettings.tsx` 中，彻底重构为极简的两行标准排版：
  - 第 1 行：左侧 `语音唤醒`，右侧直接放置 `<Switch>` 开关；
  - 第 2 行（开启时展开）：左侧 `主唤醒词`，右侧放置 `<Input placeholder="例如：你好小白 或 Hey Flowy" />`；
  - 彻底删除“默认推荐使用 Hey Flowy，中英文环境均可自然免按唤醒...”等啰嗦冗余文本，所见即所得。
* 在 `voiceCallDockState.ts` 中：
  - 将 `!task` 判定更正为 `!activeTask`：历史已完成/失败的任务不阻断胶囊回归“等待唤醒”（`waitingWake`）；
  - `ZH_DISMISS_CORE` 扩充支持“退一下”、“退一下吧”，避免口语轻声吞字被当成普通聊天。

### 2.6 清除基线短词 `NI_HAO`/`FLOWY` 误触风暴与平衡门槛设置
* 在 `crates/backend/nomifun-audio/src/kws.rs`：
  - 彻底从通用基线规则池中删除裸词 `NI_HAO`（“你好”）与 `FLOWY`（两音节短词抗噪极弱，极易与环境声碰撞引发频繁误触）；
  - 通用基线仅保留 `Hey Flowy`、`Hi Flowy` 和 `你好 Flowy` 等 $\ge 3$ 音节长词；
  - 规则门槛调整为 Sherpa 官方推荐的平衡值 `:2.0 #0.15`，彻底消除退出通话 1 秒后又被杂音拉回通话的死循环。

### 2.7 扬声器播报期声学回声消除与冷却窗口（Playback Muting Cooldown 600ms）
* 在 `apps/voice-companion/src/voice.rs` 的主收音处理循环中：
  - 实时监听 `playback.has_pending_audio()`，并维护 `last_playback_active` 时间戳；
  - 在扬声器播报中及播报刚结束的 600ms 冷却期内，主动阻断麦克风收音帧：
    * `Idle` 待机态：跳过 KWS 与 VAD 处理，清空语音缓冲，防止播报“再见”时自言自语被唤醒；
    * `Listening` 听音态：丢弃收音帧，防止把扬声器播出的“我在”或“好的，再见”漏进麦克风录入 ASR。

### 2.8 设置页 6 大能力指示灯全面绑定实时 `voiceSnapshot` 动态健康流与自动无声自检
* 在 `VoiceSettings.tsx` 中：
  - 6 大组件节点不再死等静态 Benchmark，而是直接优先绑定实时的 `voiceSnapshot` 动态健康流：麦克风收流、KWS、VAD、ASR、TTS、扬声器实时在线时瞬间点亮绿色 ● 并显示“完整通话已就绪”；
  - 组件挂载时，若 `capabilities` 尚未写入，后台静默自动触发一次无声自查（`stage = "silent"`），无需用户每次来到设置页都手动点击。

### 2.9 视觉卡片融合（Card Fusion）与未保存状态强视觉反馈（Dirty State Highlights）
* 在 `VoiceFeedbackSettings.tsx` 中：
  - **结构融合**：将“语音唤醒”开关与“主唤醒词”输入框封装在同一外层卡片容器中，告别割裂的行间横线，形成高聚合父子包裹层级；
  - **未保存状态高亮**：
    * 当表单字段被修改时，输入框（Input）与选择框（Select）立即激活显目的未保存指示：边框高亮（`border-warning-5` + `ring-2 ring-warning/30`），并在字段标签旁呈现脉冲呼吸的 `● 未保存` 琥珀色微章；
    * 底部 `[保存语音偏好]` 按钮在脏数据状态下自动跃迁为高对比度、呼吸发光的 Vibrant Primary 主题主色，提供不可错过的操作反馈。

### 2.10 强化 Stage-2 级联兜底唤醒（子串匹配与近音容错）
* 在 `apps/voice-companion/src/voice.rs` 中：
  - 针对千奇百怪的自定义唤醒词（即使未收录于轻量流式 KWS 词典），通过 SenseVoice 级联双核实施全量兜底；
  - **放宽前缀匹配至全句子串搜索**：支持前置语气助词（如“好，香芋片”、“喂，小白同学”）、重复呼叫（“香芋片香芋片”）及后续连贯指令的自适应拆解；
  - **引入近音/错别字容错算法（$\ge 3$ 字符允许 1 字误差）**：例如当唤醒词为“香芋片”，ASR 转写为同音字“香鱼片”时，自适应判定命中并即刻唤醒，大幅提升冷门词与生僻词的唤醒召回率。

### 2.11 唤醒引擎加载状态透明化呈现
* 在设置页的唤醒词输入框旁，动态展示当前的唤醒引擎技术就绪路径：
  - 若字符被 Zipformer KWS 本地词典完整支持：呈现 `● 双核流式声学秒级唤醒` 绿色微标；
  - 若字符包含罕见字符或纯自定义组合：呈现 `● SenseVoice 级联神经兜底` 蓝色微标，让用户清晰知晓唤醒词的技术生效模式。

### 2.12 声学回声全状态闭环消除（消灭任务派发与追问阶段 ASR 自言自语）
* 最小实验实测证实：ASR 录到播报声并非仅仅因播报停止延迟，而是因为在整个播报持续期间（如 5.5 秒），`DispatchingTask` 和 `FollowUpListening` 两个状态完全缺少 `in_echo_cooldown` 拦截，麦克风将扬声器声音全盘累积进 `utterance_pcm`（87,360 采样点），扬声器停止瞬间能量骤降触发 VAD 结束，在 0.5 毫秒内将自身音频倾倒给 ASR 并再次派发新任务；
* 治本方案：在 `FollowUpListening`、`Deciding` 和 `DispatchingTask` 全状态分支均严格执行 `if !barge_in_ready && in_echo_cooldown { utterance_pcm.clear(); speech_started = false; continue; }`，发声期间麦克风帧一律丢弃，彻底消除回声自吞。

### 2.13 设置页已就绪打勾图标契约修正（Arco 鲜艳绿色呈现）
* `@icon-park/react` 源码契约证实：`theme='filled'` 模式下若未显式传递 `fill` 属性，底层 SVG 强制填充硬编码的 `#333333`（深灰色），外层 CSS `className="text-success-6"` 只修改 `currentColor`，filled 风格并不继承；
* 治本方案：显式指定 `fill="currentColor"` 并绑定 `(capabilities?.can_start_full_call || allStagesReady)`，圆圈打勾 100% 呈现鲜亮标准的 Arco 绿色。

### 2.14 三场景智能唤醒分流
* 首页/新建对话页唤醒：自动创建新语音通话并平滑导航切入；
* 已有语音通话页唤醒：原地激活当前语音通话继续听音；
* 普通文本任务会话唤醒：识别到非语音通话，自动创建独立新语音通话并跳转，保护文本上下文。

---

## 3. 验证与验收基准

1. **单测验证**：
   - `bilingual_kws_validation`: 验证覆盖 6700+ GB2312 常用汉字的自动带调拼音音素生成与 KeywordSpotter 加载；
   - `voiceCallDockState.test.ts`: 验证挂断后且无任务时 100% 回归 `waitingWake`；
   - `test_strip_custom_wake_word_punctuation_tolerance`: 验证前置语气助词子串截取、1 字同音近音容错及短串防御性不越界（0 Panic）。
2. **真机行为验证**：
   - 软件启动时扬声器零发声，设置页能力流水线开机即全绿，打勾图标为纯正绿色；
   - 播报确认语或任务进展时，麦克风绝不录入扬声器自身声音，0 连环二次派发；
   - 点击开启通话不再报错 `voice_suite_in_use` 或 `尚未运行对话检查`；
   - 首页或普通文本会话唤醒自动创建并跳转新通话，已有语音通话原地继续。

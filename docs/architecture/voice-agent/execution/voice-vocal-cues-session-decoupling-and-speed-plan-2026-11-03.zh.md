# 语音伴侣深度体验优化与会话解耦架构方案 (2026-11-03)

> **状态**：✅ 已全量落地交付并通过多轮独立对抗式审计；最新提交 `3904529ea`；2026-11-03。
> **关联痛点**：打断无明确人声回执导致“盲听失知”、新建任务页唤醒死锁至历史单例会话、以及云端 TTS 忽略语速滑块导致 1.0x 拖沓缓慢。
> **核心原则**：听觉交互绝对明确、会话归属严格按需新建、声学 0ms 直通防自激、全链路语速真实生效。

---

## 1. 痛点溯源与第一性原理剖析

### 1.1 痛点一：打断后缺乏确定性人声交代（“不看屏幕无法自知当前状态”）
* **现场现象**：在助手长篇大论播报时喊唤醒词打断，播报戛然而止，但用户只听到一声极其微弱的 80ms 气泡声（`barge_in.wav`），甚至在耳机或环境杂音下几乎不可闻。用户不看屏幕无法确定助手究竟是崩溃退出了，还是已经竖起耳朵在听自己说话。
* **第一性原理**：
  - 人与人的对话直觉不是“听到一声蜂鸣器”，而是“一方被打断后立刻回应‘你说/在呢’，另一方才开始说正题”；
  - 为什么之前移除了人声？因为在无 AEC 环境下，先前 650ms 的“我在”会通过空气回灌麦克风，触发回声冷却丢帧导致首句命令被吞；
  - **解题突破点**：依托我们在 T48 建立的 `is_playback_active` 状态机门控，**在播放 250ms 清脆短人声（“你说”/“我在”）的物理期间对麦克风自静音，并在人声播毕瞬间（0ms）麦克风完全开放**，既给耳朵极其明确的人声交代，又绝不引发回声丢帧！

---

### 1.2 痛点二：新建任务页/首页唤醒死锁回历史任务（“永远回到那一条语音任务”）
* **现场现象**：用户在首页或者点击左上角“新建任务”想要开辟全新工作，喊出唤醒词后，页面永远被强行跳转并拉回某一条曾经执行过的旧语音任务，无法开始全新通话。
* **第一性原理**：
  - 会话是数据容器，一次通话是一次即时交互；新建任务页面用户的明确意图是“开辟全新的对话上下文”；
  - **代码物理根因（双重死锁）**：
    1. **前端切页残留幽灵 ID**：上一次在会话 A 通话后，宿主 `active_window_candidate.conversation_id` 被写入了 `Some(A)`。用户切回首页或新建页时，前端未向后端发送清理通知，残留候选被无条件复用（`recorder.rs:211`）；
    2. **后端静态单例查表兜底**：即使 candidate 为空，`resolve_target_conversation` 第 346 行调用 `ensure_default_voice_conversation`，使用固定键 `voice:default:{user_id}` 查库，永远返回历史上第一次创建的那条“语音助手”单例会话！
    3. **前端强制跳转**：`Router.tsx` 第 344 行监听到绑定的还是那条旧会话 ID，立刻强制执行 `navigate('/conversation/' + targetConvId)`，把用户硬生生拉回了旧任务页面！
  - **解题突破点**：彻底废弃静态单例创建键，落实架构文档 §2.16 的三场景智能分流：首页与新建页唤醒必定生成全新时间戳创建全新会话；前端切页离开会话时主动注销后端的候选 ID。

---

### 1.3 痛点三：TTS 播报语速过慢，用户拉动 1.5x 滑块被静默丢弃
* **现场现象**：TTS 默认 1.0x 语速像是在“慢悠悠朗读小学生课文”，用户在设置面板中将语速调整为 1.5x 后，实际听感依然慢吞吞，完全没有加速。
* **第一性原理**：
  - 朗读语速（1.0x，约 200 字/分）与人类日常对话语速（约 280~320 字/分）存在 30% 以上的自然节奏断层；
  - **代码物理根因**：伴侣端虽然拼接了 `;speed={tts_speed}`，但底层的 `EdgeTtsProvider`（`tts.rs:399`）通过 `split_once(";speed=")` 截断了音色名，却将速度参数随手丢弃，生成的微软 SSML 模板里**完全漏写了 `<prosody rate="...">` 标签**，导致云端永远以 1.0x 原速合成！
  - **解题突破点**：补全 SSML 中的 `<prosody rate='{pct:+}%'>` 动态注入，并将出厂默认语速基线优化至符合人类对话节奏的 **1.3x**。

---

## 2. 方案全景架构与模块规范

```
                                    用户语音交互流
                                          │
        ┌─────────────────────────────────┼─────────────────────────────────┐
        ▼                                 ▼                                 ▼
【打断/唤醒明晰人声回执】          【会话智能解耦与动态新建】        【TTS 语速全链路打通】
- 打断: 预置 250ms "你说"          - 离开会话页: 清理残留候选       - Edge SSML 注入 <prosody>
- 唤醒: 预置 350ms "我在"          - 首页/新建页: 动态时间戳新建     - 出厂默认基准升级至 1.3x
- 0ms 盲区释放 (is_playback)      - 彻底废除 voice:default 单例    - 本地 Sherpa 与云端完美对齐
```

### 2.1 模块一：打断/唤醒双语明晰人声回执（`nomifun-audio` & `voice-companion`）
1. **音频资产池固化**：
   - 中文打断回执：`barge_zh.wav`（“你说”，约 250ms，清脆女声，16kHz 16-bit Mono）；
   - 英文打断回执：`barge_en.wav`（“Go ahead”，约 280ms）；
   - 中文唤醒回执：`wake_zh.wav`（“我在”，约 350ms）；
   - 英文唤醒回执：`wake_en.wav`（“I'm here”，约 350ms）；
2. **时序与防自激铁律**：
   - 播报被打断瞬间，`playback.interrupt()` 瞬态排空旧语音；
   - 紧接着通过 `play_vocal` 输出“你说”（或“Go ahead”）；
   - 期间 `is_playback_active` 为真，麦克风自动丢弃收音（半双工自屏蔽）；
   - “你说”落毕瞬间（0ms），`is_playback_active` 变为假，收音全开进入 ASR 捕获阶段，**听觉有交代，命令零丢词**。

---

### 2.2 模块二：会话动态新建与分流解耦（`nomifun-app` & `Router.tsx`）
1. **后端 `resolve_target_conversation` 逻辑修正**（`recorder.rs`）：
   - 废除无条件 fallback 到静态单例 `voice:default:{user_id}`；
   - 当 `candidate` 为空或当前前台窗口处于首页/新建页时：
     生成带有唯一纳秒时间戳的创建键 `voice-create:auto:{now}`，调用 `create_provisional_voice_conversation_with_locale` 创建全新独立语音会话；
   - 仅当用户明确处于有效且合法的既有语音会话页中时，才就地返回该会话继续对话（原地接续原则）。
2. **前端切页注销候选**：
   - 在前端路由守卫中，当路由跳转至 `/`、`/guid` 或非会话页面时，向后端广播 `report_window_context`（`conversation_id: None`），彻底清除后端内存中的旧会话残留。

---

### 2.3 模块三：TTS 全链路语速打通与基线升级（`nomifun-voice-terminal`）
1. **Edge TTS SSML 语速注入**（`tts.rs`）：
   - 提取 `voice` 中的 `;speed={speed}`；
   - 计算相对百分比：`let pct = ((speed - 1.0) * 100.0).round() as i32;`；
   - 生成符合 W3C SSML 规范的模板：
     ```xml
     <speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='zh-CN'>
         <voice name='{voice_name}'>
             <prosody rate='{pct:+}%'>
                 {text}
             </prosody>
         </voice>
     </speak>
     ```
2. **基线语速出厂优化**：
   - 将 `VoiceFeedbackConfig::default().tts_speed` 从 `1.0` 升级为 `1.3`；
   - UI 保持 0.5x ~ 2.0x 动态可调节，且调节后立即生效。

---

## 3. 对抗式红蓝军审查与边界排查 (Adversarial Grill)

### 攻击点 1：打断时播放“你说”，用户如果连着唤醒词一刻不停说话，会不会被吃字？
* **蓝军抗辩**：
  如果用户一口气喊“Hey Flowy 帮我查天气”，后半句命令“帮我查天气”已经在 KWS 的 `wake_command` 缓冲区中被提取（见 `voice.rs:4467`），系统直接将其送往 ASR/路由器，此时根本不会播放“你说”；
  只有当用户只喊了独立的“Hey Flowy”（无命令尾随，属于口语停顿等待回应）时，系统才会播报“你说”，这完全吻合人类等待回应的心智模型！

### 攻击点 2：如果连续疯狂在首页按唤醒词，会不会产生大量空的无用垃圾会话？
* **蓝军抗辩**：
  Flowy 既有的 `create_provisional_voice_conversation` 拥有严格的“未发送有效文字不落盘”生命周期保护（`voice_lifecycle = "provisional"`）；如果用户只是唤醒后没说话就挂断或超时，该临时会话会在超时后被自动标记为 `discarded` 并安全清理，绝不污染侧边栏会话列表！

### 攻击点 3：Edge TTS 语速如果拉到 2.0x，微软云端接口是否会拒收或报错？
* **蓝军抗辩**：
  微软 Speech Synthesis SSML 标准对 `<prosody rate="...">` 的官方支持范围是 `-50%` 至 `+200%`（即 0.5x 至 3.0x）。Flowy 的前端滑块限制在 0.5x 至 2.0x，完全处于微软云端标准支持的安全舒适区内，不会发生 400 Bad Request。

---

## 4. 分阶段执行计划 (Phased Execution Plan)

| 阶段 | 核心任务 | 交付物与自动化验证 |
| :--- | :--- | :--- |
| **Phase 1** | **TTS 语速全链路透传与 1.3x 基线升级** | `tts.rs` 补齐 Edge SSML prosody 标签，`VoiceFeedbackConfig` 默认 speed 升级为 1.3；单测验证 SSML 格式正确。 |
| **Phase 2** | **会话解耦与新建任务页智能分流** | `recorder.rs` 废除静态单例查表，为首页/新建页分配独立全新会话；前端路由增加切页候选注销；自动化测试验证首页唤醒必新建会话。 |
| **Phase 3** | **打断/唤醒明晰人声回执落地** | 打断挂接预置人声“你说”（中文）/“Go ahead”（英文）；首次唤醒挂接“我在”；验证 `is_playback_active` 0ms 盲区与单测全绿。 |
| **Phase 4** | **多轮独立代码审计与全量质量门禁** | 调用 Opencode CLI 进行深度复查，跑通全量单元与流式测试，重新构建生产二进制。 |

---

## 5. 落地交付与收尾审计闭环记录

经过多轮真实代码重构与 Opencode CLI 深度对抗式代码审计（含变异测试），方案已全部落地闭环：

1. **T52 交付**：在 `EdgeTtsProvider` 中实现 `build_edge_ssml`，动态注入 `<prosody rate='{pct:+}%'>`，使 0.5x~2.0x 动态微调真实生效；将出厂默认语速从 1.0x 升级为对话黄金基准 1.3x。
2. **T53 交付**：在 `stop_session` 与 `mark_active_request_stopped` 清理 candidate 幽灵 ID；在 `nomifun-audio::vocal` 嵌入预固化 16kHz WAV 资产 `barge_zh.wav`（“你说”，448ms）与 `barge_en.wav`（“Go ahead”，489ms），首次唤醒恢复“我在/I'm here”。
3. **T54 交付**：将 `AcousticState::ListeningConfirm` 盲区条件同步升级为 `is_playback_active`，彻底消灭高危工具确认态的 600ms 残留盲区。
4. **T55 交付**：
   - 彻底修复 P1-1：实现 `format_voice_spec` 始终显式透传 user 选定的 `speed`（覆盖 1.0x、1.3x、1.5x 全档位），修复 1.0x 被静默替换为 1.3x 的缺陷，并将 `format_voice_spec` 覆盖 `play_response`、流式 chat、直接/LLM dismiss、任务工单受理工单及总结全路径；
   - 彻底修复 P1-2：移除 `voice_session_bridge.rs` 中绕过 sequence fence 的无条件候选清空，由 `report_window_context` 在顺序屏障下严格托管；
   - 彻底修复 P2-1：`accept_wake` 严格尊崇 UI 契约——`wake_reply` 留空则静音只播和弦，默认短语走预固化人声，自定义短语走 TTS；
   - 补齐 `ListeningConfirm` 盲区消除回归单测（`tests/voice_confirmation.rs` 7 项全过）；
   - 重新构建生产二进制（SHA-256: `422626f2...`，61,198,336 字节），全量 997 项测试全绿。

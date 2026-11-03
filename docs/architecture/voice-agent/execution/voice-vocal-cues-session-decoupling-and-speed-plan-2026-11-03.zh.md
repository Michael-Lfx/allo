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

### 1.4 痛点四：首页主动创建通话未新建仍跑回旧会话、退出后报错 `daemon_wake_unknown`、且点击语言会话被隐式置顶
* **现场现象**：
  1. 用户在首页主动点击通话（或唤醒），依然没有新建语音会话，而是被拉回曾经执行过的旧通话任务继续；
  2. 退出通话后，侧边栏会话列表仍然显示绿色声波状态（未真正释放），再次点击开启时，界面红框报错 `(!) daemon_wake_unknown`；
  3. 用户点击语言任务进入浏览历史时，该会话被隐式修改并强制置顶到侧边栏最开头，打乱了正常的时间线排序。
* **第一性原理与根因剖析**：
  1. **`daemon_wake_unknown` 与绿色声波残留**：
     - 当退出通话后再次点击通话按钮时，宿主向伴侣 `/api/voice/wake` 发起 POST 请求；
     - 守护进程在上一次退出过渡期间状态机若未完全退回 `Idle`，会返回 `VoiceWakeError::Busy`（HTTP 409 Conflict）；
     - 宿主 `wake_rejection_precedes_session()` 过滤函数只认 403 和 503，遇到 409 Conflict 直接当成未知错误并抛出 503 `daemon_wake_unknown`；
     - 前端收到 503 后将 `pendingCallIntent.unknown = true` 滞留在内存中未清除；由于 `activeId` 在未通话时回退到 `pendingCallIntent.conversationId`，导致侧边栏常驻绿色声波（`|||.`）；
  2. **首页创建会话未真正新建**：
     - 正因为前端内存中残留的 `pendingCallIntent` 仍绑死在旧会话，且宿主后端的 `active_window_candidate` 和 `retained_default_call` 存在单例查表与旧候选缓存，导致首页点击依然被旧凭证劫持；
  3. **点击会话自动置顶**：
     - 普通文本会话在前端创建时已有完整的 `execution_model_pool` 和 `reasoning_effort`，打开时纯只读；
     - 而语言会话是由后端伴侣守护进程创建的，初始数据库记录中未包含 `execution_model_pool`；
     - 当你在前端点击进入该语言会话时，`ChatConversation.tsx:299` 和 `NomiSendBox.tsx:480` 检测到缺少模型池，**自动触发了一次隐式的 `ipcBridge.conversation.update.invoke` 尝试补齐字段**！
     - 这一更新操作立刻触发了 SQLite 的 `UPDATE conversations SET updated_at = now() ...`，将该会话的最后修改时间修改为了当前时间，从而被侧边栏重新排序冲到了最顶端！

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
1. **后端 `resolve_target_conversation` 精准双轨分流**（`recorder.rs`）：
   - **普通文本会话降级轨（BV-03 合约）**：当 `source_id.is_some()`（即用户身处某个无工作区的普通文本对话，被唤醒需降级到默认语音对话）时，维持调用 `ensure_default_voice_conversation_with_mode`，保护既有测试用例与“语音助手”单例契约；
   - **首页/新建任务页全新动态轨（用户诉求核心）**：当 `candidate.is_none()` 且 `source_id.is_none()`（用户在首页或点击新建任务，无既有会话上下文）时，**强制生成唯一时间戳凭据 `voice-auto-{now}` 并调用 `create_provisional_voice_conversation_with_locale` 创建全新独立语音会话**，彻底斩断静态单例绑架！
2. **通话终止即注销绑定（清除幽灵 Session）**：
   - 在 `voice_companion_manager.rs` 的 `apply_host_projection` 中：若 `source_snapshot.ended_reason.is_some()`，**强制将 `source_snapshot.binding` 置为 `None`**；
   - 彻底解除旧会话绑定的残留，使底部 Dock 能立即恢复为“待唤醒”吸附态（`waiting-wake`），并防止下次唤醒被旧绑定拉回。
3. **前端切页注销候选**：
   - 在前端路由守卫中，当路由跳转至 `/`、`/guid` 或非会话页面时，向后端广播 `report_window_context`（`conversation_id: None`），彻底清除后端内存中的旧候选残留。

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

### 2.4 模块四：呼叫意图生命周期闭环与语言会话只读防置顶规范
1. **呼叫意图（`pendingCallIntent`）彻底隔离与清理**：
   - 在 `ui/src/common/adapter/voiceCompanion.ts` 的 `reconcilePendingCall` 中，**只有呼叫真正处于正在进行态（`!intent.unknown && !intent.abandoned && intent.inFlight`）时，才允许将未绑定的意图作为活跃会话**；
   - 当收到 `daemon_wake_unknown`、通话明确结束、用户退出或出现网络异常时，**强制调用 `clearPendingCallIntent()` 清除滞留意图**；
   - 彻底斩断侧边栏 `isCallActive` 误读旧会话 ID 造成的绿色声波（`|||.`）常驻死锁。
2. **伴侣守护进程状态机自愈守卫**：
   - 在 `apps/voice-companion/src/voice.rs` 的 `accept_wake` 中增加守卫：若 `events.snapshot().ended_reason.is_some()` 且状态机尚未归零为 `Idle`，**强制执行 `d.reset()` 立即重置到 Idle**，彻底消除退出通话后再次点击时的 409 Busy 报错。
3. **语言会话只读浏览防置顶保护**：
   - 在 `ChatConversation.tsx` 与 `NomiSendBox.tsx` 中，对语音类会话（`isVoiceConversation`）**严格禁止在挂载初始化阶段触发静默的 `ipcBridge.conversation.update` 写入 `execution_model_pool` 与 `reasoning_effort`**；
   - 仅当用户在会话内真正发送消息或执行设置修改时，才允许更新 `updated_at`；打开和浏览历史记录永远保持 100% 只读，绝对不篡改时间戳与侧边栏排序。

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

### 攻击点 4：如果禁止语音会话只读挂载更新模型池，用户临时在输入框键盘打字发送会报错吗？
* **蓝军抗辩**：
  查阅 `NomiSendBox.tsx:334`，当用户在输入框真正敲回车或点击发送按钮时，`nomiSendDispatch.ts` 会在“发送动作”执行的瞬间为当前消息装配执行模型；
  因此，浏览阶段保持纯只读，既完全不影响用户随时打字发送，又彻底保证了侧边栏历史时间戳和排序不被篡改乱跳。

### 攻击点 5：通话结束瞬间以极快手速（<10ms）再次点击电话按钮，会不会产生状态抖动？
* **蓝军抗辩**：
  在伴侣端加上 `if events.snapshot().ended_reason.is_some() { d.reset(); }` 守卫后，只要上一个会话已标记结束，无论其处于哪个过渡状态，后一个唤醒请求都会将其原子归一到 `Idle` 并立即接纳，0 秒时滞，彻底消灭 409 Conflict 与 `daemon_wake_unknown` 报错。

---

## 4. 分阶段执行计划 (Phased Execution Plan)

| 阶段 | 核心任务 | 交付物与自动化验证 |
| :--- | :--- | :--- |
| **Phase 1** | **TTS 语速全链路透传与 1.3x 基线升级** | `tts.rs` 补齐 Edge SSML prosody 标签，`VoiceFeedbackConfig` 默认 speed 升级为 1.3；单测验证 SSML 格式正确。 |
| **Phase 2** | **会话解耦与新建任务页智能分流** | `recorder.rs` 废除静态单例查表，为首页/新建页分配独立全新会话；前端路由增加切页候选注销；自动化测试验证首页唤醒必新建会话。 |
| **Phase 3** | **打断/唤醒明晰人声回执落地** | 打断挂接预置人声“你说”（中文）/“Go ahead”（英文）；首次唤醒挂接“我在”；验证 `is_playback_active` 0ms 盲区与单测全绿。 |
| **Phase 4** | **多轮独立代码审计与全量质量门禁** | 调用 Opencode CLI 进行深度复查，跑通全量单元与流式测试，重新构建生产二进制。 |
| **Phase 5** | **真机实测缺陷终极闭环（针对本轮 4 个现象）** | 修复 `daemon_wake_unknown` 假死意图清除、伴侣端状态机自愈、阻止语音会话挂载静默更新模型池防置顶。 |

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

---

## 6. 终极根治：“首页创建/唤醒语音通话依旧复用旧会话”的多重死锁拆解与确定性方案

用户实测反馈指出了最关键的矛盾：**“首页创建语音通话依旧没有新建，而是跑到旧会话”这个问题修了好几遍，为何依然发生？**

必须从系统全链路第一性原理彻底复盘之前修补的盲点，并拿出不可推翻的确定性系统级闭环解法：

### 6.1 历史修补为何屡次复发？（三大隐蔽盲点复盘）
1. **盲点一（局部修改被测试逼退回滚）**：
   - 先前在 `recorder.rs:346` 尝试将 `ensure_default_voice_conversation` 替换为 `create_provisional_voice_conversation`；
   - 但因为既有测试 `bv_03_busy_active_candidate_degrades_gracefully_to_default_voice_conversation` 明确要求“普通文本会话被唤醒时降级到默认‘语音助手’”，无差别替换导致 `bv_03` 失败断言红灯；
   - 先前为了保住测试，直接把代码粗暴回滚，导致首页/新建页唤醒重新退回到了 `ensure_default_voice_conversation_with_mode` 静态单例查表（`voice:default:{user_id}`）的深坑中！
2. **盲点二（只看创建逻辑，忽略了“旧通话根本没死透”）**：
   - 以为问题只在“创建时找了谁”，却忽略了**上一次通话退出时，旧会话的状态依然在前端和后端游荡**；
   - 守护进程发了 `SessionEnded`，但宿主在 `apply_host_projection` 里依然将旧的 `binding` 拷给了新快照；
   - 前端只要认为旧会话还在通话中，就会触发各种互斥保护与跳回逻辑！
3. **盲点三（前端引导页自带“自动切回”副作用）**：
   - 用户在首页点击电话图标，前端 `GuidPage.tsx` 发送呼叫；
   - 后端因为旧会话未死透判定 `is_active = true` 报出 `voice_call_conflict`；
   - `GuidPage.tsx:506` 收到该错误后，**代码里赫然写着：`await navigate('/conversation/' + errorObj.active_conversation_id)`**，把用户强行拉回了旧通话页面！

---

### 6.2 彻底根除的五大协同闭环方案 (The 5-Point Systemic Fix)

#### 1. 前端引导页：掐断 conflict 时的自动切回旧会话死循环（P0-1）
在 `ui/src/renderer/pages/guid/GuidPage.tsx` 与 `ui/src/common/adapter/voiceCompanion.ts` 中：
```typescript
// 现有问题代码：
// if (errorObj?.code === 'voice_call_conflict' && errorObj?.active_conversation_id) {
//   await navigate(`/conversation/${encodeURIComponent(errorObj.active_conversation_id)}`);
// }

// 修复策略：
// 当用户的明确目标是 NewVoiceConversation 时，遇到 conflict 说明上一个通话尚未完全在后台沉降，
// 绝对禁止强行将用户 navigate 回旧通话页面！
// 应展示轻量非侵入提示，或由用户重试发起全新会话：
if (errorObj?.code === 'voice_call_conflict') {
  Message.warning(t('voice.entry.callConflictStarting', { defaultValue: '上一通通话正在清理释放，请稍候重试' }));
  return;
}
```
*效果*：彻底掐断首页点击电话图标却被前端强行抓回旧任务的死循环通道！

#### 2. 守护进程退出：`SessionEnded` 必须无条件全局广播，严禁被局部轮次丢弃（P0-2）
在 `apps/voice-companion/src/voice.rs` 中，在所有会话终止分支（LLM Dismiss、直接退出、挂断）：
```rust
// 现有隐患代码：
// let _ = event_tx.send_scoped(VoiceCompanionEvent::SessionEnded { reason: UserStop }, &control_scope);
// 当 turn_id 发生轮次交替时，send_scoped 检查 !scope.matches(&state) 直接丢弃事件，导致守护进程从未进入 ended_reason！

// 修复策略：
// 会话终止是整个 Session 级别的终态事件，严禁受限于单一轮次的 turn_id！
// 统一改为无条件全局广播：
let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
    reason: VoiceSessionEndReason::UserStop,
});
```
*效果*：保证守护进程的 `ended_reason` 100% 写入并必达宿主与前端，彻底消灭僵尸会话！同时在 `accept_wake` 中，若上一会话已结束但状态机处于过渡态，原子 `d.reset()` 归一为 `Idle`。

#### 3. 前端呼叫意图：绿色声波精准熄灭，且保留 503 幂等重试凭证（P0-3）
在 `ui/src/common/adapter/voiceCompanion.ts` 的 `reconcilePendingCall` 中：
```typescript
// 只有当呼叫意图真正处于发起在途中（inFlight && !unknown && !abandoned）时，才允许暂时代替活跃 ID：
const pendingActive = Boolean(
  pendingCallIntent &&
  !pendingCallIntent.unknown &&
  !pendingCallIntent.abandoned &&
  pendingCallIntent.inFlight
);
const activeId = isCallActive
  ? (snapshot.binding?.conversation_id ?? null)
  : (pendingActive ? (pendingCallIntent?.conversationId ?? null) : null);
updateActiveVoiceCallConversationId(activeId);
```
*关键边界防线*：**严禁在收到 503 时无脑调用 `clearPendingCallIntent()`**！保持 `pendingCallIntent` 的 `request_id` 以满足网络断线与幂等重试契约（`callIntent.test.ts:192`）；而通过收紧 `pendingActive`，`activeId` 立即归为 `null`，侧边栏绿色声波即刻熄灭！

#### 4. 宿主快照投射与双轨目标仲裁（P0-4）
- **宿主快照展示解绑**：在 `voice_companion_manager.rs` 的 `apply_host_projection` 中，当 `source_snapshot.ended_reason.is_some()` 时，将快照的对外展示 `binding` 置为 `None`，彻底消除前端 `boundElsewhere` 误判，让药丸立即恢复“待唤醒”；
- **目标仲裁精准分流**：在 `recorder.rs` 的 `resolve_target_conversation` 中精细区分：
  - 若 `source_id.is_some()`（从既有普通文本会话唤醒）：维持 `ensure_default_voice_conversation_with_mode`，保护 `bv_03`、`review_standalone_binding` 等既有测试用例；
  - 若 `source_id.is_none()` 且 `candidate.is_none()`（首页/新建页无既有会话）：强制调用 `create_provisional_voice_conversation_with_locale`，采用 `nomifun_common::generate_id()` 动态分配全新会话并传入正确 `locale`；
  - 并在会话结束无有效发言时，由 `reconcile_empty_placeholders` 进行清理，防止磁盘和数据库残留。

#### 5. 语言会话只读保护：精确拦截 3 个静默更新 Effect（P0-5）
在 `ui/src/renderer/pages/conversation/components/ChatConversation.tsx` 与 `NomiSendBox.tsx` 中：
- 拦截 1：协作模型池自动回填（`ChatConversation.tsx:290-315`）；
- 拦截 2：模型缺失自动修复（`ChatConversation.tsx:432-494`）；
- 拦截 3：推理深度自动归一化（`NomiSendBox.tsx:476-497`）；
对于 `isVoiceConversation` 会话，只读挂载阶段直接 return 跳过更新写入，绝不篡改 SQLite `updated_at`，**会话在列表中纹丝不动，彻底消灭乱跳置顶问题！**

---

### 6.3 自动化测试验证防线
1. **防回跳单测**：验证 `GuidPage` 在遇到 `voice_call_conflict` 时绝不导航回旧会话；
2. **退出事件必达单测**：验证 `voice.rs` 退出通话时 `SessionEnded` 必被触发，`ended_reason` 必不为 None；
3. **绿色声波熄灭单测**：验证 `reconcilePendingCall` 在收到 503 或未在通话时 `activeVoiceCallId` 必为 `null`；
4. **既有集成测试回归**：确保 `bv_03`、`review_standalone_binding`、`voice_streaming` 100% 绿灯；
5. **只读挂载防置顶单测**：断言打开语音会话后 `updated_at` 不变，`voice_lifecycle` 不被翻为 `retained`。

---

### 6.4 对抗式终审裁决与时序加固落地 (H1/H2/M1/M5 闭环)

在全量代码实施后，调用 Opencode CLI 进行了针对实际落地的对抗式代码审查，深度闭环了以下 4 项边缘隐患：

1. **H1 落地：保护首页工作区候选（`useVoiceContext.ts`）**：
   - 保留 `else if (pathname !== '/guid')`，使 `/guid` 首页候选由 `GuidPage.tsx` 独占上报，避免父组件全局 hook 在 `focus`/`visibilitychange` 时将用户选中的项目目录覆盖为 null。
2. **H2 落地：消除全局 SessionEnded 跨 await 误杀新会话（`voice.rs`）**：
   - 在直控 Dismiss（第 2580 行）与 LLM Dismiss（第 3068 行）中，重新获取 driver 锁后增加代际与会话防线：
     `if playback.generation_id() != sleep_gen || event_tx.snapshot().session_id != control_scope.session_id { return; }`
     若在 `interrupt().await` 期间已有新的唤醒或呼叫接管，旧会话终止逻辑立即放弃，彻底消除新会话被误杀的竞态。
3. **M1 落地：细分 `voice_call_starting` 与 `voice_call_conflict`（`GuidPage.tsx`）**：
   - 处于预留释放时提示“上一通通话正在清理释放，请稍候重试”；真实冲突时提示“当前已有正在进行的语音通话，请先挂断”，绝不自动导航。
4. **M5 落地：意图清理同步归零活跃通话 ID（`voiceCompanion.ts`）**：
   - `clearPendingCallIntent` 同步执行 `updateActiveVoiceCallConversationId(null)`，挂断当刻侧边栏绿色声波 0ms 同步熄灭。

---

### 6.5 顽疾深层物理病灶彻底切除与专项集成测试（T58 终极闭环）

针对用户实测中反复复发的“在首页/会话B唤醒依然跳回会话A”以及“点击语言会话被置顶”，实施了物理级根治：

1. **切除桌面桥接假校验（`apps/desktop/src/voice_session_bridge.rs`）**：
   - 彻底删除在 Windows WebView2 架构下对 `window.url().fragment()` 的比对校验。因客户端虚拟 Hash 路由（`#/conversation/B`）不会触发底层的 Source URL 刷新，原代码误判为 `voice_context_stale` 将会话 B 的上报拒收；切除后只要窗口处于可见焦点，直接采信前端报告，使 `active_window_candidate` 100% 更新为当前会话 B。
2. **首页默认会话有历史消息时强制新建（`recorder.rs:346`）**：
   - 检查默认会话 `home` 是否已有消息记录（`home_has_messages`）。若已存在聊天记录，首页唤醒绝对禁止再次复用该历史会话，强制动态分配全新 UUID 生成独立会话（会话 C、D…）。
3. **彻底阻断气泡积分回填隐式写库（`MessageText.tsx:452`）**：
   - 语音会话的气泡由离线管道直接写入，缺少云端 billing 积分。原先打开时 `MessageText.tsx` 会在 400ms 后自动触发 `fetchAndPersistTurnCredits` 并调用 `ipcBridge.conversation.update`，修改 SQLite `updated_at = now()` 导致列表被置顶。
   - 现增加语音会话守卫，挂载阶段严禁触发更新，保持浏览 100% 纯只读，时间戳稳固不变。
4. **端到端专项集成测试（`voice_dialogue_integration.rs:1232`）**：
   - 新增 `test_full_wake_routing_and_conversation_switch_resilience`：真实模拟 A 产生历史对话 -> B 唤醒留在 B -> 首页第 1 次唤醒新建 C -> C 产生对话 -> 首页第 2 次唤醒新建 D，且断言 A 的 `modified_at` 绝不被篡改。51 项集成测试全绿！

---

### 6.6 对话模型自动继承与会话内唤醒绝对就地接续（T59 终极闭环）

根据真实用户实测与 Opencode 对抗审查终审，进一步完成以下闭环：

1. **新建语音会话自动继承对话模型（`recorder.rs:360-376`）**：
   - 首页唤醒新建 provisional 语音会话时，完整继承默认语音会话 `home` 中用户配置的 `model`；若 `home` 尚未配置模型，通过 `VoiceTaskRuntime::prepare_configuration` 兜底解析系统出厂默认模型，预设（`preset_id`）按设计保持为 `None`，彻底解决首句发言报 400 `Nomi conversation has no provider/model configured` 的致命缺陷。
2. **物理删除多余模型操作按钮（`NomiSendBox.tsx:2220-2228`）**：
   - 物理移除了模型不可用警告条中的“重试”与“前往模型设置”按钮，文案收敛为纯文本提示，消除冗余交互干扰。
3. **会话内唤醒提升为第一绝对优先级（`recorder.rs:193-212`）**：
   - 将 `if let Some(id) = candidate` 提升至 `explicit_workspace` 之前。若当前查看的会话是未废弃的语音会话（`is_voice_conv && !is_discarded`），100% 触发 Rule 1 原地返回该会话继续对话，绝不再被工作区分支旁路跳过；
   - 移除名字字符串启发式，与前端严格持久化标记对齐。
4. **消除通话挂断时抹除窗口候选的死穴（`voice_companion_manager.rs:1341/2437`）**：
   - 彻底删除 `stop_session` 与 `mark_active_request_stopped` 处的 `self.set_candidate_conversation(None)`。用户在当前会话页面挂断后，窗口候选依然保留，再次喊唤醒词时 100% 原地接续，彻底杜绝“挂断后再次唤醒又建新会话”。
5. **回归测试全量锁定（`tests/voice_dialogue_integration.rs` & `tests.rs`）**：
   - 断言首页新建会话必有对话模型；
   - 断言打开语音会话即使带工作区也必原地接续；
   - 增加 manager 级单测 `stop_call_preserves_candidate_conversation_for_in_place_rewake`。全量测试全线绿灯！

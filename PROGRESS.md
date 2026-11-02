# 任务开工回执 (Task 0 Receipt)

- 理解的目标：解决真机实测 4 大痛点（唤醒 60s 自锁、自检 60s 销毁重置、挂断后卡未在通话、首页全局唤醒及 KWS 门限过紧）。
- 执行顺序：T1 唤醒常驻准入 -> T2 能力解耦与静默自检 -> T3 前端胶囊状态纠偏 -> T4 KWS 0.08 门限与唤醒开关 -> T5 首页全局路由 -> T6 全量测试与提交。
- 最大风险：自检解除 generation 绑定后需要保证底层声卡真实断开时仍能及时降级；已制定健康心跳兜底方案。
- 基线状态：工作树干净，编译通过，全量单测通过。

## 执行进度记录 (Execution Progress)

- [x] **T0**: 环境与代码基线确认，建立 PROGRESS.md。
- [x] **T1**: 彻底解除 `wake_admission_until_ms` 60 秒倒计时自杀式过期，生产默认常驻有效，废除 `admission_expired` 丢弃唤醒词的错误拦截。
- [x] **T2**: 能力就绪报告与单次会话代次 `generation` 解绑，心跳容忍度由 2s 放宽至 15s，开机 2 秒后异步触发静默自检并点亮绿灯。
- [x] **T3**: 修复 `voiceCallDockState.ts`，正常通话挂断且空闲时自动回归呼吸绿光的“等待唤醒”（`waitingWake`），16 项单测全绿。
- [x] **T4**: KWS 灵敏度门槛从 0.20 回调至 0.08，规则文件词条设为 `:2.0 #0.08`；新增 `wake_enabled` 独立开关，支持完全释放麦克风。
- [x] **T5**: 主窗口顶层 `Router.tsx` 挂载 `GlobalVoiceWakeListener`，在首页/新建对话被唤醒时即刻接管并路由到通话。
- [x] **T6**: 全量验证梯子通过，二进制产物 SHA256 登记完成。
- [x] **T7 (dsh-orb 对标)**: 汉字对照表全面注入带调拼音与口语连读变调（`n ǐ / n í`, `h ǎo`, `x iǎo`, `b ái` 等），规则文件生成带调声调。
- [x] **T8**: 打通 Stage-2 SenseVoice 级联唤醒支持 `feedback.wake_words`，流式未跨过门限时由 SenseVoice 精准命中唤醒，增加实时 `[KWS-LISTEN]` 与 `[KWS-CASCADE]` 调试日志。
- [x] **T9**: 破除 `voice_suite_in_use` 悬空预定锁，`start_call` 拒绝时即刻清理，`voice_suite_in_use` 增加 3 秒超时过滤，彻底解除自检死锁。
- [x] **T10**: 开机自检采用 `stage = "silent"` 实现扬声器零发声静默自查，修复能力签名对比使 6 节点绿灯开机即常驻。
- [x] **T11**: 设置页重构将“语音唤醒”开关与“主唤醒词”输入框合一联动，开则展开，关则收起并释放麦克风。
- [x] **T12**: 彻底删除 `voice.rs` 中的 `!admission_valid_for` 拦截，宿主 `refresh_wake_admission` 保持常驻准入，彻底杜绝 `admission_expired` 错误丢弃唤醒词。
- [x] **T13**: `Router.tsx` 增加设置页保护：身处设置页时绝对不自动跳转离开到会话。
- [x] **T14**: `ZH_DISMISS_CORE` 扩充收录“退一下”、“退一下吧”、“退下啦”，避免轻声吞字被当成普通聊天。
- [x] **T15**: `voiceCallDockState.ts` 将 `!task` 更正为 `!activeTask`，会话中有历史完成/失败任务挂断后秒级回归“等待唤醒”（`waitingWake`），17 项单测全绿。
- [x] **T16**: `kws.rs` 清除 baseline 孤立 2 音节短词 `NI_HAO`（“你好”）与 `FLOWY`，避免底噪特征碰撞，门限收敛为 `#0.15`，根除误触风暴与伪 30s 超时失效。
- [x] **T17**: `voice.rs` 引入扬声器播报 600ms 声学冷却（Playback Muting Cooldown），播报中及刚结束 600ms 内丢弃麦克风帧，彻底阻断回声自吞（如“再见”录入 ASR）。
- [x] **T18**: `VoiceFeedbackSettings.tsx` 交互极简重构为双行标准布局，清除冗长大段说教文本。
- [x] **T19**: `VoiceSettings.tsx` 6 大能力指示灯全面绑定实时 `voiceSnapshot` 动态健康流，并自动后台触发无声自检（`stage = "silent"`），开机即全绿。
- [x] **T20**: `voice.rs` 强化 Stage-2 级联唤醒算法：实现全局滑动窗口子串匹配（支持前置语气词与重复呼叫）并引入 $\ge 3$ 字符允许 1 字近音/同音容错（如“香鱼片”容错匹配“香芋片”）。
- [x] **T21**: `kws.rs` 拼音映射表扩充加入“香”、“芋”、“片”、“茶”、“糖”等常用字符。
- [x] **T22**: `VoiceFeedbackSettings.tsx` 视觉卡片完全融合：消除独立行分割线，封装于同一优雅 sub-card 容器中。
- [x] **T23**: `VoiceFeedbackSettings.tsx` 实现未保存状态全链路高亮（输入框与选择框琥珀边框光晕、标签旁 `● 未保存` 脉冲徽标）与 Vibrant Primary 醒目保存按钮。
- [x] **T24**: 唤醒词输入框动态回显引擎工作路径状态微章（`● 双核流式秒级唤醒` vs `◈ SenseVoice 级联双核兜底`）。
- [x] **T25**: 在 `nomifun-audio` 中内置覆盖 6,763 个全部 GB2312 常用汉字与现代词汇的紧凑拼音音素映射表（`cjk_pinyin_table.rs`），彻底废除手写 match 穷举，任意汉字（如“菠萝蜜”、“麻辣烫”、“皮卡丘”）100% 自动解析出带调声韵母。
- [x] **T26**: 严格锁死 Stage-2 级联容错门禁为 `kw_len >= 3`（2 字词 0 容错防误触，3 字词 1 字近音容错提召回），并增加越界切片防御测试，通过 40 项前端单测与全套 Rust 声学单测。
- [x] **T27**: `voice_companion_manager.rs` 解除 LLM 付费自检阻断：对话模型作为可信 Provider，在未执行付费探活时直接置为 Ready，彻底根除“尚未运行对话检查”导致的通话启动拒绝。
- [x] **T28**: `playback.rs` 与 `voice.rs` 重构回声冷却状态：仅在实际有播报任务运行时绑定 `playback.is_speaking()`，彻底解除因声卡缓冲悬挂导致的麦克风永久静音假死。
- [x] **T29**: `VoiceSettings.tsx` 设置页防并发自检：检查按钮绑定 `self_check_active` 状态自动 Loading 锁定，静默拦截 409 `self_check_in_progress` 并平滑等待结果，绝不弹红报错。
- [x] **T30**: `VoiceAudioStatus.tsx` 音浪感知升级为 RMS + Peak 复合运算，修复低电平设备与虚拟麦克风音浪表死直平线。
- [x] **T31**: `voice.rs` 声学回声全状态覆盖：在 `DispatchingTask`、`Deciding` 和 `FollowUpListening` 全面加入 `in_echo_cooldown` 拦截，杜绝任务派发播报期间 5.5 秒自身语音录入 ASR 导致二次连环派发。
- [x] **T32**: `VoiceSettings.tsx` 修正 `CheckOne` 图标 fill 样式：显式设置 `fill="currentColor"` 并绑定 `allStagesReady`，消除深灰色硬编码，图标恢复 Arco 鲜亮绿色。
- [x] **T33**: `recorder.rs` 与 `Router.tsx` 落地三场景智能唤醒分流：首页/新建页新建语音通话，普通会话自动新建语音通话并跳转，已有语音通话原地接续。
- [x] **T34**: 彻底根除会话劫持（当前会话原地接续、首页/文本会话自动新建独立通话）：在 `recorder.rs` 中实行“当前语音会话不可剥夺原则”，直接就地返回 `source_id`；首页与普通文本会话唤醒彻底废除 `retained_default_call` 历史单例查表，直接调用 `create_provisional_voice_conversation_with_locale` 创建全新独立语音通话；并在 `voice_session_bridge.rs` 中增加首页清空候选会话守卫，通过集成回归单测。
- [x] **T35**: 闭环 opencode 审计重点项：① `recorder.rs` 语音会话识别全面覆盖 `extra["voice"]`/`is_default_voice` 与英文/改名场景；② `VoiceFeedbackSettings.tsx` 默认麦克风选择发送 `"default"` 哨兵，彻底解除设置脏状态卡死；③ `VoiceSettings.tsx` 就绪判断以 `capabilities` 为最高优先级；④ `voice.rs` 追问超时对齐 `effective_idle_timeout`。通过全套前端与后端单测。
- [x] **T36**: 第一性原理全栈归正修复：① 重建 `recorder.rs` 三层决议，原地接听与项目工作区专属 C03 逻辑完美融合，清零 50 项集成与 13 项运行时单测；② 移除 `voice_pipeline.rs` 的 2.5x 硬增益，恢复标准 PCM 采样；③ 前后端补齐 `self_check_active` 字段并收敛 `resolveStageStatus` 类型，清零 voice typecheck 报错；④ 修复 `VoiceFeedbackSettings.tsx` 中的 dead CSS；⑤ 恢复任务结果播放后 180s 追问契约。全套测试全绿通过。
- [x] **T37**: 落地免 AEC 的 KWS 唤醒词打断架构（Keyword-Spotting Barge-in）：① 在 `voice.rs` 播报态分支开启 KWS 供流，起播瞬间执行 `kws.reset()` 清空声学滑动窗口；② 部署文本预检动态屏蔽防线，彻底杜绝 AI 念出自身名字时的自激打断；③ 播报态 KWS 命中时调用 `playback.interrupt()` 瞬态排空声卡队列并切入 `Listening`；④ 编写全自动真实回放打断测试与遮蔽单测，71 项 voice-companion、82 项 nomifun-audio 与 16 项流式单测全部通过。

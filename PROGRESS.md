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

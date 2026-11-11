# R0–R1 声音反馈实施记录

> **状态**：🔧 进行中；用户已授权无人值守推进全链路，2026-10-14。  
> **完整目标**：按照PRD推进无人值守任务，按推荐方案打通全链路，最后交付Owner验证实测。声音反馈、真实runtime、控制/确认、恢复与原安装验收均保留，不把本轮部分实现当最终完成。  
> **当前入口**：[R声音计划](R-auditory-feedback-plan.zh.md)、[原V0–V8](vertical-execution-plan.zh.md)。

## 1. 执行基线与失败证据

核对当前工作区仍为 `feat/xiaozhi-voice`，HEAD `50f8ecb86070a9807ed57704ace070cc6748422e`，领先40提交；所有未提交实现/文档保留，没有重置或提交。

测试沿用户已经授权的生产worker、文本HTTP、输出AudioSink及快照作用域接口进行；不在生产加入模型/账户/音频替身。

| 失败测试 | 结果 | 覆盖的真实缺口 |
| --- | --- | --- |
| [R0-transition-red.log](R0-transition-red.log) | 失败：识别成功音0次，期望1次 | 经PCM→生产worker→模型接口到合成请求，原文有内容但无识别反馈 |
| [R0-earcon-red.log](R0-earcon-red.log) | 失败：没有进入输出消费等待 | 既有play_earcon仅入队，不能提供真实完成锚点 |
| [R1-reply-cancel-red.log](R1-reply-cancel-red.log) | 失败：停止后旧回复drain仍占用输出 | 增加共享输出所有权后，取消必须同时结束旧消费等待，否则下一提示阻塞 |

与资产损坏、ASR没有文字的替代解释相比，失败输入已产生有效原文，已有静态WAV亦有效；反馈缺口由事件未接线和入队语义造成。这里没有调整ASR模型/关键词或录音设备来绕开缺口。

## 2. 已实施的纵向路径

- `apps/ai-mate/src/voice_feedback.rs` 统一当前单次状态音，daemon发声，按generation及会话/轮次防止旧声音污染新会话。
- 有效ASR文字（剥离唤醒词后仍非空）响Guide家族识别音，与对话模型是否可用分开；不增加模型请求。
- TTS消费完成后响一次追问开启音，随后发布追问窗口；TTS失败而输出可用时，本地错误及追问音保留文字降级。
- 追问静默和初次倾听watchdog出口接入结束音；无PCM使用错误音。Ignore与失败出口通过统一反馈记录，不再仅依赖页面phase。
- PlaybackPipeline用现有Tokio锁串行写入/消费，网络合成阶段不占输出锁。提示音消费等待有2秒安全上限，回复消费有60秒上限；取消检测每10ms检查generation，释放旧输出所有权。超时清理自身输出，不让旧请求flush新回复。
- 公共DTO追加可选feedback诊断；记录Pending/Started/Completed/Cancelled/Failed、scope、request_id及脱敏原因。老快照缺字段仍可读取；宿主沿原投影传递，不新增权限、数据库或依赖。

Wake声改为异步调度，避免在内部KWS接受路径等待提示播放时停止消费麦克风。软件Completed只证明输出端消费回执，不证明物理声音可听。

## 3. 当前验证

- [R1-transition-green.log](R1-transition-green.log)：库5项、完整流程7项、原会话9项、播放/路由12项，共33项通过；在线测试默认ignored。完整流程验证识别音一次、Wake与追问开启各一次、追问结束Sleep一次、错误音零次和单次模型请求。
- 强化最后一项为等待Sleep的Completed回执，不以页面已Idle替代输出完成：[R1-transition-receipt.log](R1-transition-receipt.log)。
- 生产编译检查与真实KWS/VAD/ASR→测试模型→在线Edge回放的最终结果，按 [R1-production-check.log](R1-production-check.log) 与 [R1-real-acoustics-edge.log](R1-real-acoustics-edge.log)结束状态追加；不得把正在执行记为通过。
- 本轮没有前端变更、设备录音或物理播音；沒有更新默认开发二进制，Owner仍运行Q轮产物。新增诊断在前端的显示属于后续R4。

## 4. 剩余门禁及接续点

R1仍为进行中，未签核完整出口：初次六秒期限与Wake完成锚点、提示期间已开口时的期限撤销、追问期间重叠讲话、正常静默与捕获故障区分、输出失效/取消竞态都需继续生产入口测试。

随后按R2回声/取消、R3轻量等待、R4设置诊断、R5真实设备及安装、原V5–V7/R6任务与确认推进。保持单输出/单采集、无假接单、原文/理解/回答、任务幂等身份与一个审批权威。尚未完成三轮模型上下文、跨页/恢复/性能与安装门禁的完整审计。

不把33项定向测试宣称为全部16项AF或原20项AC通过。用户已授权持续实施，接续点不需要再请求是否启动；最后再交付完整产物和Owner实测剧本，实机签核仍由Owner完成。

## 5. 原始授权与历史

当前用户目标原文：

> 按照prd推进执行无人值守任务，按照推荐方案把全链路打通，最后再给我验证实测

上一目标轮分类为进展：已经新增完整PRD、技术方案、执行计划及校验记录。当前轮新增了生产实现和能够捕获真实缺口的失败/成功测试，同样为进展；整体目标保持active，不标完成或blocked。原计划“待下一实施指令”的文字保留历史，本次授权已启动实施。

## 6. 本轮补充：六秒期限与最终验证

- [R1-listening-budget-red.log](R1-listening-budget-red.log)复现Wake未消费完成时已存在六秒deadline。现由driver先暂停该期限，提示消费完成再开始有效六秒，并投影ListeningReady；用户在提示期间开始讲话则不重新启动期限。持续输入不等待播放完成。
- 两项生产worker测试覆盖上述完成锚点及播放期间真实PCM消费/人声检测；最终 [R1-listening-budget-green.log](R1-listening-budget-green.log) 共35项通过（库5、完整流程9、原会话9、播放12）。原33项记录保留为前一实现状态。
- 第一次装配ListeningReady时误加到了phase枚举，导致编译错误；已改到真实事件枚举。中间编译日志 [R1-listening-budget-assembly-failed.log](R1-listening-budget-assembly-failed.log) 保留，不把它当作行为失败或隐藏验证记录。
- 增加deadline接线前，生产检查退出0，真实KWS/VAD/ASR与Edge回放通过（19.87秒、24kHz/71424样本、模型替身一次、零输入溢出）；最终deadline接线后的生产编译结果以 [R1-final-production-check.log](R1-final-production-check.log) 的结束状态为准。模型/输出消费仍是测试控制，不代替实际扬声器。
- 当前代码定向Rust格式和diff空白检查退出0；未修改前端，未部署默认二进制。R1接续点缩小为：追问的四秒期限使用同一driver权威、提示期间已经开口不重新计时、初始正常静默与捕获错误区别，以及停止/输出失效竞态。后续R2–R6和原完整目标不变。

2026-10-15接续确认：最终生产 `cargo check --locked -p ai-mate -p Flowy` 退出0（2m09s），没有开启voice-skeleton。低音量/连说/迟开口等八项真实模型回放正在复查，结果以 [R1-standard-audio-regression.log](R1-standard-audio-regression.log)结束状态判断，不先签核。

最终八项真实模型回放全部通过（67.40秒），包括低音量、连说/相邻指令、六秒内迟开口但发言超过期限、静默退出、直接ASR、KWS正样本和负对照；上述“正在复查”保留为中间状态。本轮目前为35项定向回归、8项真实声学回放及1项先前在线Edge整链回放通过，生产编译/定向格式/diff检查通过。最终deadline接线之后未再次请求在线Edge，也未做麦克风/扬声器或安装验证；这些证据不合并为完整离屏门禁。

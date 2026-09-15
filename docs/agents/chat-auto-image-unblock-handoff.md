# 对话框图片发送:移除前端拦截,统一走后端 image_analyze 自愈链

## 交接状态

- 日期:2026-09-15
- 工作分支:`feat/chat-auto-image-unblock`(从 main `f0e33897d` 切出)
- 当前状态:已拆分两个 feat commit 提交(共 16 文件,+68/-246),未推送、未创建 PR;自动化验证全绿,实机验证待做(见末节清单)。
- Commit 拆分:`73f993dc0` 发送路径松绑(NomiSendBox/useGuidSend/GuidPage/CSS/structure test,5 文件)→ `87efd82ab` 选择器禁用机制移除(逻辑层/组件/i18n/探针/单测,11 文件);每个 commit 中间态均通过相关测试与 typecheck。
- Git 约束:commit 无 AI 署名;hooks 正常通过;回退 = 按序 `git revert 87efd82ab 73f993dc0`(或整体 reset 到 main)。

## 背景与问题

旧行为自相矛盾:

- **Auto 族模型**:前端硬拦截携带图片的发送(toast "自动模型当前仅支持文本" + 输入框上方常驻警告条),模型选择器在有图片附件时整体禁选 Auto 族与无 `vision_input` 的文本模型;
- **非 Auto 纯文本模型**:前端不拦发送,后端已有完整兜底——`image_analyze` 内部工具把图片交给独立视觉模型产出文字分析注入正文,文本模型即可"读图"不报错。

两类模型在后端走同一条兜底链,前端拦一个放一个没有依据;且 Auto 定位"省心自动",禁图违背定位。

## 决策记录(为什么是这个方案)

**已选:纯前端松绑(方案①)** —— 删除前端全部 auto/图片拦截,完全依赖后端自愈链。

否决的替代方案:

1. **方案②(前端松绑 + 后端 catalog traits 主动预判,首轮直接走读图)**:对 Auto 是负优化——静态标记 `supports_image=false` 等于永远放弃"网关路由到视觉模型直传"这条最优路径;且引入 traits 数据质量风险(实际支持视觉但未标注的模型会被误降级为读图模式,用户无感知)。它解决的问题(每模型每进程首次带图多一次被抑制的失败往返)在用户侧几乎不可感知,却动工厂层,风险大于收益。定位为后续可选的纯性能优化。
2. **方案③(服务端路由层,auto 带图优先路由视觉模型)**:服务端不可干预,硬约束排除。
3. **维持现状(Auto 禁图)**:与后端能力直接矛盾,违背 Auto 产品定位。
4. **半套松绑(只删发送拦截、保留选择器禁选)**:同一张图能否发送取决于操作顺序,比现状更费解;目标要求选择器语义一致。
5. **客户端预分析(先把图发给分析接口再拼文本)**:`image_analyze` 是会话轮内部的宿主协调工具,无独立对外接口;造平行链路属重复建设。

关键认知:**被动自愈是 Auto 场景的唯一正确机制**。Auto 的真实模型由网关动态路由,客户端无法预知;直传成功即最优路径(原生多模态),失败才降级读图。

## 后端自愈链机制(本次的事实基础,改动未触碰)

1. 模型 `supports_image` 默认 None → 按 true 处理 → 图片直传网关(`crates/agent/nomi-config/src/compat.rs:67`)。
2. 上游 400 且错误文案命中签名(`crates/backend/nomifun-ai-agent/src/protocol/send_error.rs:679-702`,如 "does not support image"、"unknown variant `image_url`")→ 映射为 `UserLlmProviderImageUnsupported`。
3. 该错误被 pre-response 抑制器吞掉,用户不可见(`crates/backend/nomifun-conversation/src/service.rs:10816-10822`)。
4. `strip_images_and_rebuild`(每轮一次,`service.rs:11005-11049`):标记进程内存 `VisionUnsupportedRegistry`(唯一写入点 `failover_seam.rs:476`)→ 终止 runtime → 同模型剔图重建。重建时工厂读 registry 注入 `supports_image=false`(`provider_config.rs:28-34,129`)→ 解析 image_analysis_model(`factory/nomi.rs:441`)→ 持久化居中 tips"当前模型不支持图片输入,已自动移除图片并重试。"(`message_persistence.rs:9-16`,硬编码中文无 i18n)。
5. 重跑及同进程后续带图发送:`agent.rs:1626-1648` 调 `analyze_image_blocks`(`manager/nomi/image_analyze.rs`)——宿主协调的内部工具,不暴露给主模型;专用 system prompt 支持 OCR/VQA/计数/定位,产出 <600 词分析,带缓存(128 条)/分批(3 张一批,并发 2)/重试(2 次)/60s 超时;结果以 `[Untrusted image observations...]` 文本注入正文,图片本体剔除(`agent.rs:1722-1727,1751-1755`);分析失败注入占位说明,不炸会话。
6. Auto 档位模型 id(如 `AIPC-auto-balance`)经 `toApiModel` 原样持久化、原样作为 registry key,写读两侧一致——自愈对 Auto 确认可行。
7. 分析模型来源:用户配置 `tools.imageAnalysisModel`(模型 Hub → Global Model Config → image-analysis tab,或设置弹窗 → 系统分组;`ui/src/renderer/pages/modelHub/ImageAnalysisModelContent.tsx`);未配置则自动扫描(DeepSeek Vision > MiniMax-M3 > Kimi,排除托管免费模型,要求 enabled+healthy+chat+vision_input,`factory/nomi.rs:1169-1256`);扫描宽容返回 None,agent 构建照常成功。
8. 无可用视觉模型时:发送报 BadRequest → 包 "Invalid parameters: " 前缀 → 分类为 `USER_AGENT_INVALID_PARAMS`(retryable=false)→ 前端 `MessageTips.tsx` 泛化错误卡(zh-CN "所选 Agent 拒绝了请求"),真实英文原因在诊断摘要行,带反馈入口。

## 已完成的修改(全部在 ui/,拆分在两个 commit)

### 逻辑层(commit `87efd82ab`)
- `ui/src/renderer/utils/model/chatModelPicker.ts`:删除 `ChatModelPickerOptions`、`withAttachmentRestriction()`、`ChatModelOption.disabled/disabledReason` 字段;`buildChatModelPickerViewModel`/`allChatModelOptions`/`findChatModelOption` 收窄签名去掉 options 形参。**保留 `supportsVision` 字段**(catalog 元数据投影,ButtonLayoutProbe fixture 仍用)。

### 会话页
- `NomiSendBox.tsx`:删 `hasImageAttachments`/`autoModelHasImageAttachments` 派生变量;`canSendModelFiles` 删 auto 分支、保留同名 wrapper 委托 `canSendImageAttachments`(数量上限,structure test 锁定该符号);删 onSendHandler/onSendWithSkillsHandler 两处拦截、警告 Alert(`nomi-auto-image-warning`);移动端 sheet 区清理 auto 否决、visionRequired 描述、auto 项与档位禁用;删 AutoTierSelector/NomiModelSelector 传参。

### 首页
- `useGuidSend.ts`:删接口字段、解构默认值、发送拦截、依赖数组条目。
- `GuidPage.tsx`:删派生变量、Alert(`guid-auto-image-warning`)、三处传参;`selectedChatModelOption` 去掉第 4 参;删孤儿 import(`isImageAttachment`、`Alert`)。
- `index.module.css`:删孤儿类 `.guidAutoImageWarning`。

### 组件层
- `ChatModelPickerMenu.tsx`:删 prop、auto 行禁用与 tooltip;`handleMenuItemClick` 与 modelRow 不再消费 `option.disabled`(字段已删)。
- `AutoTierSelector.tsx`:删 prop、档位禁用析取项、弹层底部提示条。
- `NomiModelSelector.tsx` / `GuidModelSelector.tsx`:删 prop 与透传;GuidModelSelector 的 `findChatModelOption` 去掉 options 实参。

### i18n
- 删 `conversation.modelPicker.autoTextOnly` 与 `visionRequired`(zh-CN/en-US `conversation.json`);`bun run gen:i18n` 重新生成 `i18n-keys.d.ts`(生成物,勿手改;`check:i18n` 通过)。

### 测试与探针
- `chatModelPicker.test.ts`:"有图禁用文本模型"用例重写为"有图也不禁用任何模型";删 `withAttachmentRestriction` 整例;Auto 元数据用例去 disabled 断言。
- `AutoTierSelector.structure.test.ts`:删 autoTextOnly 断言;"guards the Nomi and Guid image-bearing send paths" 整例重写为 `relies on the backend image self-healing chain instead of frontend send gates`——保留 canSendModelFiles 正向锁定,新增反向锁定(NomiSendBox/useGuidSend/GuidPage 不得再出现 autoModelHasImageAttachments/autoTextOnly/警告 testid);顺带修正两条与源码漂移的预存断言(`popupVisible={effectivePopupVisible}`、`aria-expanded={effectivePopupVisible}`,main 上即红)。
- `ChatModelPickerMenu.structure.test.ts`:删 hasImageAttachments 断言,新增"菜单不得携带附件门禁"反向锁定。
- `ButtonLayoutProbe.tsx`:删 `auto-image-disabled` 场景及相关 prop/传参。

## 验证结果

- 相关 3 个测试文件:11 pass / 0 fail;`check:i18n` 绿;改动文件 typecheck 零错误;13 个关键词零残留 grep 干净。
- `bun run test:ui` 全量:Bun 1.3.14 段错误崩溃(posthog-js 环境错误触发),**已对照 main 基线确认为预存环境问题**;main 基线另有 3 个预存失败(AutoTierSelector.structure 两条漂移断言 + notification facade 契约),本分支只剩 notification facade 1 个,另修复了 2 个。
- `check:button-layout-contract`:MeetingPage.tsx:98 预存失败,与本次无关。
- 保留不变:`MAX_IMAGE_ATTACHMENTS = 10` 数量上限及全套数量检查;其他 `family === 'auto'` 命中(策略槽/标签显示)不动;后端 crates/ 零改动。

## 待实机验证清单(自动化覆盖不了)

1. **Auto 档位附图发送**(最关键):应观察到短暂处理 → 自动重跑 → 居中 tips"已自动移除图片并重试" → 正常回答。若直接报错,抓网关 400 文案与 `send_error.rs:682-692` 签名比对,不命中需后端补一行签名(不在本分支)。
2. 同一会话第二次发图:应直接走读图,不再 400。
3. Auto 纯文本发送回归;cloud 视觉模型附图直传(无 tips);cloud 文本模型附图兜底回归。
4. Guid 首页 Auto+图片提交;移动端 sheet 有图时 Auto 族/文本模型可选可切档;桌面菜单 auto 行不再禁用。
5. 清空 `tools.imageAnalysisModel` 后 Auto+图片 → 泛化错误卡 + 诊断摘要含真实原因。
6. 图片数量上限(>10 张)仍拦截;粘贴/拖拽/附件按钮三种添图方式各模型下不被拦。
7. 应用重启后 Auto 首次带图:确认重新试错的延迟可接受(registry 进程内存不落库)。
8. **静默吞图排查**:若网关不报错但忽略图片,模型会在没看到图的情况下回答且无任何提示——被动模式固有盲区,只能实测确认网关行为。

## 残留风险与跟进项

- 网关 400 文案不命中签名 → 错误直出(后端 `send_error.rs` 补一行签名即可修)。
- registry 重启后每模型重新试错一次(代价为一次被抑制的往返);如实测可感知,再评估方案②(traits 主动预判)作为纯性能优化,但不要对 Auto 族做静态标记。
- 行为变化需产品知悉:Auto+图片从"前端拦截"变为"发送后自动重跑 + tips 提示"。
- **后端跟进项**:`agent.rs:1631` 无可用视觉分析模型的分支引入专属错误码(如 `NOMIFUN_IMAGE_ANALYSIS_NOT_CONFIGURED`,resolution 指向模型设置),前端 `conversation.agentError.codes` 加一行文案即可得专用引导卡片。本次否决了前端按 detail 字符串匹配的方案(后端硬编码英文串不在契约面内、同 code 承接其他 BadRequest、与"错误表面由 code 驱动"架构相悖)。

# Claude Code 图片处理公开资料调研

调研日期：2026-08-10

## 结论

`anthropics/claude-code` 公开仓库不包含 Claude Code CLI 的图片附件、模型请求或调度实现源码。因此，无法从官方公开源码验证它是否将多张图分批、是否逐图调用模型，或是否使用并发请求。

现有公开证据支持的做法是：图片作为直接多模态附件处理，在发送前统一压缩/缩放，并受整轮请求的载荷与内存约束控制。没有官方资料支持“10 张图默认拆成 10 个并发视觉请求”。

## 可验证证据

1. 官方交互模式文档将粘贴图片描述为输入中的 `[Image #N]` 图片引用，而非单独的图像分析工具调用。

   来源：[Claude Code Interactive Mode - Keyboard shortcuts](https://code.claude.com/docs/en/interactive-mode.md)

2. 官方变更记录说明，粘贴和附件图片会被压缩到与 `Read` 工具读取图片相同的 token 预算。这是发送图片前的统一预处理，不是逐图另起请求的描述。

   来源：[CHANGELOG.md](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#L2604)

3. 官方变更记录说明，粘贴图片会下采样；模型图片单边上限被修正为 `2000px`。另有记录表明上传前会 resize，避免 API 大小限制。

   来源：[下采样超大粘贴图片](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#L1995)、[2000px 上限修正](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#L2036)、[上传前 resize](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#L5089)

4. 官方变更记录明确处理过“很多图片”导致的请求过大误报及无界内存增长，说明多图处理是整体请求、内存和载荷管理问题；公开记录没有把解决方案描述为逐图并发。

   来源：[many images 请求过大错误](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#L370)、[many images 内存增长](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#L2064)

5. Anthropic 的视觉 API 文档把多张图片放入同一条消息/请求的 image content blocks 中。其通用 API 上限取决于模型上下文窗口（200k 上下文模型每请求 100 张，其他模型 600 张）；当单请求超过 20 张时会触发更严格的单图尺寸限制。此条是 Claude API 的能力边界，不是 Claude Code 的实现承诺。

   来源：[Claude Platform Docs - Vision / Request limits](https://platform.claude.com/docs/en/build-with-claude/vision#request-limits)

## 对 Flowy 的启示

- 选用支持图片的主模型时，保持多图同一条多模态请求的直传方式，并在客户端/后端统一缩放、压缩与限额，和公开资料中的 Claude Code 行为相符。
- 对不支持图片的主模型，`image_analyze` 是兼容层，不应伪装为 Claude Code 已采用的实现。
- 对 9 到 10 张图的单次 `image_analyze` 在 45 秒内超时，优先方案应是保持一次综合分析；仅在超时或 Provider 拒绝载荷时按小批次降级，例如每批 3 张、最大并发 2。默认拆为 10 路会重复系统提示和问题、丢失跨图关系，并更容易触发 Provider 并发限制。
- 图片能力分流已经可靠时，不应向用户显示“可能被忽略或导致报错”的旧提示；该提示与自动 `image_analyze` 分流的实际行为矛盾。

## 调研边界

- 本文只使用 Anthropic 官方文档与其官方 GitHub 仓库。GitHub issue 是用户报告，不作为实现事实依据。
- 公开仓库的文件树未提供 CLI 产品源码，因此无法确认私有实现中的队列、并发数、缓存键、重试策略或模型调用次数。

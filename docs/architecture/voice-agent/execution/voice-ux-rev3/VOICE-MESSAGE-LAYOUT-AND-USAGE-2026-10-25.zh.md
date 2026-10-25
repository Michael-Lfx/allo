# 语音消息排版与快模型积分归属修复 · 2026-10-25

状态：代码修复与定向回归已完成；生产构建结果见下方验证记录。基准 `cd5a904af77493faceba1c41a9df85220666b2a5`，分支 `feat/voice-next`。本报告保留此前启动恢复、原生与声学验收记录，不替代其结论。

## 1. 问题与处理链路

截图中用户与助手均靠左，原因在实时适配层：后端已发送 `position: right` 的语音用户投影，`transformMessage` 却把所有 `content` 消息强制改为 `left`。现保留事件明确给出的布局；旧 `user_content` 仍靠右，未指定位置的助手流仍靠左。消息数据库无需修复或迁移。

现有流程为：ASR 转写 → 一次快模型调用返回需求理解、意图与短答/任务目标 → 按需交给任务 Agent。`understood_request` 与短答由同一次调用产生；不存在额外一次专用的 ASR 纠错/摘要模型调用。原始 ASR 文字继续单独保存，不被模型理解覆盖。快模型使用宿主当前可用的模型配置，不等同于本地 ASR 模型。

## 2. 积分归属与兼容边界

| 边界 | 本次实现 |
|---|---|
| 调用前身份 | 从已经落盘的生成 claim 读取 Conversation ID 与稳定用户消息 ID；核对完整请求和归属，不能使用临时声学 turn ID |
| 普通及流式调用 | 复用 `nomifun-ai-agent` 已有计费作用域；流式任务在 `tokio::spawn` 内恢复作用域，取消与超时仍丢弃原模型 future |
| 回复投影 | 助手持久内容保存 `turn_id`，实时事件同步发布；空闲成对投影与忙时追加保持一致 |
| 实际积分查询 | 使用现有服务端按轮查询；语音快模型独立于任务模型选择，不能因主任务模型是本地/第三方就跳过查询 |
| 重放 | 已生成/缓存请求不再调用模型；投影回放读取原始存储行，原有交付回执指纹保持不变 |
| 任务执行 | 执行 Agent 继续按原有执行轮次计费和展示；本次补上快模型理解/短答这一笔归属，不把两个独立调用显示为同一笔，也不额外扣费 |
| 历史与无效输入 | 不凭估算补旧调用积分；ignore 仍不生成公开聊天消息，不为显示消耗污染上下文 |

没有修改积分价格、扣款规则、全局余额状态、鉴权、数据库迁移或声学模型。

## 3. 验证记录

证据目录：`build.noindex/voice-workspace-display-review/voice-message-layout-usage-20261025/`。

| 验证 | 结果与证据 |
|---|---|
| 排版反例 | `layout-red.txt`：新增用户投影靠右断言失败；初次 Bun 文件过滤还匹配了忽略目录中的旧副本，后续均改为 `./` 精确路径 |
| 前端最终回归 | `ui-verified.txt`：4 文件、69 passed、0 failed、200 assertions；覆盖实时布局、持久消息映射、live/durable 接替、实际积分查询资格与缓存 |
| 真实 HTTP + SQLite | `integration-verified.txt`：最终文件 34 passed、0 failed；覆盖普通及流式调用归属、缓存重放单次生成、忙时投影、实时与重载身份一致；此前 `integration-final.txt` 同样 34 项通过 |
| 宿主语音 | `host-voice.txt`：92 passed、0 failed、3 项既有 ignored |
| Conversation 库 | `conversation.txt`：648 passed、0 failed、0 ignored |
| TypeScript | `typecheck-verified.txt`：exit 2，61 条既有错误；本轮修改文件无错误，不调整既有基线 |
| 桌面编译 | `desktop-check.txt`：`cargo check -p Flowy` exit 0 |
| 生产前端构建 | `ui-build.txt`：`bun run build:ui` exit 0 |
| 错误契约 | `error-contract.txt`：exit 0 |
| 格式与差异 | 两个受影响包的 `cargo fmt --check` 与 `git diff --check` exit 0；新增测试片段单独显式启用真实 rustfmt。全仓 fmt 遇 Windows 参数长度限制，且仓库全局配置本就禁用重排，未重写既有文件 |

初次后端测试存在新夹具编译错误（未导出的 helper 与枚举误用），随后 HTTP 测试也暴露了夹具未设置 `prompt`、重放事件数量预期错误；已修正并重跑。上述失败不作为“计费缺陷已 RED 复现”的证据。模型使用本地测试 completer，没有发起付费云端请求。

## 4. 剩余验收边界

本轮证明本地归属、投影、查询入口和重放行为；没有实际请求云端扣费服务，不能据此声称线上扣款数值已经人工核验。界面需运行更新后的前端/宿主才能看到修复。原生 DPI、桌宠与物理音频范围沿用原验收边界，未在本轮扩展。

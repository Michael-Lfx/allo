# Wave 2：决策与流式发声闭环 (Decision & FastChat Loop)

> **对齐**：基线 Phase 2
> **前置**：Gate 1 通过（声学底座与模型管理已生产化）
> **出口门禁（Gate 2）**：路由 JSON 解析成功率 ≥ 99.5%；意图四分类准确率 ≥ 95.0%；VAD 静音点起算闲聊首音 P50 ≤ 850ms / P95 ≤ 1250ms；断网稳定播 `error.wav`；设置页过 `check:theme` / `check:i18n` 且 reduced-motion 人工验收通过

---

## 1. 目标与范围

交付“唤醒 → 转录 → 单次四路路由 → Edge-TTS 流式发声 → 追问免唤醒”的完整快轨闭环，并把桌面设置页从 Wave 1 的四态推进到全七态 + 完整动效降级。本波结束时，闲聊场景达到基线全部延迟指标。

## 2. 任务卡

| 任务卡 | 属主 | 产出 | 完成定义 (DoD) | 映射 |
| :--- | :--- | :--- | :--- | :--- |
| T2-01 | WS-B | `qwen3.5-flash` 单次路由客户端：JSON Schema 约束输出（经 `nomifun-model-invoke` 扩展）、3 轮/600 token 上下文窗、Task FSM 状态注入 | VT-2-01/02 绿 | Gate 2 |
| T2-02 | WS-B | 本地启发式预筛器：短碎杂音/非语音拦截在路由前 | VT-2-03 绿 | 隐私与成本 |
| T2-03 | WS-B | `EdgeTtsProvider`：tokio-tungstenite WebSocket、Symphonia MP3 解码、100ms 抖动缓冲 | VT-2-04 绿 | TC-AM-004 |
| T2-04 | WS-B | 快轨闭环：闲聊路径全程延迟仪表化（VAD 静音点 → 首音），P50/P95 上报 | VT-2-05 绿 | Gate 2 |
| T2-05 | WS-B | 4 秒温热追问窗：FOLLOW_UP_LISTENING 迁移 + 免唤醒再路由 | VT-2-06 绿 | TC-AM-005 |
| T2-06 | WS-B | 断网降级：TTS 连接失败立即内嵌 `error.wav`，FSM 回 IDLE 不卡死 | VT-2-07 绿 | TC-AM-010 |
| T2-07 | WS-F | 设置页全七态完整版：`Ready` 愉悦时刻、【释放空间】行内两步确认、SSE 200ms 节流合并 | bsk-2-01 通过 | TC-AM-017 |
| T2-08 | WS-F | 动效降级与双主题：reduced-motion 即时切换、明暗主题人工核查素材 | bsk-2-02 通过 + 人工签核 | Gate 2 |

## 3. 纵向测试集 VT-2（先写成失败测试）

| 编号 | 竖切 | 断言 | 通过标准 |
| :--- | :--- | :--- | :--- |
| VT-2-01 | golden 指令集 → 路由 mock（合法 JSON） | Schema 解析成功率 | ≥ 99.5% |
| VT-2-02 | 标注语料集 → 路由 mock | 四分类准确率（ignore/chat/agent_task/task_control） | ≥ 95.0%，混淆矩阵归档 |
| VT-2-03 | 短碎噪音语料 → 预筛 | 路由调用次数 = 0 | 100% 拦截 |
| VT-2-04 | 文本 → TTS mock（录制 WebSocket 帧）→ 解码 → 播放汇 | 帧序列完整、抖动缓冲行为正确 | 无 underrun |
| VT-2-05 | 闲聊语料 → 真实链路（TTS mock 边界）→ 首音 | 延迟分布 | P50 ≤ 850ms / P95 ≤ 1250ms |
| VT-2-06 | 闲聊播报完毕 → 2 秒内追问 | 无需唤醒词直接路由 | FSM 迁移序列正确 |
| VT-2-07 | 断网（TTS mock 拒绝连接）→ 提问 | `error.wav` 播放；FSM 回 IDLE | ≤ 1 个超时周期内恢复 |

## 4. bsk 验证剧本

| 剧本 | 步骤 | 断言 |
| :--- | :--- | :--- |
| bsk-2-01 设置页全态流转 | 驱动后端遍历 `NotInstalled → Probing → Downloading → Verifying → Ready → Removing`（fixtures 源 + 删除） | 七态行内表现与 ui-motion §3 映射表逐行一致；就绪图标 240ms scale 0.9→1；两步确认 3 秒自动还原；全程行高恒定 60fps |
| bsk-2-02 动效降级与主题 | CDP 模拟 `prefers-reduced-motion` → 重跑全态流转；切换明/暗主题 | 无位移无过渡（computed style 断言）且语义完整；双主题截图归档供人工签核；`check:theme`/`check:i18n` 绿 |

## 5. 出口门禁检查表

- [ ] Gate 2 四项硬指标全部达标（解析率 / 分类准确率 / P50 / P95）
- [ ] VT-2-01~07 全绿；断网用例稳定复现通过
- [ ] bsk-2-01/02 记录归档；reduced-motion 与双主题人工签核完成
- [ ] `bun run check:i18n`、`bun run check:theme` 绿

## 6. 本波禁止事项（追加）

- 禁止为压延迟指标在测试里跳过 VAD 静音点起算口径（基线统一口径：从静音切分点起算）。
- 禁止在路由客户端内为重试引入第二次 LLM 调用（契约是单次原子路由；失败走降级音效）。
- 禁止把追问窗时长做成隐式常量散落代码（4s 属冻结契约行为参数，改动需回写基线）。

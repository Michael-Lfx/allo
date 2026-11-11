---
id: v3
title: "悬浮 Pill 说真话——图课程流绑 structure 任务真值，落库即退场；v2 流零变化"
type: task
status: closed
assignee:
blocked_by: [v1]
part_of: ../map.md
---

# v3: 悬浮 Pill 说真话（G1 谎言面）

**What to build:** 图课程流（`generation.request.course_kind ===
'learning_graph'`，含恢复态）HTTP 秒回 `completed` 后，悬浮 Pill 不再显示
「✓ 生成完成」——改为绑定该课 structure 任务的真值：

- **structure 在跑**（快照行 running/queued）：label 换新 key（如
  「结构生成中 · 已 07:31」，elapsed 来自快照行；attempt 可选带入），
  保留「查看」+ 取消（取消走既有 per-key 端点 `api.ts:223`
  `/api/learning/generation-tasks/{key}/cancel`——纯接线）。
- **落库**（快照 done）：Pill 自动退场（不显示完成态——完成的真话在
  工作区徽章/摘要行与对话框卡片，避免再造一个秒级翻转的完成面）。
- **失败/取消**：显示对应终态（复用现有 genBgFailed/genCancelled 语义 +
  「查看」），点击回对话框/工作区。
- **数据源**：与 v2 同源快照轮询（course id 取
  `generation.result.course.id` 或恢复态 `restoredTask.course_id`）；
  轮询仅在图课程 completed 后且 structure 任务未终态时开。
- **v2 描述流/知识库流 Pill 行为零变化**（HTTP 终态即真终态，现有
  genBg* 语义保留）。

文案前端词典新增 key 双语对称 + 双语断言（红线 2）。

## 法证证据（2026-11-11 真机 + 渲染探针）

- 渲染探针 S3（真组件 + 真状态）：图课程 HTTP 秒回后 Pill 输出
  `✓ 生成完成 / 通盘认识股票市场基础 / 查看`——与结构实际在跑 12 分 30 秒
  同窗口（12:00:42–12:13:12），绿标完成态全程说谎（G1）。
- 根因：Pill 绑 `generation.status`（`index.tsx:446-455`），graph 流该状态
  数秒翻 completed（c1 设计的呈现语义：卡片先出）；「完成」label 对 v2 流
  是真话、对图课程流是半程。
- 取消端点已暴露：`api.ts:216-223`（list + per-key cancel，d7 加）。
- 轮询先例：c2 收敛轮询形状（假数据源可注入）。

## 实施要点

- [ ] `index.tsx` Pill 装配处按图课程流分流：structure-watch 状态源
      （小 hook 或本地 effect，与 v2 轮询同形状；不跨文件抽公共 hook——
      评审 judgement「第三处手写轮询」留痕，若施工时抽则两处共用同一实现）。
- [ ] label/取消接线 + 新 key 双语 + 断言（真实词典 JSON）。
- [ ] 落库退场；对话框重开（`onView`）行为不回归。
- [ ] 渲染测试：图课程三态（在跑/落库退场/失败）+ v2 流零变化对照 +
      en-US 文案。

## 验收

- 渲染测试绿；typecheck 零新增；i18n 双闸绿。
- 真机（与 map 欠账合并）：建图课 → 关对话框 → Pill 显示「结构生成中 ·
  已 …」→ 落库后 Pill 消失；取消路径（Pill 取消 → structure_cancelled →
  终态呈现）；v2 描述流 Pill 呈现与改动前一致。

## 非目标

- 对话框内呈现（v4）与工作区徽章（v2）。
- v2 流 Pill/对话框零变化（map 全波非目标）。
- 后端取消/注册表行为变更（per-key 端点已存在，纯接线）。
- 轮询抽公共 hook 的重构（票面注明：两处同形状实现可接受，抽公共属
  顺手优化非目标）。

## Resolution（2026-11-11 施工收口）

**代码面验收全过，已闭票；真机目验与欠账合并执行，FAIL 按票重开。**

- 施工 = `522ea2323` + `4803a6957`（合入 v2）：`structurePillWatch.ts`
  （findStructureTaskRow / structurePillSpec / startStructurePillWatch 5s
  节拍、假数据源可注入）+ index.tsx 装配分流 + per-key 取消接线（api 已有，
  纯接线）+ 落库退场。40 例测试（TDD 先红后绿 31→40）。
- 施工裁量（留痕，评审判合规）：①快照缺行 → 退场而非续拍等待——后端不摘
  行、行在请求生命周期 pre-register，缺席只可能是异常/清空/未来剪枝，退场
  是唯一不撒谎选择；与恢复态收敛「缺行→cancelled 兜底」有意不同（彼处有
  挂起承诺，此处无）。②恢复态 structure running 期间 Pill 仍走既有「课程
  生成中」面（c3 现状；票面谎言面只在秒翻 completed 后）。
- 评审修复 = `73983597c`：F2 双 Pill 块合并 + undefined/null 双哨兵收敛为
  GraphPillFacet 两相（legacy/structure，取消按相位分流）；F3 Attempt 占位
  符断言；F4 装配三分流源断言五钉（照 v2 multiSourceDisclosure 形态）。
- 留痕不修：structureLine prop 透传（轻度 Middle Man，沿既有形态）；两处
  同形状轮询不抽公共 hook（票面明示）。

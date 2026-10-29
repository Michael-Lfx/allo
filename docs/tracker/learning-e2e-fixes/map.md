# Map: 全流程 E2E 修复波——生成耗时 + 旅程四缺陷（e1–e5）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-10-29）。来源 = 吸收六波后的真实课程全流程 E2E（复利课，
      learn0928 尖端 c67c991dd..8335d719c 二进制）。五票全部 open，互不阻塞；
      e2（door_refs）是用户旅程 BLOCKER 优先级最高。 -->
<!-- 前情：六轮吸收波已并入 feat/learn0928（六轮 map 关闭于
      docs/tracker/learning-absorb-6/，真机四臂对照卡在其 assets/）。 -->
<!-- 参考：learnhub C:\Users\test\Desktop\my\learnhub（tip bd31277；其后 3 票只
      动 node_growth/sessions/coach/judge，行号涉彼已按 bd31277 核准）。研究
      代理 2026-10-29 五面深挖结论已织入各票 Reference。 -->

## E2E 分相位账目（2026-10-29，Flowy Cloud flash，真实课程「复利与利息」）

| 相位 | 墙钟 | 明细 |
| --- | --- | --- |
| 建课→结构完成 | **11m14s** | 双调用 max_tokens=384000；**首次尝试 3m57s 白跑**（gd.no_schedule「每月」违约 → 双产物整体重生成）；好路径 7m07s；期间 1 次 SSE 瞬断（被重试吸收） |
| 首批生长（5 节点） | **7m55s** | 自动衔接；1-2 次大调用 |
| 单节内容+题 | **2m33s / 1m22s** | agent 草稿循环 **9-15 次串行调用**（各 max_tokens=8192）；一次失败尝试 33s 后 BAD_GATEWAY 直抛调用方；一次调用方断连（15s）即静默取消 |
| **用户看到第一节内容前** | **≈27 分钟** | 含 4 分钟结构白跑 + 1 次瞬断白跑 |

产物留档：`C:/Users/test/Desktop/work/absorb6-verify/e2e/`（REPORT.md、各步
JSON、两课正文）。

## Notes（两仓对账关键事实）

- **learnhub 无流式调用**（rig 非流式补全）：allo 的「瞬断流错误」类目在
  learnhub 不存在；其对应纪律是错误两桶（可重试/数据性，llm.rs:24）+ 门面
  ≤3 带反馈环 + 层内短退避 + 显式超时=可重试失败。
- **learnhub 全仓无 max_tokens 上限**——规模控制全靠条款数（结构块 40–80/硬
  100、生长批 1–5）；allo 的 384000 天花板与 8192 小调用并存是自留形态。
- **结构重试粒度**：learnhub 明言「任一环节失败两份一起重跑」且踩过单侧重跑
  的坑（反馈通道单源注释）——allo 的白跑不是粒度错，是**缺同指纹早停**
  （同装配同输出盲烧预算；learnhub 生长侧有指纹早停+守卫豁免，
  node_growth/facade.rs:303-366）。
- **两遍式 vs 工具循环有案**：learnhub ADR-0014/契约 v1.7 = content-plan +
  content-write 恒 2 次 LLM 出整课（正文走文本接缝避 JSON 转义雷区），题目
  另面单发+元素级丢弃；工具只作 ≤8 轮有界只读查询（插件先例 129 回合后收敛）。
  allo 的 9-15 次循环是草稿工具交互形态——收敛是大手术（e1 内单列裁决点）。
- **耗时基线两仓都没有**：learnhub smoke-suite 仅定性读数。两仓首条耗时读数
  随 e1 立。
- **learnhub 生成等待模型**：命令 await 完成但 spawn_blocking + 步骤级进度
  事件 + 协作取消（ADR-0028/0029；architecture.md:721「分钟级阻塞任务一律
  spawn_blocking」红线）——从不挂请求生命周期。
- **door_refs**：learnhub 自己也 3/21 低标注（ADR-0013:21 明言），答案 =
  #165 扩面条款（入门档全部块逐块判）+ 零标注 warn 读数（不加机械门）+
  端点硬拒绝但文案给出路（「请改用逐个跳过」）。维护面不补标注；存量二选一
  （近似映射或删课重建）。

## 验证总纲

1. 每票带前后相位耗时对照（e1 的对照卡 = 同 fixture 建课→第一节内容就绪的
   墙钟）；learnhub 参考实码先读铁律照旧。
2. e2 的验收硬判据 = 真课声明端点 200（铸概念计数守恒：零节点写入）。
3. 涉提示词的票走金样/围栏/条款覆盖断言三闸；不新增迁移。
4. 真机纪律照旧（NOMI_CHANNEL=dev、核 mtime、--data-dir 位置）。

## Tickets

| id | 票 | 优先 |
| --- | --- | --- |
| e1 | 生成耗时优化——相位预算、白跑早停、调用形态收敛裁决 | 中 |
| e2 | door_refs 生成侧标注——解锁已会声明旅程（BLOCKER） | **高** |
| e3 | 声明窗口——建课后暂停在门口，生长不自动抢跑 | 高 |
| e4 | lesson/generate 后台化 + 瞬断重试加固 | 高 |
| e5 | 题面小项——difficulty 消解、reflection explanation 三保险、密度锚补强 | 低 |

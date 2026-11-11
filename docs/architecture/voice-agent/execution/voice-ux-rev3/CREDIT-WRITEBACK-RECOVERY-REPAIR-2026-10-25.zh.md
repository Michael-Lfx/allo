# 语音积分并发写回与失败恢复 · 修复记录

> 日期：2026-10-25。分支 `feat/voice-next`，接手 HEAD `cd5a904af77493faceba1c41a9df85220666b2a5`。本轮延续尚未提交的消息排版、快模型计费归属及独立悬浮条成果；历史计划和首轮复审原文完整保留。

## 1. 问题和边界

全面复审增加两个真实反例：历史语音轮次并发查询后，以查询前的旧 Conversation 快照覆盖积分表；首次查询未认证或异常后，协调器永久保留 attempted，恢复网络也不补查。新增正式测试同时暴露了第三个关联问题：持久写入失败仍提前写入成功缓存，使后续查询跳过真实写回。

三者均为积分展示与持久恢复问题，不涉及新增扣费。普通对话保留原云模型门禁，语音协调仍只在语音 dock 挂载；没有修改价格、余额 Context、鉴权、数据库迁移、声学模型或 Agent loop。

## 2. 实际修复链路

```text
语音当前/历史持久轮次 → 语音调度（查询中 / 成功 / 有限失败重试）
                                   ↓
普通入口（原模型门禁） → 同一查询核心 → Conversation 写回串行队列
                                   ↓
                       重新读取最新积分表 → 合并 → PATCH
                                   ↓
                        成功后写缓存、发事件、刷新视图
```

| 决策 | 实现与理由 |
| --- | --- |
| 写入互斥粒度 | 同一渲染器内按 Conversation 排队，查询仍可并发；不同会话不互相阻塞。不能只按 turn 去重，因为 PATCH 覆盖整张嵌套 usage map |
| 读取时机 | 等上一笔写入完成后才读取最新持久 Conversation；SWR 以 current 合并，不再回写请求前旧快照 |
| 缓存成功时机 | PATCH 成功后才登记成功缓存并发布积分事件；PATCH false/异常返回查询失败，后续可以重试；一笔失败不会阻断后续排队写入 |
| 历史与别名 | 继续保留 wire/server echo/alias 三种 lookup key、已有较丰富 usage 与原持久表容量上限；不覆盖其他 extra 字段 |
| 语音失败预算 | 首次失败后最多再试 3 次，间隔复用现有 2.5s、8s、22s；查询中、冷却中及成功轮次不因 RMS 重复查询 |
| 无快照恢复 | 调度等待 pending promise 和重试计时器，不依赖新音量快照；网络 online 或窗口 focus 可重新开放失败轮次预算，成功轮次仍去重 |
| 生命周期 | effect 卸载/切换会话会取消后续计时和事件订阅；已开始查询可以完成真实 usage 持久化，旧视图不得再发起重试 |

本次串行队列解决当前批量回填和同一渲染器内普通/归属入口的并发写回；不是跨进程锁，不宣称为任意独立客户端同时整表 PATCH 提供数据库级原子合并。未改变现有公共 PATCH 契约。

## 3. 正式回归与 RED → GREEN

证据目录：`build.noindex/voice-workspace-display-review/credit-repair-20261025/`。

- `red.txt`：正式迁入审查反例后 **7 passed / 3 failed**；覆盖多轮覆盖、失败写入污染缓存、认证恢复不补查。未以删测或放松成功/隔离断言消除失败。
- `green-first.txt`：首轮修复后 **20 passed / 0 failed**。随后增加乱序完成的 12 轮写回、普通/归属入口混用、不同会话独立写入、别名与持久表上限等边界。
- `ui-regression-final.txt`：语音目录、普通积分核心和消息映射 **172 passed / 0 failed / 966 assertions / 15 文件**。
- `browser-probe/result.json`：真实 React/Chrome 中 **11 项通过**，覆盖无快照自动恢复、RMS 重渲染节流、成功去重、卸载、online/focus、旧视图 pending 请求延迟返回；无 page error。测试响应仅在忽略目录的独立 bundle fixture 注入，生产源码没有假积分或旁路。
- `browser-probe/cleanup.json`：`connected=false`，本轮浏览器已关闭。

新增测试中曾有类型导入误写到动态解构内，造成一次语法失败（`ui-regression.txt`、首轮 typecheck）；已改成静态 `import type` 后重跑，不把该失败记为产品缺陷，也不删除其日志。

## 4. 验证限制和交付范围

完整 SPA、200% 原生缩放、桌宠透明首帧/DPI、真实麦克风与扬声器/AEC、线上付费金额仍保留原未签核边界。新的 Chrome 探针只验证 hook 生命周期，不替代完整页面视觉验收。前一轮真实按钮布局门禁因临时 profile 清理 EBUSY 未完成，本轮只检查按钮静态契约，不把它改写成真实布局通过。

后端五个 Rust 文件延续此前已验证的原始成果，本轮未再改写；对应上一轮原生语音集成 34 项、会话 9 项、宿主 92 项通过（3 既有 ignored）的日志仍在 `full-review-20261025/`。本次生产构建和质量门禁的最终回执追加于下节。

## 5. 最终验证与提交回执

以本节末尾后续追加的实际结果为准；不将本地提交等同推送或完整产品验收。

- 最终定向回归：**172 passed / 0 failed / 966 assertions / 15 文件**；Chrome hook 探针 **11 项通过**、无 page error、已关闭浏览器。
- 最终 TypeScript：**61 对 61，逐条相同，新增/移除 0**，见 `typecheck-comparison.json`。首轮测试语法错误已消除。
- `bun run build:ui`：**exit 0**，1m51s；构建仍有既有 bundle 分块体积提示。
- `check:i18n`、`check:theme`、`check:motion`、`check:icons`、`check:button-layout-contract`：**全部 exit 0**；`git diff --check`：**exit 0**。
- 未将前一轮既有 dead-css 失败或实际按钮布局 EBUSY 写成通过；没有修改仓库基线。后端沿用上一轮同源码原生回归结果。
- 交付范围为经用户授权的本地 Git 提交：包含接手时已有的排版、归属和 dock 相关成果、本轮修复、正式反例与文档；忽略目录中的日志、测试 bundle 和独立探针不纳入提交。提交 ID 以 Git 日志及最终交付消息为准，不推送远端。

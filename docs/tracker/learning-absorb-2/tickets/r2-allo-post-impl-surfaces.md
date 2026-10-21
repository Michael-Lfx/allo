---
id: r2
title: "allo 实施后对应面盘点"
type: research
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

allo 学习模块在一轮吸收实施落地（i01–i08，16 提交）后的现状：教练批管线（提示词纪律、
门清单、重试形态）、铸名门覆盖面和缺口、内容生成写作纪律、est/minutes 量纲、词条层——
哪些面已与 learnhub 新提交对齐、哪些是二轮增量？

## Resolution

**速览**：实施后现状与二轮吸收的关键对照结论——

1. **教练批**：已迁 json 接缝 + ng.* 条款化契约 + 3 轮带反馈重裁（「点修不是重写」纪律已有）；
   **缺**同指纹早停、修订轮容量守卫；评审「同批兄弟 teaches 不构成供给」已对齐；COACH_SYSTEM
   内联常量（g4 已裁与契约迁移解耦）；提示词无「装箱/先推后练/隐渐/est=XP」字样。
2. **铸名门**：精确名撞（canonical∪别名，大小写折叠）+ 批内唯一 + 落库幂等复用已覆盖——
   learnhub #79 的名字面兜底 allo 已有同效；**缺**别名入池闭环、`deprecated` 口径一致
   （门过滤、apply_batch 复用/解析查询未过滤）、无定义近邻判定（learnhub 既有机制非本窗口）。
3. **内容生成**：视觉承诺兑现制 + 密度纪律 + 篇幅/闸门已有；**缺序列面纪律**（先推后练/
   首现引入/渐隐/引用名开讲）；graph_node_topology（definition+档位停讲+供给节点摘要+防
   超纲）较完整，缺 node_ctx 三投影（首现/词表/引用名）。
4. **量纲**：教练 minutes（1-60 门、常规 5-30）、estimated_minutes（缺省 10）、打卡（复习
   事件计数）、companion XP（Lv=sqrt(xp/100)）——**三套独立无桥，无单一量纲宣言**。
5. **词条层**：CONTEXT.md 无「道与术/概念不可学完/概念投影/est 与 XP」条目。
6. **冒烟**：nomicore learning-smoke 四子命令已落地（structure/json/grow/loop），判据含
   首批 assumes 全空、就绪即发布、铸名全局唯一。

完整报告（含文件路径行号、条款清单、现状细节）：
[assets/r2-allo-post-impl-findings.md](../assets/r2-allo-post-impl-findings.md)

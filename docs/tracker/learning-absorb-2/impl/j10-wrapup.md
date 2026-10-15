---
id: j10
title: "收口：文档对账 + 全量门 + 真机冒烟"
type: task
status: open
assignee:
blocked_by: [j01, j02, j03, j04, j05, j06, j07, j08, j09]
part_of: ../map.md
---

# j10: 收口

**What to build:** 全波收尾对账与验证（对齐一轮 i08 的收口模式）。

**实现决议:** 全部。

**Blocked by:** [j01]–[j09] 全部。

**Status:** ready-for-agent

- [ ] **文档对账**：`docs/architecture/learning.zh.md`（模块地图/命令计数/依赖边——挂载
      与修补两命令、xp 端点、generation-tasks）；ADR 补记（结构转活文档裁决 → 新 ADR 或
      ADR-0010 后继，含「learnhub ADR-0017 七裁决」的 allo 适配注记）；CONTEXT.md 词条
      补缺（挂载/维护回合——若 g1 全量进）
- [ ] **契约文档对账**：learning-structure-artifacts.md（st.* 新章 + 版本史）；ng.aliases
      条款入 growth 契约位
- [ ] **全量验证**：`cargo check --workspace` 干净；`cargo test -p nomifun-learning -p
      nomifun-db -p nomifun-ai-agent -p nomifun-companion` 全绿（**看汇总行数而非只看
      头部**——一轮 i01 教训）；`bun run typecheck + check:i18n + check:theme +
      check:button-layout-contract + check:css-scope + check:codemirror-runtime` 全过
      （typecheck/dead-css 存量基线除外，确认无 learning 新增）
- [ ] **真机冒烟全套**：learning-smoke `structure / grow / json / loop / content / mount /
      repair / xp` 逐个跑通，判据全绿（EXIT=0）；观测面档案抽查
- [ ] **领域词表终检**：CONTEXT.md 新词条（道与术/概念不可学完/概念投影/est 与 XP/挂载/
      维护回合）与实现一致；GLOSSARY 式术语无漂移
- [ ] 二轮 + 三轮地图的雾区/后记终审（后置项触发条件复核）；推送分支
- [ ] 验证：全门 + 冒烟证据链接进本票 Resolution

**Resolution:**（收口时写）

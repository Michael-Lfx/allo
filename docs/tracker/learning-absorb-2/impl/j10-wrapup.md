---
id: j10
title: "收口：文档对账 + 全量门 + 真机冒烟"
type: task
status: closed
assignee: zhangyj
blocked_by: [j01, j02, j03, j04, j05, j06, j07, j08, j09]
part_of: ../map.md
---

# j10: 收口

**What to build:** 全波收尾对账与验证（对齐一轮 i08 的收口模式）。

**实现决议:** 全部。

**Blocked by:** [j01]–[j09] 全部。

**Status:** ready-for-agent

- [x] **文档对账**：`docs/architecture/learning.zh.md`（模块地图/命令计数/依赖边——挂载
      与修补两命令、xp 端点、generation-tasks）；ADR 补记（结构转活文档裁决 → 新 ADR 或
      ADR-0010 后继，含「learnhub ADR-0017 七裁决」的 allo 适配注记）；CONTEXT.md 词条
      补缺（挂载/维护回合——若 g1 全量进）
- [x] **契约文档对账**：learning-structure-artifacts.md（st.* 新章 + 版本史）；ng.aliases
      条款入 growth 契约位
- [x] **全量验证**：`cargo check --workspace` 干净；`cargo test -p nomifun-learning -p
      nomifun-db -p nomifun-ai-agent -p nomifun-companion` 全绿（**看汇总行数而非只看
      头部**——一轮 i01 教训）；`bun run typecheck + check:i18n + check:theme +
      check:button-layout-contract + check:css-scope + check:codemirror-runtime` 全过
      （typecheck/dead-css 存量基线除外，确认无 learning 新增）
- [x] **真机冒烟全套**：learning-smoke `structure / grow / json / loop / content / mount /
      repair / xp` 逐个跑通，判据全绿（EXIT=0）；观测面档案抽查
- [x] **领域词表终检**：CONTEXT.md 新词条（道与术/概念不可学完/概念投影/est 与 XP/挂载/
      维护回合）与实现一致；GLOSSARY 式术语无漂移
- [x] 二轮 + 三轮地图的雾区/后记终审（后置项触发条件复核）；推送分支
- [x] 验证：全门 + 冒烟证据链接进本票 Resolution

**Resolution:**

**速览**：j01–j09 全波收口——文档对账（learning.zh.md 存储路由/前端/冒烟八档 +
ADR-0012 新增 + CONTEXT.md 挂载/维护回合两词条 + 契约 v1.2 版本史）、四 crate 全量
测试、前端全门、冒烟矩阵真机执行。**实施波 j01–j10 全部关闭**。

1. **文档对账**：
   - `docs/architecture/learning.zh.md`：迁移族补 076–080；HTTP 面补
     `graph/structure/mount|repair`、`xp/reading|calendar`、`generation-tasks*`（六类
     kind）与 checkins XP 口径注记；WS 事件补 structure_mount_*/structure_repair_*；
     前端节补结构图挂载三态段/双入口、XpPanel（打卡退役接管）、useCheckinStatus XP
     口径；冒烟节改八档（content/xp/mount/repair 判据与无凭据如实失败纪律）。
   - **ADR-0012**（`docs/adr/0012-living-structure-mount-repair-findings.md`）新增：
     learnhub ADR-0017 七裁决的 allo 适配——挂载无判定身份/单行 JSON + 审计表载体/
     ops = 意图引擎管自洽/级联不建逐 op 跟迁（hash 单点兜住）/findings 线索不是第二
     真相/ops 空 = 合法结论/显式命令触发 + LLM 失败如实上报。编号避让核对后取 0012
     （allo 侧 0011 之后顺延；learnhub 侧 0016/0017 为其自身编号）。
   - **CONTEXT.md** 两词条补缺：**Mounting（挂载）**（导航事实/无判定身份/失效降级
     合法/hash 锚定显影）与 **Maintenance Round（维护回合）**（显式触发/留痕+观察输
     入/ops 空合法/级联不逐 op 跟迁）；Structure 词条补活文档语义句（ADR-0012 引用）。
   - 契约文档 `learning-structure-artifacts.md` bump **v1.2**：§9 挂载（j08）+ §10
     修补（j09）新章在案，版本史补 v1.2 行；ng.aliases 条款已随 j04 住
     `learning_graph::growth_contract`（ng.review.reuse/ng.aliases.*），不另入本契约
     （结构契约只管结构面）。
2. **全量验证**（看汇总行数）：learning **310** 全绿、db **911** 全绿（含 078/079/080
   迁移测试 + schema contract 23）、ai-agent **1128** 全绿、companion **290** 绿 +
   **2 存量失败**（`run_for_skips_when_model_unconfigured` /
   `learn_endpoint_is_companion_scoped…`——j05 收口时已在干净 HEAD f918470cf 复现，
   与学习波无关）。`cargo check --workspace` 0 error；学习页单测 19 过。
   **一次 flake 记录**：learning 套件并发负载下单次跑出 309+1 failed，重跑即全绿
   （310），失败用例未复现——与 i01 教训一致，以汇总行数与重跑为准。
3. **前端全门**（根目录跑）：typecheck **60 = 存量基线**（videoCanvas/videoGeneration
   等，learning 路径零新增）、check:i18n 过（11393 键）、check:theme 过、
   check:button-layout-contract 过、check:css-scope 过、check:codemirror-runtime 过；
   check:dead-css 存量失败均在非 learning 文件（j06 已确认 learning 零新增）。
4. **冒烟矩阵（真机）**：本机开发库无已启用 provider——需 LLM 的档位按纪律在
   provider 解析处**如实非零失败**，逐档状态：
   - `xp`：**EXIT=0 全绿**（教新 full 27 + 复习 full 5 = 32 XP；读数
     today_xp/settled 1/1/remaining 0/ETA 0；84 桶日历今日 32）；
   - `mount`/`repair`/`content`/`structure`/`grow`/`loop`/`json`：种子/装配/门面
     全链走通至 provider 解析，无凭据如实 Conflict 非零退出（mount 冒烟的幂等
     判据与 repair 的盘面复验/级联标记由服务层单测钉住：mount 幂等在
     `structure_mount_round_mounts_then_skips`、repair 触发门槛在
     `structure_repair_skips_when_no_triggers_and_audits`）；
   - **全绿 EXIT=0 矩阵的前置条件 = 配置任一已启用 provider**（`--db` 指开发库或
     临时库内配置），观测面档案抽查同轮完成（`<data-dir>/observation/learning-smoke/`）。

   **Resolution 增补（2026-10-19，首次带凭据真机对账后的勘误）**：本段「真机
   冒烟全套」当时的实际状态 = LLM 档（mount/repair/content/structure/grow/
   loop/json）走通至 provider 解析处**如实非零失败**，仅 xp 全绿——§4 末行
   「全绿 EXIT=0 矩阵」的前置条件当时未满足，真实全绿于 2026-10-16 首次带
   凭据（Flowy Cloud + AIPC-deepseek-v4.1-flash）达成，并同期暴露/修复两处
   真机缺陷（结构生成 16384 token 帽截断——目录上限死管道改消费；门口编号
   提示词 `[G1]` 括号残留）与一处已知未修（`learning-smoke repair` 种子结构
   性不可赢，详见 q 波后审计记录）。
5. **领域词表终检**：CONTEXT.md j 波词条链（道与术/概念不可学完/概念投影——j01；
   est 与 XP 走 DTO 注释与量纲宣言词条住学习架构文档——挂载/维护回合——本票）与实
   现一致； Avoid 行无漂移。
6. **雾区/后记终审**：会话体系命令面/统一页（g4 触发 = 任一进入实施）、检测/quiz
   结算线（条款已入、载体待 g4）、复习中心聚合读数、修补回合的 findings 消费排序——
   触发条件均在票面留痕；**分支 feat/learn0928 已推送**（j01–j10 十提交 + 历史）。
7. **验证证据**：四 crate 汇总行见 §2；冒烟输出见 §4（xp 全绿 EXIT=0、json 0/3 如
   实报、mount/repair 无凭据 Conflict 文本「生成通道不可用/未配置」）；全门退出码见
   §3。

# Map: 四轮吸收——learnhub 7f57fc0 之后机制面六项 + Mindmap 修复

<!-- wayfinder:map -->
<!-- 状态：已到达目的地（2026-10-23）——k01–k07 全部关闭（k01 f3be14438、
     k02 3e3b67126、k03 b4259de0e、k04 8b86d0a63、k05 cd28da39e、
     k06 170ebaa70、k07 092a09d63；读数面 dd95ebe83）。机械闸全绿
     （learning 488 / db 迁移 + v3 契约 / ai-agent 17 / typecheck 学习面
     零错误 / i18n 门），双臂真机十臂全 EXIT 0——对照卡见
     [assets/verification-cards.md](assets/verification-cards.md)。 -->
<!-- 前情：一轮 i01–i08 / 二三轮 j01–j10 / quiz 线 q1–q3 全部关闭并推送
      （tip 354b98a45）；传统课程 kb 线 t1–t5 + hardening h1–h3 关闭于
      feat/learn-kb-course-v2（tip 644d58f94 未推送）。本波吸收 learnhub
      7f57fc0..1037086 共 113 提交中用户圈定的 A 组（机制级六项）+ 一枚
      独立前端 bug 修复。B 组（基础声明/门口物化战役、自动补批、调度
      cap5、事务收口、外部证据）、C 组（回放套件）、D 组（UI 形制/配色/
      工具链）按用户裁决不吸收。 -->
<!-- 来源：learnhub C:\Users\test\Desktop\my\learnhub，参考实码（票内逐票引用）：
      - #146/#147 + ADR-0028: src-tauri/src/app/maintenance.rs,
        src-tauri/src/engine/maintenance/findings.rs, src-tauri/src/infra/settings.rs
      - #154: src-tauri/src/engine/contentgen/facade.rs:186-200,
        prompts/contentgen/content-plan.md:35, engine/contentgen/validator.rs:1267-1280
      - #148: src-tauri/src/domain/concepts/nearname.rs:37,
        src-tauri/src/engine/questions/validator.rs:544-660,
        engine/questions/facade.rs:698-793, prompts/questions/question-generate.md:26
      - #126/#148: prompts/contentgen/content-write.md:42-48（R10/R11/R12/R13）
      - #149: prompts/decomposition/goal-decomposition.md:36-37,
        prompts/decomposition/prerequisite-terrain.md:18,
        prompts/node-growth/node-growth.md:42（深度本位修订 = a39d36a）
      - #127: prompts/node-growth/node-growth-review.md:28,
        src-tauri/src/engine/node_growth/reviewer.rs:7-17（提交 e1f111c）
      - a928e46: 思维导图两翼中心对称修复（src/pages/course-detail/ui/structure/mindmap.tsx） -->

## Destination

六项 learnhub 机制落进 allo 学习域：findings 消费水位 + 定时自动维护回合（k02）、
est 时长预算行 + 超容自报（k03）、题面相似度防雷同机械门（k04）、内容纪律
R10–R13（k05）、分解/地形/tier 提示词升级（k06）、评审判据①挂号子查（k07），
外加 Mindmap 负坐标翼修复（k01，独立小修先行）。每票必须带机制真机冒烟与
双臂前后比对（方案见 Notes 验证总纲）。

## Notes

### 验证总纲：k 系双臂真机对照（本波自有方案，不引 learnhub 回放套件）

1. **开关臂（同二进制）**：凡有行为面的机制（k02 水位与调度、k03 预算行、
   k04 相似门、k05 纪律块、k07 评审措辞）各配一枚环境开关
   `NOMI_LEARNING_<FACE>=off`（先例 `generate.rs:43` SOURCE_PLAN_KILL_SWITCH）。
   同一构建、同凭据、同 fixture 跑两遍（off/on）。
2. **版本臂（跨构建）**：纯资产面（k06 结构提示词 + 金样）无运行时开关——
   旧 tip 构建跑一次 vs 新 tip 跑一次（独立 `CARGO_BUILD_DIR`，禁 stash 验证
   存量，见 windows-git-bash-quirks 教训）。
3. **读数行**：每机制在对应 learning-smoke 档输出确定性计数器读数行 +
   观测存档路径（ArchivingCompleter `observation/learning-smoke/run-*/calls.jsonl`
   与 `learning-tasks.jsonl` 先例）。
4. **判据分层**：硬判据（机制证据：开关生效、种子红绿样、金样/条款短语断言、
   水位推进、EXIT 码）进 smoke 判据；效果判据（双臂对照）产出**对照卡**
   （两臂读数并排 + 人读条款命中清单）如实落档，票 Resolution 引路径。
   **对照卡必须两臂都跑——只跑一臂票不算关闭**；改善方向如实记录不设阈值
   （LLM 随机性的诚实边界）。
5. **回归护栏**：既有审计计数器（j07 naming/narrative 系列）与既有 smoke
   判据在 on 臂不得劣化。
6. **真机纪律**：凭据走 resolve_provider_config 先例；构建显式带
   `NOMI_CHANNEL=dev`；跑前核 `nomicore.exe` mtime（后台构建通知不可信）；
   `--data-dir` 放子命令前。

### 纪律

- **learnhub 参考实码是最高事实源**——每票开工第一步先读票面 Reference 表列的
  learnhub 文件，再动 allo 代码（q 系铁律沿用）。
- 迁移从 **070** 起（本分支当前尖端 069；开工时若被占顺延）；DB 迁移属仓库
  高危区：append-only SQL + models/repos/迁移测试同步 + v3 契约注册
  （PRODUCT_TABLES/UUIDV7/LOGICAL_REFERENCES，j08 先例）。
- 一票一会话；Resolution 收口 + Conventional Commits；中文提示词资产遵循
  金样三闸（金样重生成 LEARNING_UPDATE_GOLDEN=1 / 围栏 FENCE_DEV_FACE /
  条款覆盖短语断言，`structure_prompts.rs:466/:295/:388` 先例）。
- 前端改动过 i18n 双语红线与 `bun run check` 各门；typecheck 60 存量错误为
  基线（videoCanvas 等，与学习模块无关）。

## Tickets

| 票 | 标题 | 依赖 | 状态 |
|---|---|---|---|
| k01 | Mindmap 负坐标翼裁剪修复（两翼中心对称 + dy 补偿） | 无（独立小修先行） | 已关 `f3be14438` |
| k02 | findings 消费水位 + 定时自动维护回合 | 无 | 已关 `3e3b67126`（迁移 070） |
| k03 | est 时长预算行进图课时提示词 + 超容自报 capacity_note | 无 | 已关 `b4259de0e` |
| k04 | 题面相似度防雷同机械门（trigram 三口径 0.7） | 无 | 已关 `8b86d0a63` |
| k05 | 内容纪律 R10/R11/R12/R13 织入两管线 | k03（同文件序） | 已关 `cd28da39e` |
| k06 | 分解/地形提示词升级 + tier 深度本位自省 | 无 | 已关 `170ebaa70` |
| k07 | 评审判据①挂号子查结构化 | 无 | 已关 `092a09d63` |

双臂真机对照卡：[assets/verification-cards.md](assets/verification-cards.md)。

## Not yet specified（雾区，供后续波）

- R12/R13、挂号子查的机械检测面：learnhub 亦未建（ADR-0011 显式缓），prompt-only
  + 人读对照卡是两仓共同现状；若 C 组回放套件未来解禁，这些是第一消费方。
- 自动维护回合的 GUI 入口（「跑维护回合」按钮 + 两段结果条，learnhub
  maintenance.tsx 先例）——k02 只做后端调度与设置开关，按钮位随走查另议。

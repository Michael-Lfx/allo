---
id: m5
title: "companion store v3 老库缺 companion_learning_xp——升级后启动即崩（validate 前幂等补基线）"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# m5: companion store 老库基线表补建

**What to build:** `nomifun-companion` 的 store `open()` 在对存量库跑完
`upgrade_schema_in_place` 后、`validate_baseline_schema` 之前，幂等补建缺失的
基线表（全部 `CREATE TABLE IF NOT EXISTS` 语义），使任何 main 时代建过的
`companion/shared/memory.db` 升级后可直接启动，不再
`companion store start failed: companion store missing table companion_learning_xp`
退出 1。

## 法证证据（2026-11-09 m2 真机升级演练实锤 + 实码定性）

- **触发实录**：演练相位 B，未改动的 main 时代数据目录首次启动新二进制即崩
  （EXIT=1）；证据 `mc-drill/evidence/phaseB/deviation-companion-reset.txt` +
  两次启动日志（`evidence/phaseB/`）。learning 库（070）不受影响——这是
  companion 自己的 SQLite 文件、自己的版本轴。
- **根因三点**：
  1. `04c5da393`（j05，learn0928 线）把 `companion_learning_xp` 加进基线
     DDL（store.rs:774）与校验注册表（:1095），**但 `STORE_VERSION` 停在 3
     没抬**（:866），main 时代建的库同为 user_version=3；
  2. `upgrade_schema_in_place`（:1570）只跑 v3 特定迁移（embedding 列 /
     FTS 重建 / owner 收缩），注释明写「Non-v3 stores are left untouched」
     ——基线表缺口没有任何补建路径；
  3. `open()` = upgrade → `validate_baseline_schema`（:2125-2140），后者对
     注册表逐表硬校验，缺表即退出；:1837 注释自认这类失败面是「fail the
     boot of every install that…」。
- **用户面**：任何跑过 main 版本构建的安装，升级到含 j05 的构建后**启动即
  崩**，直到手删 `memory.db`——「升级导致程序报错」红线的最重形态。本波
  m1–m4 不触此文件（缺陷是 learn0928 线上 04c5da393 起就存在的存量缺陷，
  真机演练首次实证）。
- **现成红样库**：演练冻结副本里存有原始 `memory.db`（user_version=3、缺表、
  10 表全 0 行）——`mc-drill/evidence/phaseA/data-001-065-frozen/` 内。

## 实施要点

- [ ] **幂等基线 ensure**：`open()` 路径在 validate 前对全部基线注册表条目
      执行 `CREATE TABLE IF NOT EXISTS`（DDL 本就是 IF NOT EXISTS 形状，幂等
      无害）；新库路径不变（先建后 validate 顺序保持）。
- [ ] **版本轴裁决**（票内定）：a) 不抬 `STORE_VERSION`——ensure 幂等覆盖缺
      表场景，版本号继续表达「v3 布局」；b) 抬到 4 并让 v3→v4 迁移补表。
      推荐 a（改动面最小、对未来基线增量同样免疫）；选 b 则必须同步写
      v3→v4 迁移。裁决后在此勾选。
- [ ] **红绿样**：①用冻结副本的原始 memory.db（或按同形状构造）走 open()
      → 启动成功、表补建、validate 过；②新代码建的完整 v3 库 open 后
      **内容零变化**（ensure 幂等性）；③确认除 `companion_learning_xp` 外
      逐一排查基线注册表在 main..HEAD 之间有无其它新增表（有则同被 ensure
      覆盖，测试各钉一行）。
- [ ] `cargo test -p nomifun-companion` 全绿。

## 验收

- 单测红绿齐 + `cargo test -p nomifun-companion` 全绿；fmt。
- **真机复验**（现成阵地）：取演练 frozen 数据目录（含原始 memory.db）→
  修后新二进制一次启动成功、判据 1 由「有条件 PASS」转全 PASS；演练判据
  2–6 复跑一遍确认无回归（脚本都在 mc-drill/scripts/）。

## 非目标

- companion 其它 schema 演进/新功能。
- learning 侧任何改动（070 与本票无关）。
- 演练中发现的语义变化类事项（reviewed_count 改今日 XP 读数等，j05 设计如
  此，非缺陷）。

## Resolution（2026-11-09 施工 + 评审 + 真机复验收口）

**全验收过，已闭票；并回 blocker 解除。** 施工 = `fa317d604`（基线 ensure）+
`f43fcf75b`（评审加固），并回 feat/learn0928 @ `541925e8b`。

- **裁决落档 = 案 a**：不抬 `STORE_VERSION`（恒 3），ensure 并入
  `upgrade_schema_in_place` 尾部（复用其 user_version 门与 companion_memories
  在场门，open() 只调 upgrade 一处）；SCHEMA 重放只含 IF NOT EXISTS CREATE，
  由 `schema_replay_only_contains_if_not_exists_creates` 形状测试钉死——未来
  「进 SCHEMA 不抬版本」的基线增量被结构性免疫。
- **评审轴两轮**：首轮 0 硬违规 + 1 必修 3 应修全修（错误路径关池防 Windows
  锁库文件 / ensure-upgrade 双门收敛消「缺 companion_memories 被静默补出」
  缝 / SCHEMA 形状钉 / 命名对齐 ensure_baseline_schema_objects + 口径统一）；
  次轮规格轴确认四 checkbox 落地、fixture 等价真实 main 时代库（8fd9a4fb0
  三处 DDL/注册表差异面恰为 xp 一表）。fail-loud 保持：坏形状仍硬失败
  （`baseline_repair_must_not_rescue_a_shape_broken_table` 回归钉）。
- **单测**：`store::tests` 21/21（红样错误信息与演练实录逐字同）；全量
  294 pass / 2 failed = 存量环境依赖用例（stash 对照修前同款，非本票）。
- **真机复验（evidence/reverify/，reverify-summary.json 起读）**：
  - 判据 A 7/7——带原始坏 memory.db 的 main 时代数据目录（sha256 与红样
    逐字同）新二进制**一次启动成功**（修前同一目录即崩）；日志零
    missing-table/companion 失败/retired/谱系行，WARN/ERROR 零条；
    `_sqlx_migrations` 65→69 行（066/067/068/070 全 success），001–065
    checksum 逐行未变。
  - 修后 memory.db 11/11：`companion_learning_xp` 建表语句与正常新库逐字
    一致；user_version 仍 3；原 10 表 0→0 行列不变；sqlite_master 39 对象
    == 正常新库 39 对象，不多不少无重建残留。
  - 判据 B 无回归：判据 2–6 断言原样复跑全 PASS（due 4、日历/记忆归真、
    问题管理 25 行、活课程 7/7、SQL 35/35——`assert-sql.mjs` 一行未改）。
  - **额外发现**：本机 stable 安装（`AppData\Local\Flowy\Nomi`）的
    memory.db sha256 与冻结红样完全相同——真实安装确在受损人群里，无本票
    则升级即崩；修复后该安装升级将自愈。
- **如实记档（未覆盖面）**：`companion_learning_xp` 的写入路径本次真机未跑
  到（数据集 earned_xp=0 且无伙伴，无可记对象）——该路径由单测覆盖
  （grant 首笔 true / 同事件重放 false）；下次真机有 XP 结算时顺带补读数。

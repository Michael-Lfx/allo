---
id: m5
title: "companion store v3 老库缺 companion_learning_xp——升级后启动即崩（validate 前幂等补基线）"
type: task
status: open
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

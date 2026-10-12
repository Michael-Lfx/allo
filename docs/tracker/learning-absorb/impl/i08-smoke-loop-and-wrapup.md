---
id: i08
title: "冒烟合龙 grow/loop + 域文档链接 + 收尾全绿"
type: task
status: open
assignee:
blocked_by: [i07]
part_of: ../map.md
---

# i08: 冒烟合龙 grow/loop + 域文档链接 + 收尾全绿

**What to build:** 真机冒烟收口（s1 余量）：`nomicore learning-smoke grow`（首批 assumes
全空机械地板 = 空登记表判据，learnhub smoke-run 同款 exit 2 纪律；后续批全过结构门 +
铸名不撞）与 `loop`（端到端合龙：结构 → 首批 → 教练循环 ×N，确定性学习者替身程序化
完成/跳过节点驱动再生长，EXIT=0 判据；缺省从零临时 SQLite，`--db` 可指开发库；凭据复用
`resolve_provider_config` 现链）。域文档 `docs/architecture/learning.zh.md` 链接契约
文档（g2 #6）。全 effort 收尾：`cargo check --workspace` + `cargo test` 广域 +
`bun run check` 全绿。

**Blocked by:** [i07 教练批全迁](i07-coach-batch-json-migration.md)（grow/loop 冒烟的
对象 = 迁移后的教练批管线）。

**Status:** ready-for-agent

- [ ] learning-smoke grow / loop 子命令 + 机械判据（失败非零退出）
- [ ] 确定性学习者替身（完成/跳过程序化入口）
- [ ] 域文档链接契约
- [ ] `cargo check --workspace` + 广域测试 + `bun run check` 全绿

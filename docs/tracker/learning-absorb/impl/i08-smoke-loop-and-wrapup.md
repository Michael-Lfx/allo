---
id: i08
title: "冒烟合龙 grow/loop + 域文档链接 + 收尾全绿"
type: task
status: closed
assignee: zhangyj
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

- [x] learning-smoke grow / loop 子命令 + 机械判据（失败非零退出）
- [x] 确定性学习者替身（完成/跳过程序化入口）
- [x] 域文档链接契约
- [x] `cargo check --workspace` + 广域测试 + `bun run check` 全绿

## Resolution

**速览**：全项落地，commit `2b99c79c5`（feat/learn0928）。**吸收 effort 实施全部
完成**（i01–i08 全关闭）。

- **learning-smoke grow/loop**：`grow` = 立课 → 生长批 ×N（批间确定性学习者
  替身程序化把所有未完成节点置 completed——完成事件自动触发生长，与真实学习
  者同线）；`loop` = 结构生成先行再跑生长轮。机械判据（失败 exit 2）：① 首批
  assumes 全空（空登记表机械地板）；② 每批就绪即发布——批内每条 assumes 必须
  被**既往供给**（本课程更早批次的 teaches 或其他课程，档位达标）覆盖，同批不
  算；③ 铸名全局唯一（canonical ∪ 别名）；loop 追加 structure_json 落列检查。
  观测存档沿用 ArchivingCompleter（grow/loop 必挂）。
- **域文档**：`docs/architecture/learning.zh.md` 全面刷新到现状——结构生成先行
  的建课流、ng.*/gd.* 契约链接（→ docs/contracts/learning-structure-artifacts.md）、
  终点/罗盘退役（071）、结构投影路由与 WS 事件、四视图工作区（含结构图 Tab）、
  新增「真机冒烟」一节。
- **收尾门**：`cargo check --workspace` 干净；learning+db+ai-agent 广域
  2201 测全绿；`bun run check` 链逐门单跑——16 门全过（i18n/theme/motion/
  error-surface/support-surface/button-layout/icons/css-scope/codemirror/
  process-runtime/browser-platform/console-hide/market/fingerprint/
  release-sync/help），唯二失败均为**存量基线**：typecheck（60 个
  videoCanvas/conversation/wallpaper 错误，origin/main 自带）与 dead-css
  （旗标文件清单零学习页）。
- **补账（诚实的自纠）**：071 迁移集成测试的夹具顺序自 i01 起就是错的（先跑完
  071 再插终点行），i01 的验证窗口被 `head -30` 截断而漏看——本票修复（066–070
  → 插存量数据 → 071）并顺带修正 progress 夹具的 status CHECK 违规；learning
  页与全部学习相关测试现已真全绿。
- 票目 s1 设计的四档至此全部落地（structure/json 在 i02，grow/loop 在本票）。

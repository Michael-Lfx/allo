---
id: t1
title: "repair 冒烟种子结构性不可赢——种子与判据二选一改"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

`learning-smoke repair`（j09 落地）的种子是一份故意做坏的最小结构：2 块、
1 主干、零 frontmatter/domain/gaps/quality。而修补回合的管线契约（st.ops.apply）
要求修补后的盘面**全量 gd.* 复核零放宽通过**，冒烟自己的 [2/2] 盘面复验也用
同一把全量验证器。七型修补 op 里**没有**「加主干」「写 frontmatter/domain/
gaps/quality」的 op——从该种子出发：

- 模型返回任何非空 ops → 应用后复核必拒（种子违规原样都在）→ 回合失败；
- 模型返回空 ops（noop，合法结论）→ 盘面原样 → [2/2] 对种子跑全量 gd.* 必拒。

两条路都是死路，与模型强弱无关。j09 票面第 9 段印证：当时 repair 冒烟只跑
到「provider 解析无凭据如实失败」，从未真绿；2026-10-16 首次带凭据真机即暴
露此缺陷（EXIT=1，`修补后的结构未通过全量复核（结构与审计均未改动）`）。

### 两条候选修法（票面裁决）

1. **种子换全量合法结构**：种子改为完整 gd.* 合法盘面（≥6 主干、≥40 块、
   全 frontmatter），findings 留痕照旧触发修补回合——模型的修补 op 应用后
   盘面仍合法，[2/2] 复验语义不变。代价：要造/复用一份大夹具（可考虑复用
   structure 冒烟的真机产物或 golden 结构）。
2. **盘面复验只在 applied 轮跑**：noop 轮跳过全量复验（盘面没动，无完整性
   风险），种子保持最小——但 applied 路径的种子仍不合法，管线应用门
   （st.ops.apply 全量重过）依旧会拒任何非空 ops，等于只修了 noop 路的冒
   烟价值，**不彻底**；若选此路须连带裁决「applied 路冒烟怎么过」。

两法都须同步：`learning.zh.md` 冒烟节 repair 判据描述、j09/j10 勘误回链。

## Resolution

**速览**：修法 1（种子换全量合法盘面），commit `931932de4`。修法 2 票面
自评「不彻底」，弃。

- **种子**：对齐 `structure_contract` 金样形状——6 主干 × 42 块（入门/
  进阶/高级轮转，含 assumes/composes/contrasts 三边各一）、八标量键齐、
  domain 七键齐（门口 G1）、gaps ×3 / quality 五条规范措辞、terrain 带
  G1 门口对账（旧种子 `doors: []` 会在门口对账上翻红）。findings 留痕照
  旧（触发修补回合）。
- **两路均可赢**：noop（盘面没动，全量 gd.* 复验过）与 applied（修补 op
  应用后仍合法）——旧种子的两条死路（ops 应用后复核必拒 / noop 复验必
  拒）消除。
- **同步项**：`learning.zh.md` 冒烟节 repair 判据改为「种子是全量合法盘
  面，两路均可赢」；j09 Resolution 增补勘误回链、j10 增补回链均落。
- **真机验收**：2026-10-20 dev 库（flash）`learning-smoke repair` **首次
  EXIT=0**——`status=applied applied=1`、盘面复验全量 gd.* 零红、级联标
  记 mounts_stale=true 判 stale 正确。历史：2026-10-16 首次带凭据即
  EXIT=1（`修补后的结构未通过全量复核`），结构性不可赢确认。

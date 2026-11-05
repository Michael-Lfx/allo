# Map: 提示词与上下文质量修复波——absorb-8 真机法证驱动（p01–p03）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-05）。来源 = absorb-8 真机两臂（grow/content EXIT=0）
      的提示词与上下文逐段法证（档案 run-1793828585963 / run-1793829277050，
      完整提取在 C:/Users/test/Desktop/work/absorb8-forensics/）。用户指示：
      立票优化「误导模型或描述不清导致模型迷茫」「传入上下文机制或内容不足
      导致模型表现不佳」两类问题。本波是本地质量波，非 learnhub 吸收——
      参考实现 = 本仓 absorb-8 尖端实码；证据 = 真机档案，票内逐条引用。 -->
<!-- 基线：feat/learning-absorb-8 尖端 a672c7eb4（本波立票提交）。 -->

## 证据总表（全部来自 run-1793828585963 grow / run-1793829277050 content）

| # | 现象 | 证据 | 归票 |
| --- | --- | --- | --- |
| 1 | 碎片三值菜单无「声明面正向提及已覆盖」出口——模型被迫用噪声撒谎放行（滤杯/流程线/教辨认滤杯 皆判噪声，note 自创「scope 正面教学覆盖，非缺口」）；字面执行模型则会误打回 | 响应 fragments 92 unique terms 中噪声 113/127，note 与噪声定义（切词碎片）直接矛盾 | p01 |
| 2 | 点名清单 127 行：**单字碎片 38 行（30%）**、跨源重复 35 行（滤纸×3/温度计×3/途×3…），评审单调用 303–308s、范围外 6→14 轮间方差 | 清单全文 + 响应两轮分布；机械核 `relates()` 子串匹配本就接受「同词一行覆盖多源」 | p01 |
| 3 | 自教行循环自证禁令**已写在挂号表条款**（「ref 指向本条提名自身 = 循环自证，无效」）但首轮仍犯 `{term:滤杯, ref:手冲器具功能}`（= concept 自身）→ 死因重试 303s | call2 term_provenance 32 行含 1 违规；call3 修正 | p01 |
| 4 | 前置事实**双传**：前置路径内嵌前置节点要点摘要（L16-19）与已建立先例块（L22-23）同一事实两种形状；前置路径/前置契约**双头双规**（L11/L12 各说一遍「不要重新讲授」） | content user prompt 全文 | p02 |
| 5 | 空值行噪声：程度声明空值行（L4）照渲、L33 却要求「忠于学习目标、程度声明…」指向空值 | 同上 | p02 |
| 6 | 旧叫法行方向易误读：「历史波动率 又叫 贝塔系数——正文不使用」，而首现行（L28）就是历史波动率；渲染源 = active 别名池（lesson_draft.rs:292），语义自洽但措辞不设防 | 同上 + render_text 读码 | p02 |
| 7 | overshoot 四词硬扫无否定豁免——scope 声明「不引入 GARCH」类诚实边界句会被误计超纲（hit=1 已实证一次，一过正文不可追溯） | map 雾区 2026-11-05 法证条 | p03 |
| 8 | 提名候选集首批全空仍逐条渲染 5 个空块（「提名「X」的候选：」后无内容） | review user L146-156 | p03 |
| 9 | 教练不知 intent/scope 词面会进评审四源点名——声明面写法对点名负担零知情 | coach sys 无相应条款；intent/scope 源贡献 80/127 碎片 | p03 |

## 总约束

1. **机械核零语义放宽**：p01/p02 的瘦身与措辞修复全部不放宽任何机械门——
   `validate_fragments` / `check_out_of_scope_row` / 挂号表核验逻辑不动
   （p01 的渲染合并与 prompt 一行覆盖依赖既有 `relates()` 多对一匹配，已被
   真机 + 单测证明合法）。
2. **三值词表不动**（噪声/名词/范围外，learnhub 同形）：修复走条款扩义，
   不加第四值——避免契约/金样/互钉词表连锁改动。
3. **真机臂严格串行**；p01 必跑 grow 臂（fragments 读数对照），p02 必跑
   content 臂（判据回归 + 前置段渲染断言）。
4. **开工先复核 file:line**：票内行号来自 2026-11-05 盘点（基线 a672c7eb4）。
5. 每票一提交，并回集成分支；真机验收按票串行。

## 票表

| id | 票 | 档 | 阻塞/顺序 |
| --- | --- | --- | --- |
| p01 | 评审碎片面——第四出口条款 + 清单瘦身（合并渲染/单字过滤）+ 自证显性化 | **高** | 先行（mod.rs 评审条款区 + fragment_verdict.rs 独占） |
| p02 | 内容前置上下文——双传收敛 + 双头合并 + 空值行省略 + 旧叫法措辞 | 中 | 独立（lesson_draft/lesson.rs/lesson_content_loop）可并行 |
| p03 | smoke 读数与教练知情——overshoot 豁免 + 候选集空省略 + 四源知情条款 | 中 | 独立（smoke + render_nomination_candidates + mod.rs 声明面区）可并行 |

## 文件冲突面

- `learning_graph/mod.rs`：p01 碰评审条款区（挂号表/碎片裁决条款块），
  p03 碰声明面纪律区（一行知情条款）——不同区段可并行。
- `service/learning_graph.rs`：p03 碰 render_nomination_candidates；
  p01/p02 不碰。
- `learning_smoke.rs`：p02 碰 content 臂判据断言（前置段字样），p03 碰
  overshoot 读数与 grow 臂读数行——不同臂/函数。
- `lesson_draft.rs`/`service/lesson.rs`/`lesson_content_loop.rs`：p02 独占。

## 验证总纲

1. `cargo test -p nomifun-learning`（p01 新测：合并渲染/单字过滤/一行覆盖
   多源的 validate 断言）；p02 加 `-p nomifun-ai-agent`。
2. 真机：p01 grow 臂（fragments total 显著下降、unhandled=0、degraded=
   false、[brief]/[names] 判据不回归）；p02 content 臂（判据全过 + 前置段
   不再双传）；p03 content 臂（overshoot 读数措辞更新 + 判据不回归）。
3. 聚合门 PR 前：`cargo check --workspace && bun run check`。

## 雾区

- **upstream 候选**：问题 1（碎片第四出口缺失）learnhub 同构存在
  （reviewer.rs 同款三值菜单 + 同款噪声定义）——可回 learnhub 报；
  allo 侧修复条款可同步上游。
- **保留雾区不动**：前置/后续/禁止清单不带下游 scope（absorb-8 map 雾区，
  触发条件未满足）；ArchivingCompleter turn 响应存档（观测面，非本波）。
- intent/purpose 分工重叠（教练三文本字段）真机未观察到实际混淆——留观，
  不立票。

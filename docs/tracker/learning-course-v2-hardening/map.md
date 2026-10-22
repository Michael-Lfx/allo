# Map: 学习课程 v2 管线硬化——复验暴露的三处系统性改进

<!-- wayfinder:map -->
<!-- 状态：立票（2026-10-22）。h1–h3 待认领；三票相互独立，可并行。 -->

## Destination

v2 五段管线(清点→分诊→结构→展开→披露)已在真机复验中证明方向正确(成套/杂乱两库全流程 EXIT=0、脏物全带理由、无静默丢弃)，但复验读数暴露三处系统性弱点，本波把它们修到位：

- 分诊 LLM 批回包缺条目率高，大量文件靠保守 uncertain 兜底(精度损失，不是内容损失)；
- 覆盖对账不认父子滚入，章级 topic 报「未覆盖」的噪音警告污染披露质量；
- 同内容重复对的 canonical 按最新 mtime 选中「备份/错位」文件当正身，课时来源观感差。

## Notes

- **证据基线(2026-10-22 复验，评审修复后尖端)**：工件
  `C:\Users\test\Desktop\work\learning-smoke-course-20261022-verify\`(reports/verify-clean.json、verify-dirty.json + *-summary.txt + 两个 run log)。
  - 成套库 19 文件：7 文件「单文件分诊仍解析失败，保守按 uncertain 保留」；12 个 uncertain topic 中 8 个是章级 H1(实际都被 H2/H3 子课覆盖)。
  - 杂乱库 27 文件：同样 7 文件批缺条目落 uncertain(z9/999 靠 sha256 规则仍正确配对)；53 个 uncertain topic 中 7 个章级同因;`05-LinkedList.md` 被标 `duplicate_of=z9-LinkedList-备份.md`、`11-哈希.md` 被标 `duplicate_of=999-错位-哈希.md`(mtime 选中非正身)。
  - t5 修复前基线(对照)：脏库 8 文件同因 uncertain;三库冒烟见 `learning-smoke-course-20261022-t5\`。
- **代码锚点**：
  - 分诊批/兜底:`crates/backend/nomifun-learning/src/generation/triage.rs`(LLM 分诊批、`TriageLlmFile`、`map_llm_reply`、单文件兜底 reason「单文件分诊仍解析失败…模型未返回该条目的判定」);批拆半驱动 `generation/structured_batch.rs`(部分覆盖只补缺失子集)。
  - 覆盖对账:`generation/structure/topics.rs`(topic 拆分)与 `generation/expand.rs`(SourceCoverage 台账 × 课时锚点重对账,covered/skipped/uncertain 三态)。
  - canonical:`generation/triage.rs` 规则预分类(Q12 顺序:非翻译原文 → README → 语言 → 最新 mtime;hash 对在此处分流)。
- **上游波**:[../learning-kb-course-v2/map.md](../learning-kb-course-v2/map.md)(全票关闭;其 t5 Resolution 与状态行里的「移交观察」即本波来源)。

## Decisions so far

- 三票均属实现细节硬化,无产品级未决;设计方向写死在各票,阈值/提示词措辞归实施时定。
- 保守语义不变:任何硬化不得引入新的静默丢弃路径;uncertain 兜底作为最后防线保留。

## Tickets

- [h1](tickets/h1-triage-batch-completeness.md) 分诊批回包完整性硬化：缺条目补判 + 单文件兜底修复,把「批缺条目→uncertain」的损失率打到接近零。
- [h2](tickets/h2-topic-rollup-coverage.md) 覆盖对账父子滚入：章级 topic 由其被覆盖的子 topic 滚入,消除「章已覆盖却报未覆盖」的噪音警告。
- [h3](tickets/h3-duplicate-canonical-tiebreak.md) 重复对 canonical tie-break:规整文件名优先于 mtime,正身文件当 canonical。

## Not yet specified

- 无。三票验收都含真机复验读数门槛,证据目录已给。

## Out of scope

- 分诊模型本身的质量(换模型/温度调优)——只修交互协议与解析,不换脑。
- 覆盖披露 UI 呈现(已由 v2 t5 落地,滚入只改数据语义,呈现面跟随)。
- 大书锚点偏窄(17/105 covered)与 provider SSE 挂起无超时——另立票,不在本波。

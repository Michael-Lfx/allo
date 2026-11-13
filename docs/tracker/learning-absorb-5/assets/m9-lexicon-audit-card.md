# m9 定义用语表与只读审计——验证登记卡

- 日期：2026-10-27；票：m9
- **无真机臂（票面）**：纯本地零 LLM（base 语料移植 + 审计纯函数 + 端点与标记），验证 = 夹具红绿（m4 Resolution 定案），无双臂需求。
- 实施分支真机相关性说明：
  - m9 主体（审计函数/端点/标记）随 m4 夹具红绿关票，本轮无 owed live arm——如实记「no live arm required (ticket)」。
  - 碎片面（m9f）的真机读数与 ENFORCE 臂见 [m9f 对照卡](m9f-fragment-verdict-comparison-card.md)——那是 m9 在 grow 打印面上唯一的运行时观测面，已覆盖。
  - `NOMI_LEARNING_LEXICON_ENFORCE` 缺省 off 维持不变（真机两臂均未开阻断，v1 纪律）。

---
id: rs1
title: live 调用档案——ArchivingCompleter 接生产缝，开关控制
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

live 桌面端学习域零调用留痕：2026-11-20 真机法证只能靠聚合日志行反推
时间解剖（draft 215s/review 348s 的内部构成只能猜），同题在 smoke 臂用
ArchivingCompleter 档案 30 秒出 token 级答案（reasoning_tokens 88%/91%）。
观测缺口=每次法证多付一小时税。smoke 已覆盖（learning_smoke.rs:231-232），
缺口只在生产。

## 已核验事实（2026-11-20 对码）

- `ArchivingCompleter`（nomifun-ai-agent/src/archiving_completer.rs）已
  具备全部能力：逐调用一行 JSONL（ts/method/ok/elapsed_ms/chars/usage
  含 reasoning_tokens/system/user/response/error），脱敏
  `register_redaction`，run 目录制。
- smoke 接线缝：`learning_smoke.rs:231-232`
  `service.set_generation_dependencies(knowledge, pipeline)`（pipeline=
  Archiving 包 live）。生产缝：`nomifun-app/src/services.rs:2922` 同名
  调用，当前直接传裸 completer。
- lesson agent 引擎不经管线 completer（OneShotDeps 自解析 provider，
  learning_smoke.rs:258 注释）——rs1 不动 lesson 链（首轮观测帧已有）。

## R（用户裁决 2026-11-20：先立票不开工）

- **R-a 开关控制**：默认 **off**；dev 渠道（--data-dir 含 -dev 或
  config 既有 dev 判定缝）建议默认 on，供真机法证随开随查。开关名建议
  `NOMI_LEARNING_LIVE_ARCHIVE`（沿 NOMI_LEARNING_* 词表惯例，
  switches.rs 单源）。
- **R-b 接线**：生产缝包一层 ArchivingCompleter，run 目录按日
  （`observation/learning-live/YYYY-MM-DD/`），不按 run（live 无 run
  概念）；ArchivingCompleter::new 现签名按 run_dir 建目录，若目录粒度
  不适配做最小改造（只放宽目录策略，不动行格式）。
- **R-c 隐私面**：行内含 prompt/回包全文——开关 off 时零写入（不是
  只不落盘不采集）；README/配置文档补一句「含全文，注意数据目录同样
  的访问边界」。脱敏复用既有 register_redaction（生产缝现无注册项，
  沿 smoke 惯例后置）。
- **R-d 范围外**：lesson 链逐调用留痕（smoke 首轮帧已有，live 补齐另
  立候选）；磁盘水位清理（观察项：learning-smoke 现存 5 run 共 ~250KB，
  live 按日会增长，清理策略留观察）。

## Acceptance

- 开关 off：生产行为零字节新增（无目录创建、无 IO）。
- 开关 on（dev 默认）：跑一轮真机生长，calls.jsonl 出现
  complete_json_with_thinking 行，elapsed_ms/usage 与日志行
  `learning coach llm call done` 一致（elapsed 差 <1s）。
- `cargo test -p nomifun-ai-agent`（Archiving 既有测试不红）+
  `cargo test -p nomifun-learning` 全量绿。
- 负优化判定：开关 off 下任何 IO/内存新增 = 回退重做；on 下写放大
  阻塞生成管线（archive 在锁内同步写——观察项，必要时改异步缓冲）
  = 换法。

## Resolution

已施工收口（2026-11-20，implement-spec 并行波）。

- **落地**（wave/rs1 4961e2e65，并入 feat/learn1116 merge 4e58d42a5）：
  5 文件 +185/−36。生产缝（services.rs 装配块）live 档案臂按日目录
  `observation/learning-live/YYYY-MM-DD/`；开关三件套单源
  switches.rs（`live_archive_enabled_value` 纯取值 + 进程读数）；
  ArchivingCompleter 目录策略放宽（`new_in_dir`，行格式零改动）；
  docs/architecture/learning.zh.md 补生产档案段 + 隐私句。
- **偏离 R-a 的裁决（已核安全）**：dev 渠道缺省 on 落地——缝=
  `crate::channel()`（`option_env!("NOMI_CHANNEL").unwrap_or("stable")`），
  未设=stable=生产默认 **off**（R-a 满足）；`run-dev.mjs:70` 显式设
  dev → 开发缺省 on。缺省值在接线处注入，channel 知识不渗入
  learning crate。
- **偏离已核验事实的修正**：生产缝并非裸 completer——o2 的
  `NOMI_LEARNING_CALL_ARCHIVE`（按 run）已占同一缝；本票保留 o2 臂，
  rs1 开关优先、二者皆开只挂一层（防双层全文落盘写放大）。
- **检查**：`cargo test -p nomifun-learning` 1014/0（本分支态，含新
  开关测试）；`cargo test -p nomifun-ai-agent` 1178/2——2 失败为
  openclaw/acp_session_sync 既有计时 flake（单跑绿、与本票无机制
  交集）；`cargo check -p nomifun-app` 过。合并点全量
  1022/0 + workspace check 过（2026-11-20）。
- **欠账（真机）**：live 生长轮核 calls.jsonl 行与日志行
  elapsed/usage 一致——机制与 o2 同缝先例，挂下次真机跑顺带核。
- **留观**：按日目录取进程启动日（跨天常驻不滚动，重启换日）；
  off 态零写入已由装配臂保证（off=不 mount）。

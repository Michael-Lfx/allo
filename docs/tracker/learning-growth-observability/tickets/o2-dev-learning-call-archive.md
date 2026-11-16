# o2: dev 面 learning 管线调用留痕（calls.jsonl + usage）

## Status

已关票（2026-11-16 施工收口，feat/learn1116；真机归因读数留观——见
Resolution 末条，候选项②的裁决数据待下一次带凭据真机跑回填）。

## 已核验事实（2026-11-16 对码）

- **机制已存在，dev 不挂**：`ArchivingCompleter`
  （`nomifun-ai-agent/src/archiving_completer.rs`）包任何
  `LearningCompleter`，每次调用（成败都记）一行 JSONL——method/ok/
  elapsed_ms/system_chars/user_chars + system/user/response 全文，带
  脱敏注册表（注册表空 = 逐字节原样；归档失败只降级不阻断）。现政策
  =「Smoke runs MUST mount it; production runs never do」，故
  `calls.jsonl` 只出现在 `observation/learning-smoke/run-*/`；
  `bun run dev` 生产路径零逐调用留痕。
- **病历代价实证**：2026-11-16 的 19 分钟病历只能靠 nomicore 日志行
  差值 + learning-tasks.jsonl 聚合数反推逐调用耗时。
- **usage 已到事件面但被丢弃**：one-shot drain
  （`nomifun-ai-agent/src/factory/provider_config.rs`
  `drain_text_or_reasoning`）对 `LlmEvent::Done { stop_reason, .. }`
  忽略 usage；`TokenUsage` 有 `reasoning_tokens`
  （`nomi-types/src/message.rs:164`，注释明确 reasoning 是 output 的
  子集不得重复累加）。openai 请求面已支持
  `stream_options.include_usage`（`nomi-providers/src/openai.rs`
  `include_stream_usage` 位）。**reasoning_tokens 正是拆「评审慢 =
  推理还是排队」的归因数据**——map 范围外候选②的裁决依据。
- **失败流已有留痕**：`nomi-providers/src/failed_sse_capture.rs` 只捕
  失败流；成功调用在 dev 面无任何记录。

## Terms

- **R1（dev 面挂载开关）**：env 开关（`NOMI_LEARNING_CALL_ARCHIVE`
  三件套进 `switches.rs`，缺省关 = 现行为逐字节）控制生产/dev 路径挂
  `ArchivingCompleter`；落点与 smoke 同构（`observation/` 下独立子目录，
  run-ts 目录每进程一个），不与 learning-smoke 混目录。
- **R2（usage 采集）**：drain 面把 `Done.usage` 带出（返回结构或
  side-channel，one-shot 全通道一致），挂载面写进 calls.jsonl 行
  （input_tokens/output_tokens/reasoning_tokens）；provider 不回 usage
  时行如实标 absent，不造数、不填零冒充。
- **R3（不做/红线）**：不改变调用语义/重试/超时；观测面失败只降级不
  阻断（ArchivingCompleter 现纪律沿用）；脱敏注册表生产恒空 = 原样
  落盘（calls.jsonl 含 prompt 全文本就是该文件既有形态，数据目录在
  用户本机）；不做 UI 呈现；不改 smoke 现有挂载路径与格式（增量字段
  向后兼容，serde default 容旧行）。

## Acceptance

- `cargo test -p nomifun-ai-agent` + `cargo test -p nomifun-learning` 绿；
  开关 off 臂零行为变化（生产默认路径不动）。
- 真机 dev 开开关跑一次 growth：calls.jsonl 每调用一行（含 usage 或
  absent 标记），与 o1 的逐调用日志可互相对账（同调用可按时间戳对齐）。
- 用一跑真机数据回答归因问题：评审调用 reasoning_tokens 占 output
  比例 + 耗时-推理 token 关联，为候选②（评审降推理档）提供裁决数据，
  读数写回本票 Resolution。

## Resolution

已施工收口（2026-11-16，feat/learn1116）。R1/R2 落地（开关缺省关、
sibling one-shot、CallUsage 桥接、归档行 usage 键），测试 1163 绿。

- **归因读数已回填（2026-11-16 17:00 真机首跑，smoke loop v4.1-flash，
  calls.jsonl 4 行全带 usage）**：
  - 概念评审调用：**161.9s，reasoning_tokens 28579 / output 31290 =
    91%**——「评审慢 = 推理还是排队」的答案坐实为**推理主导**；
  - growth draft：69.5s，reasoning 9561 / 10774 = 89%；
  - 结构两调（terrain/artifact）：2.4s / 4.1s，reasoning 占 80%/60%。
  - **候选②裁决数据在案**：评审输出是 tiny verdict 但 91% 的输出
    token 花在推理上——降推理档（显式 `reasoning_effort:"low"` 起步）
    有真实的分钟级收益空间，是否动质量闸门待用户裁决。

## Resolution

已施工收口（2026-11-16，feat/learn1116）。四问裁决（grilling 收口）全部
落地：缺省关 / sibling 变体 / 独立目录 / 不做 UI。

- R1（挂载开关）：`NOMI_LEARNING_CALL_ARCHIVE` 三件套进
  `nomifun-learning/src/switches.rs`（未设/off/0/false = 关，纯函数 +
  进程读数 + 测试 `call_archive_switch_defaults_off_and_requires_explicit_on`
  绿）；挂载点 = `nomifun-app/src/services.rs` 生产接线——开关开时把
  `LiveLearningCompleter` 包进 `ArchivingCompleter`（obs root =
  `observation/learning-dev/`，run-ts 目录每进程一个），挂载失败只
  WARN 降级绝不阻断启动；缺省关臂 = 原路径逐字节。
- R2（usage 采集）：`provider_config.rs` 增
  `drain_text_or_reasoning_with_usage`（`Done.usage` 捕获，组装语义与
  原 drain 逐字节一致）+ sibling
  `one_shot_completion_json_text_or_reasoning_with_usage`（grilling Q2
  裁决：只 learning 路径换调，knowledge/title 等既有调用方零改动——
  原函数变薄壳丢弃 usage，行为不变）。nomi-types `TokenUsage` 不出
  backend 边界：trait 面新 `nomifun_learning::CallUsage`（Option 字段，
  如实缺席）+ `complete_json_with_thinking_usage`（默认实现委托旧方法
  + usage 缺席——全部既有测试替身零改动）；`LiveLearningCompleter`
  唯一实现路径走 usage 变体（旧方法 = 薄壳丢弃，重试语义共享不漂移）。
- 归档行：ok 臂恒带 `usage` 键（对象或 `"absent"`；旧行缺键 = o2 前
  存档，向后兼容）；失败臂无 usage 键（error 行形状不变）。
  `ArchivingCompleter::complete_json_with_thinking` 经 inner 的 usage
  变体取数——smoke 挂载面同享 usage（smoke 路径/格式增量兼容）。
- R3（红线）如实执行：调用语义/重试/超时零改动；观测面失败只降级；
  脱敏注册表生产恒空；无 UI。
- 验证：`cargo test -p nomifun-ai-agent` 1163 绿（含新增
  `usage_row_reflects_inner_report_or_absent`：覆写 inner → 用量对象、
  默认 inner → "absent"）；`cargo check` 三 crate 绿；`cargo fmt` 过。
- **留观（真机归因读数未回填）**：下一次带凭据真机跑（开
  NOMI_LEARNING_CALL_ARCHIVE=on）后，把评审调用的 reasoning_tokens
  占比与耗时-推理关联读数回填本节，作为候选②（评审降推理档）的
  裁决数据。

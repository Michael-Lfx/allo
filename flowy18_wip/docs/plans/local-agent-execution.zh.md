# Flowy 本机助理 · 执行与验收方案

> **状态**：P0、P2 实施；P1 于 2026-11-19 回退（清单任务、产物核验与交付物入口移除，入口恢复为普通对话，后续恢复）；P3 进入目标机能力探测\
> **基线**：`8aae3a5fb`，2026-11-19（香港时间）\
> **核心原则**：一条真正可验收的本机链路先行。一个输入入口，同一个 pi-rust loop，任务单独保有状态、权限和产物。\
> [设计与原始讨论](local-agent-design.zh.md) · [开源证据](local-agent-evidence.zh.md)
>
> P0–P2 的实施、门禁退出码和 Windows 证据见 [实施进度](local-agent/PROGRESS.md) 与 [待验证项](local-agent/BLOCKED.md)。

## 点子王审查

结论是先做小范围验证。用户注意力、Windows 桌面资源和对结果的信任决定能否扩大。以下三项都进入验收。

| 优先级与依据 | 可能失败的路径 | 最小证伪实验与约束 |
| --- | --- | --- |
| P1 · 推断 | 「解释这个文件」「修改并另存」「覆盖原件」识别不清，用户把交付物选项当作写入许可。 | 用同一份夹具测三种提示。解释只答，另存只写任务产物区，覆盖保留为待授权。核对输入文件哈希和实际写入路径。 |
| P1 · 源码风险与产品约束 | 两个任务争用目录或桌面，取消后旧事件污染新运行，旧历史快照可能在后面落盘。 | 用两个可控流阻塞并乱序完成，取消一个后制造迟到片段，再重启读取历史。两任务状态和配置独立。对未知副作用回到待核对，防止简单重跑。 |
| P1 · 推断 | Worker 自报完成，用户得到空文件、坏文件或错误页数。 | 用正常和故障产物做一次正反验证。任务完成必须读取实际文件并核验声明条件。恢复后可查看旧版本、失败原因和下一步。 |

待验证的关键假设是桌面侧可取得一个能生成和预览的 PPTX 管道，以及第一轮用户愿意沿自动提示直接发任务。前者先做目标机探测，后者用同一输入框的普通问答与目录清单对照。首个执行切片据此采用清单，PPTX 的完整方案仍保留。

### 点子王复审 · Markdown 与交付边界

**结论 · needs-attention。** 保留现有入口、任务、Worker 与 pi-rust 的分工。P2 纳入基础 Markdown，P1 补齐提交和文件写出的中断用例。以下是二次审查的三条阻断项。

| 优先级与依据 | 首次暴露的失败 | 最小证伪实验 |
| --- | --- | --- |
| P1 · 设计推断 | 助手回复包含外部图片、文件地址或伪造的交付链接。用户在阅读历史时触发联网，或把未经核验的本机路径当成产物。 | 用本机 HTTP 计数器、临时文件和不受支持的 URI 做夹具。加载、滚动和重开时请求为零，未授权目标始终不可打开。显式点击受支持链接时只走一次宿主打开入口。文字里声称交付仍无产物卡。 |
| P1 · 基于源码的推断 | 同一类型的 Assistant 行既可处于流式又可已落定。工具调用切断正文或会话切换时，展示错误版本、重复一段，或跳离当前阅读位置。 | 用确定性流回放晚到引用、未闭围栏、中文和表情，中间插入工具调用。流结束后与最终原文完整解析比较。人工延迟 A 的旧更新，切到 B，再回 A 验证各自的原文、行顺序与锚点。用例需让错误的「最后一行就是流式」判定失败。 |
| P1 · 设计推断 | 双击发送或重试时生成两个 Worker。另一个中断窗口出现在文件已写出、核验记录尚未提交时，恢复造成重复生成或误报交付。 | 重放同一提交 ID 并模拟回执丢失，查宿主只有一个任务或消息。用隔离文件夹在文件写出后强制结束，分别提供可核验与来源不明的文件。重启后应复用同一已核验产物，或进入待核对并保留文件。记录 Worker 启动次数、产物路径和哈希。 |

确定的约束是原始消息可重放、产物可核验、用户电脑的权限和资源由宿主管理。当前开源证据支持采用成熟解析器和原生排版。流式增量、独立高亮和消息内虚拟化仍由 Windows 数据决定。剩余问题是目标机上超长单条内容的可用上限，以及 GPUI 语义在 Windows 上的实际读回表现，分别进入 TC-09 和 TC-08。

## 分阶段落实

| 阶段 | 工作与交付 | 出口证据 |
| --- | --- | --- |
| **P0 会话执行可靠性** | 复用 `flowy-agent-core`，传递稳定会话 ID，一次性取得模型和工具配置。为流式事件和状态写入补 Attempt 识别与有序落盘。 | 两个会话并行、旧事件迟到、旧快照晚写的复现测试。完成后重开历史，双方状态均可复现。 |
| **P1 统一入口到真实本机交付（2026-11-19 已回退，后续恢复）** | 首页与会话使用同一提交结构。原路径承接普通问答，新任务路径承接指定目录清单。任务 Worker 从只读资料生成 Markdown 到自己的产物目录，Router 返回任务回执。支持待补资料、取消及重启后续办。 | 同一输入框完成问答和清单。两条来源不同的任务可并行；退出重进后续办对准原任务。软件读取并核验真实清单，原件哈希一致。 |
| **P2 GPUI 详情与历史** | 统一输入控件，任务优先显示待确认和产物。按轮折叠，保持锚点、状态语义和旧会话兼容。视觉验收先看空首页、普通对话、交付详情三张同尺寸稿。 | Windows 亮暗主题、窄窗和高 DPI 实跑。键盘、滚动、取消、待确认和打开文件均有可追溯记录。 |
| **P3 幻灯片与性能扩展** | 先运行目标机能力探测，取得一个可编辑、可预览的 PPTX 并按页数验收，再开放入口。对长历史量首帧与流式更新成本，证据支持时再加入解析缓存或惰性载入。 | 实际打开 PPTX、核对页数及重启后定位；普通对话、清单、幻灯片用同一入口与 loop。新缓存有切会话迟到结果测试。 |

P0 与 P1 合为首次可执行切片。仅在 P1 需要的地方接最小任务回执、等待和真实文件打开。完整视觉统一、持久化收敛和专业生成能力各自按验收分步推进。

### P2 Markdown 执行与验收

1. 从现有 `flowy-agent` 投影明确每段正文是否已落定，在 `flowy-studio` 内验证解析器接线与 GPUI 主题。优先试 `pulldown-cmark 0.13` 的精简特性，核对许可证、锁文件和 Windows 构建。先把普通回复、已有历史和同轮交错工具结果做通。
2. 接入原生链接与复制操作，测试暗亮主题、窄窗和至少一个高 DPI 档。对代码复制、交付控件和链接执行键盘路径。用 Windows UIA 能读取的节点核对名称、角色、焦点和状态，配截图或录屏核对几何、折行和滚动。UIA 缺失单独记为待验证，键盘和截图只覆盖相应交互。
3. 在同机固定数据测纯文本基线和候选版本。分别用 1000 行历史、单条含长代码和宽表格的回复，记录打开首个可读帧、可见列表重排耗时和流式收尾的锚点，保留样本大小、窗口尺寸、DPI、测量方法和原始数据。开工时用纯文本基线确定可接受预算和单条解析上限。超限时核对纯文本可读、原文可复制，滚动与取消生成仍有响应。若基线测量受阻，记录受阻原因与未决预算。

| 用例 | 出口证据 |
| --- | --- |
| TC-08 内容和边界 | 同一组夹具比较期望结构、原文复制、代码内容、路径和请求计数。包含常用格式、中文和表情、HTML、外部图片、未知协议、假产物链接。键盘与 Windows 辅助功能核对两条证据分开记。 |
| TC-09 更新和定位 | 回放不完整围栏和晚到引用，结束后与完整渲染对照。取消时正文仍可读。展开、折叠、切换会话和变宽变窄时定位到原行与行内偏移，原行不可见时定位到该轮摘要。附两类性能基线及对照。存在后台解析时额外控制旧结果的到达顺序。 |

P1 的目录清单使用真实 Markdown 文件作为交付物，按路径、内容与读取验证。（该实现已于 2026-11-19 撤回，后续恢复。）P2 渲染正文的效果单独验收。P3 的性能候选在 TC-09 发现可归因的瓶颈后再进入实现。完整用例参见 [设计与验证矩阵](local-agent-design.zh.md#5-验收与验证顺序)。

## 基线和统一命令

在 `flowy18_wip` 目录的 Windows Git Bash 中使用以下路径。同步遵循 [Windows 构建记录](windows-build-rebase-pitfalls.md)。

```bash
mkdir -p build-test/tmp-probe
export CARGO_TARGET_DIR=$(cygpath -w "$PWD/build.noindex")
export TEMP=$(cygpath -w "$PWD/build-test/tmp-probe")
export TMP="$TEMP"

cargo fmt --all -- --check
cargo test --offline --locked -p flowy-agent-core -p flowy-agent -p flowy-auth -p flowy-app -p flowy-studio -p flowy-ui -p flowy-i18n
cargo clippy --offline -p flowy-ui -p flowy-studio -p flowy-app -p flowy-i18n --all-targets --locked -- -D warnings
cargo clippy --offline -p flowy-agent-core -p flowy-agent --lib --locked -- -D warnings
cargo check --offline -p flowy-desktop --locked
```

本轮在 `8aae3a5fb` 用 Rust 1.100.0 实测，上述五个 Cargo 命令均退出 0。测试统计按 crate，Doc-tests 未另计。

| crate | 通过 | 失败 | 既有 ignored |
| --- | ---: | ---: | ---: |
| `flowy-agent-core` | 39 | 0 | 2 |
| `flowy-agent` | 28 | 0 | 1 |
| `flowy-auth` | 12 | 0 | 0 |
| `flowy-app` | 24 | 0 | 0 |
| `flowy-studio` | 20 | 0 | 0 |
| `flowy-ui` | 20 | 0 | 0 |
| `flowy-i18n` | 4 | 0 | 0 |

补充命令 `cargo clippy --offline -p flowy-agent-core -p flowy-agent --all-targets --locked -- -D warnings` 在同一基线退出 101，现有 10 条均在测试代码。保持现有告警规则，用文件、lint 名称和数量核对增量。将这些历史项的清理独立处理。

| 测试代码位置 | 既有 lint |
| --- | --- |
| [core engine](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/engine.rs#L801-L816) | `used_underscore_binding` × 1 |
| [core session](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/session.rs#L250-L280) | `field_reassign_with_default` × 1、`assert_is_empty` × 1 |
| [core builtin](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/tools/builtin.rs#L651-L711) | `semicolon_if_nothing_returned` × 2 |
| [core pi adapter](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/tools/pi.rs#L199-L325) | `unnecessary_literal_bound` × 2、`match_wildcard_for_single_variants` × 1 |
| [core registry](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/tools/registry.rs#L330) | `semicolon_if_nothing_returned` × 1 |
| [agent lib](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent/src/lib.rs#L39-L42) | `map_unwrap_or` × 1 |

Cargo 发出既有 `gpui-pre-macros` profile 配置提示。以上是代码基线，后续产物生成和真实 Windows 交互需要独立证据。实机验收用隔离实例和临时目录，普通对话使用现有授权路径。可用时优先让本机 agent-desktop 或原生 UIA 读回焦点、控件状态和文本，再用截图核对布局和主题。异步读回与实际文件核对分别记录。

## 首个可复制任务书

下段是 P0 + P1 的执行指令，全文可单独投给一个执行者。分阶段范围详见本文件上文。

````text
你负责实现 Flowy 本机助理的第一个切片。开发依据为本任务书和 flowy18_wip/docs/plans/local-agent-design.zh.md。遇到缺条件的事项，写入 flowy18_wip/docs/plans/local-agent/BLOCKED.md，继续做独立项。每完成一步更新同目录 PROGRESS.md。恢复时先读它。目标是从同一个入口回答问题或产出真实本机清单，并证明任务可以独立运行和续办。优先级为用户数据安全、可验证交付、界面完善。

## 我替领导拍的板

- 已定：模型执行 loop 用仓库锁定的 pi-rust。本机执行，任务层持有授权和状态。
- 建议默认值：首个清单为 Markdown，任务只读指定目录并在自己的产物目录创建新文件。猜错的代价是更换清单格式，入口和运行契约仍可复用。
- 建议默认值：一项任务一个主 Worker，两个独立任务可并行。选择只读清单优先验证恢复与隔离，PPTX 属于下一阶段。

## 界限

仅可修改 flowy18_wip/crates/{flowy-agent-core,flowy-agent,flowy-studio,flowy-app}/src 下与本切片相关的文件、flowy18_wip/crates/flowy-i18n/locales 的必要对称翻译、上述 PROGRESS.md 与 BLOCKED.md。其余只读，含原有两份 model-tiers 未跟踪方案、依赖、CI 和判卷命令。可新增有针对性的回归测试，保留所有现有验收。保留已有会话格式的读入路径。实跑使用临时样例目录，原件保持字节一致。真实账号与网络按现有授权处理。

## 现状与任务 0

基于 8aae3a5fb。Windows Rust 1.100 实测 147 通过、0 失败、3 个既有 ignored。执行 flowy18_wip/docs/plans/local-agent-execution.zh.md「基线和统一命令」中的 fmt、锁定依赖的离线测试、两条通过的 Clippy 和桌面 check。补跑 agent 全目标 Clippy 对照同节 10 条既有测试告警，记录退出码和七 crate 分项。基线有偏差时记录证据及影响。先读现有 Engine、RunFence、ToolRegistry、StudioPanel 和 ChatFeed。开工回执写在 PROGRESS.md，最多十行，说明目标、顺序和最大风险。

## 任务 1 · pi-rust 运行可靠性

给当前 loop 传稳定会话 ID；同一次执行取一份模型、工具和工作目录配置。用当前 Attempt 校验状态与流式更新，保证历史有序提交。测试两个会话并行、旧事件迟到与旧快照晚写。分别比对重新打开后的两个会话，确认无丢失或串写。反向验证故意放过一种迟到更新，先看到测试红，再恢复并输出绿。

## 任务 2 · 两条入口路径

复用首页和普通对话共同的提交结构。普通提问继续进入对话。明确要求给指定目录生成清单时，创建稳定 Task ID，请求绑定只读输入和独立产物目录，Worker 通过 pi-rust 运行受宿主检查的工具。交付一份能打开而且可按已知夹具逐条核对的 Markdown 清单。任务回执展示范围，完成卡能打开真实文件。用缺资料触发等待，退出重开后给原任务补资料。取消的运行不得写入后续任务。先用显式选项验证路径，自动识别不确定时只问一个所需参数。

同一提交 ID 重放必须返回同一任务或会话消息。用双击、回执丢失和文件已写但交付记录未提交三种场景做 TC-10。重开后按动作记录核对原产物，已知结果只登记一次，未知结果保留待核对及现场。报告任务、Worker 启动次数、输出路径与哈希。Markdown 原生渲染在 P2 单独实施。

## 规矩

测试夹具记录输入目录、文件名和哈希。越界、重解析点和已有文件覆盖无法确认时要拒绝或明确等待。用真实注册工具与产物核验；脚本化流仅替代模型响应。不得跳过测试、改阈值、放宽现有断言、用零字节假成品或改 CI。制造一次空产物或授权不足，留下安全分支的可复验证据。GPUI 实跑区分未验证、受阻、已通过。单项三次失败后记阻塞并完成独立项。提交保留人类身份，按仓库流程，不推送。

## 完成条件

1. 普通问答不建任务；两项目录任务的输出可读、各自匹配输入，且并发与重启续办用例通过。原件哈希一致，错误产物不报已交付。
2. fmt、原 UI Clippy、agent 生产库 Clippy、七 crate 测试与桌面 check 均通过。补充的 agent 全目标 Clippy 无新增 lint。用例数量不低于各 crate 基线，ignored 不新增，变更符合白名单。交付贴实际退出码、负例及恢复证据，附 PROGRESS.md、BLOCKED.md（无阻塞写「无」）。缺实机证据时报告缺口。
````

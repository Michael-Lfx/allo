# 上游契约漂移修复：完成交接（2026-09-15）

> 用途：上下文压缩/换会话后的恢复入口。所有结论与证据均已落盘在
> `docs/architecture/` 三份文档中，本文件只做索引、状态与待办。
>
> 分支：`fix/upstream-contract-drift`（未推送；提交数量以
> `git rev-list --count origin/main..HEAD` 为准，清单见 §2）
>
> 工作树：干净；基线：`f0e33897d`（main）

## 1. 一句话状态

六个客户端修复已实现、单测与真实网络验收通过；OBS-1 受控采样完成并判定
"保持串行调度"；**仅剩推送分支 + 创建 PR**（以及长期的 OBS 样本积累）。

## 2. 提交清单（按时间顺序）

```text
# 调查与计划（4 + 1）
86bbffe2b docs: record upstream contract drift investigation findings
570b30b74 docs: add backup dev environment findings to contract drift record
442a8641d docs: expand fix rationale and rejected alternatives for review
54f991db2 docs: reconcile fix plan with cross review findings
d148d4020 docs(architecture): 制定上游契约漂移修复计划

# 六个修复
854809a75 fix(provider): 协商工具请求的 reasoning_effort
0cffa50be fix(web): 按名称发现 You 工具并恢复失败探测
f05aeba7e fix(provider): 协商模型输出 token 上限
48e4624df fix(provider): 兼容 Gemini 嵌套工具 schema
28e47613a fix(agent): 忽略未广告工具的纯进度预览
d3dfdbfdb fix(provider): 限制初始请求总等待时间

# 测试与验证
bcc752921 test(provider): 补齐契约协商边界回归
cacb18ba3 test(conversation): 覆盖超时故障的模型转移
d6f89fc97 test(web): 覆盖冷却后并发重发现的单飞
a04d609be test(provider): 增加 90s 初始协商 deadline 手动验收
aa8a1a7d1 test(web): 增加 you.com 实时契约手动验收
275a8144f test(web): 增加 OBS-1 受控采样工具

# 文档回填
14f46b1a1 docs(architecture): 记录 OBS-1 历史基线
173edda48 docs(architecture): 回填上游契约漂移修复证据
b6f2de954 docs(architecture): 补充工具链与基线限制说明
0288700b8 docs(architecture): 记录补充测试提交与覆盖说明
c8d7cf42e docs(architecture): 回填真实验收记录
60953f132 docs(architecture): 整理运行时影响说明与 OBS-1 采样记录

# 交接
1a1444218 docs(handoffs): 记录上游契约漂移修复完成交接
```

改动范围：19 个文件，+3438 / -51（4 个 Rust crate + 3 份文档 + 1 个 example）。

## 3. 文档索引（恢复时先读这三份）

| 文档 | 内容 |
| --- | --- |
| `docs/architecture/upstream-contract-drift-investigation.zh-CN.md` | 背景、两处现场证据、同类问题矩阵、方案取舍、OBS 历史基线（§9） |
| `docs/architecture/upstream-contract-drift-execution-plan.zh-CN.md` | v4 执行计划、实施记录（§15）、真实验收记录（§16）、决策基线 |
| `docs/architecture/upstream-contract-drift-runtime-impact.zh-CN.md` | 运行时影响逐项说明、复现命令、OBS-1 采样方法与结论 |

工具与手动用例：

- `crates/agent/flowy-web/examples/obs_sampling.rs`：OBS-1 受控采样（真实托管搜索链）。
- `nomi-providers` ignored 用例：90s 黑洞 deadline 计时。
- `flowy-web` ignored 用例：you.com 实时契约探针。
- OBS 原始捕获（本机临时目录）：`%TEMP%\obs-managed-search-*.jsonl`。

## 4. 验证结果摘要

自动化（全绿；两个既有失败见 §6）：

```text
nomi-providers                 243 passed（187+13+17+26），1 ignored
flowy-web                      189 passed，1 ignored
nomi-agent                     830 passed（787+11+32）
nomi-config                    216 passed
nomifun-ai-agent send_error    33 passed
nomifun-conversation failover  9 passed
cargo check -p nomifun-cloud   Finished
```

真实网络验收：

| 项 | 结果 |
| --- | --- |
| 90s 初始协商 deadline | ✅ 实测 90.03s，`InitialRequestTimeout`，无重试叠加 |
| GPT5.6-Sol 工具 schema + effort 双协商 | ✅ 真实网关：两次降级后 `terminal: ok`，无 `all_channel_models_failed` |
| qwen3.8-flash + web_search | ✅ 成功（web 宿主走默认 DDG；托管链由实时探针覆盖） |
| you.com 实时契约 | ✅ 按名发现 + 真实 `you-search` 解码成功（2.66s，5 hits） |
| gemini 输出上限 | ⚠️ 该账号通道接受 128000，现场拒绝不可复现；65536 仅 wiremock 覆盖 |
| 超时故障模型转移 | ✅ failover 端到端用例（两次发送、一次 rebuild、写入下一候选） |

OBS-1 受控采样（30 次探针）：

```text
样本 A 全链路 20 次：parallel 20/20，P50 1066ms / P95 2590ms，0 fallback / 0 全失败
样本 B 禁用 parallel 直达 you 10 次：you 10/10，P50 1970ms / P95 2255ms，0 schema_mismatch
决策门：P95 ≤ 6s 且全失败 0% → 保持串行调度，Stage 8 关闭
主样本（≥100 条真实调用）未达到 → 标记“证据不足”，不作长期依据
```

## 5. 恢复后的待办

1. **推送 + PR（唯一可立即完成的交付动作）**
   - `git push -u origin fix/upstream-contract-drift`
   - PR 描述草稿见 §7；合并前确认仓库 Git 归属规则（无 AI attribution）
2. **OBS-1 长期采样（未来工作，不阻塞合并）**
   - 修复上线后真实使用中累计 ≥100 条，再跑
     `cargo run -p flowy-web --example obs_sampling` 复核决策门；若触发
     "P95>6s + 串行等待≥50% + DDG 3s 成功率≥90%" 再考虑 Stage 8 hedge。
3. A3 类验收（可选）：需要具备 65537 输出上限的通道/环境。

## 6. 既有失败与环境限制（非本分支引入，勿误判）

- `nomi-config hooks::tests::test_hook_timeout`：Windows 无 `sleep` 可执行文件，
  基线同样失败。
- `nomi-agent badcase_regression_test::a_round_that_keeps_truncating_stops_at_three_passes`：
  基线同样失败。
- `cargo check --workspace`：本机 40 分钟未完成，已改为依赖面核对
  （`ProviderError` 消费方无新增风险）。
- `bun run check`：在既有 UI typecheck 阶段失败（videoCanvas / analytics 测试），
  本分支未触及 UI 文件。
- `rustfmt.toml` 设置 `disable_all_formatting = true`，`cargo fmt` 是既定 no-op。

## 7. PR 描述草稿（可直接粘贴）

```text
## 背景
- 用户反馈（2026-09-12）：GPT5.6-Sol 带工具对话报"模型服务商暂不可用"，
  实际是网关拒绝 tools + reasoning_effort（HTTP 500 → 被当瞬时错误重试 2 次）。
- 本机复盘（2026-09-15）：托管搜索 You 通道因工具数量契约漂移被永久禁用；
  另发现未广告工具进度会终止回合、初始请求最长可挂 90–231s。
- 完整证据与取舍见 docs/architecture/upstream-contract-drift-*.zh-CN.md。

## 修改（纯客户端，无 DB/接口/配置变更）
- fix(provider): 工具请求命中 effort 拒绝时降级 reasoning_effort=none 并记忆；
- fix(web): You 按名发现、发现只缓存成功、SchemaMismatch/ToolMissing 10 分钟冷却、
  调用期清理陈旧缓存；
- fix(provider): supported-range 拒绝时解析 inclusive/exclusive 并向下收敛 max_tokens；
- fix(provider): 识别 Gemini parameters.any_of 文案 + 组合分支归一化（fixture 驱动）；
- fix(agent): 未广告 ToolUseDelta 仅告警忽略；最终未广告 ToolUse 仍硬拒绝；
- fix(provider): chat/completions 初始协商 90s 绝对 deadline（不可重试）。

## 验证
- 单测：nomi-providers 243、flowy-web 189、nomi-agent 830、nomi-config 216、
  send_error 33、failover 9；
- 真实网络：90s deadline 实测 90.03s；GPT5.6-Sol 双协商成功；you.com 实时
  发现+检索成功；超时故障 failover 成功；
- OBS-1：30 次受控探针，parallel 20/20、you 10/10、0 schema_mismatch，
  决策门判定保持串行调度。

## 未验证/限制
- gemini 65537 上限拒绝需具备该限制的通道，当前仅 wiremock 覆盖；
- 托管搜索在 web 宿主按设计关闭，桌面宿主端到端由实时探针替代；
- OBS 长期结论需要 ≥100 条真实样本（Stage 8 暂关闭）。
```

## 8. 恢复检查清单

```text
git status --short --branch                  # 期望：fix/upstream-contract-drift，干净
git log --oneline origin/main..HEAD          # 期望：§2 的完整提交序列
cargo test -p nomi-providers -p flowy-web    # 期望：243 + 189，各 1 ignored
```

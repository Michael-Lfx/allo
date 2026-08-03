# Managed Fetch 当前状态与后续交接

```yaml
status: COMPLETE_LIMITED_ENABLEMENT
date: 2026-08-03
branch: feat/fetch-optimization
code_baseline: 9c7133ea2475c50780f8b16a38705233d5d8a32b
main_merge_base: 46fc4af1e8584a8a82612e688e325e32f64c9a41
worktree_at_snapshot: clean
remote_delivery: local_commits_only
```

## 当前结论

Managed Fetch 已作为 Desktop 的默认 `EvidenceBacked` 能力落地。模型和宿主仍只看到
现有 `web_extract`；Parallel、MCP Fetch 工具、Provider 名称、Payload 和 `final_url`
均留在 `flowy-web` 深模块内部。

当前默认远程回退范围只有：

- PDF；
- JavaScript Shell；
- Empty Content。

普通 HTML Local 成功时不会联系 Remote。Unsupported Document、DNS、TLS、Network、
Timeout 仍为 Deferred 或 Local-only；401/403/404/410/429、挑战页、登录墙、付费墙、
私网和敏感 URL 继续禁止远程出站。

紧急回滚方式：设置 `NOMIFUN_MANAGED_FETCH_MODE=off` 并重启 Desktop。空白或非法值
同样 fail-closed 为 `Disabled`。没有增加数据库配置、UI 开关或 Web Host 开关。

## 已落地的架构边界

- Desktop 和 `nomifun-ai-agent` 只选择 `ManagedExtractMode`，不选择 Provider、endpoint
  或健康策略。
- `ManagedProviderFactory` 组装 Search、Extract 和 Lifecycle；Parallel 初始化失败时
  保留独立 Search fallback，Extract 自动退化为 Local-only。
- 产品 rollout profile 与 Provider capabilities 分离，最终资格为安全策略、profile、
  capabilities 和预算的交集。
- `RemoteExtractProvider` 为 crate-private 深模块接口；Evaluation warmup 不在生产接口中。
- Parallel Search 与 Fetch 共享同一 runtime 和 endpoint health，并由 exactly-once lifecycle
  统一关闭；Fetch 工具级故障不会禁用无关 Search Provider。
- `ParallelMcpCallPolicy` 是不可绕过的网络前授权边界；Evaluation quota/control 只能附加
  配额和证据，不能替代生产安全策略。
- Remote 结果只按 canonical requested URL 精确归属。Missing、Extra、Malformed、
  Unmatched 或 Dropped item 会整批 fail-closed，并恢复每项原始 Local 错误。

## 出站安全不变量

Fetch 的网络调用必须同时满足：

- 工具名为 `web_fetch`；
- 参数键精确为 `urls` 和 `full_content`；
- `full_content=false`；
- URL 数量为 1–3；
- 每个 URL 重新通过 `prepare_remote_url(..., false)`；
- 准备后的 outbound URL 与实际发送值完全一致；
- 禁止非 HTTP(S)、私网、凭据和敏感 Query/Fragment；
- 初始调用、Session recovery、Tool rediscovery 合计最多三次；第四次在网络前阻断。

日志只允许记录计数、分类和耗时。禁止记录 URL、Query、Fragment、稳定 URL Hash、
正文、Marker 原文、用户问题、Conversation、Cookie、Authorization、Header、原始 MCP
Payload 或原始 Provider Response。

## 自动化证据

代码基线完成过以下验证：

- `cargo test -p flowy-web`：162 项通过；
- `cargo test -p flowy-web --features fetch-eval --all-targets`：203 项库测试和 4 项集成
  测试通过；
- `cargo test -p nomifun-app --features managed-search --lib managed_web`：3 项定向测试通过；
- `cargo check -p flowy-web --no-default-features`：通过；
- `cargo check -p flowy-web --features fetch-eval`：通过；
- `cargo check --workspace`：通过，只有仓库既有 warning；
- `cargo fmt --all -- --check`、`git diff --check`：通过；
- `.github/workflows` 下 `.yml`/`.yaml` 数量为 0。

`bun run check` 中 typecheck、i18n、theme、icons、process-runtime-boundary 和
browser-platform-boundary 通过；最终只被仓库既有的 8 条 agent-vocabulary retired
reference 阻断。本分支没有修改这些无关基线文件。

## 真实 Provider 与 Desktop 证据

脱敏 post-enable Canary 位于 ignored 目录 `fetch-evaluation-raw/post-enable-v8/`：

- run ID：`019fc192-56b3-74b1-bb37-16370d503dac`；
- 9 个逻辑 attempt，9 个完成；
- 6 次实际 Fetch、0 次 Search、0 次 recovery；
- PDF：3/3 Remote 有效成功，Q3，warm P50 366 ms、P95 389 ms；
- JavaScript Shell：3/3 Remote 有效成功，Q3，warm P50 462 ms、P95 496 ms；
- Static HTML：3/3 Local 成功，0 次 Remote；
- source mismatch、dropped item、sensitive egress、retry-limit violation、
  cancellation-late result 均为 0；
- status/safety provenance 完整，运行以 `completed` 结束。

Owner 已在 `NOMIFUN_MANAGED_FETCH_MODE` 未设置的 Desktop 会话中执行 HTML、公开 PDF、
JavaScript Shell 三项 `web_extract` 验收，并正常关闭应用。该开发启动器没有把本次
stdout 持久化到历史日志文件，因此不补写或推断逐请求计数；最终人工验收与自动
Canary 证据保持分层，不把人工结果计入正式 Admission 统计。

## Git 与交付状态

- 当前功能代码基线：`9c7133ea2475c50780f8b16a38705233d5d8a32b`；
- 分支没有 upstream，所有提交仅在本地；
- 未 Push、未建 PR、未创建或修改 GitHub Actions；
- 工作区在写入本状态文档前为 clean；
- ignored 评测证据保留在本地，未删除、未暂存。

关键提交：

- `c5f955eb` `test(web): define remote extract provider contracts`
- `01359f73` `refactor(web): deepen managed extract provider assembly`
- `6453ed50` `fix(web): make managed MCP egress safety non-bypassable`
- `24838484` `feat(app): enable evidence-backed managed fetch on desktop`
- `b6712533` `fix(web): close managed fetch policy edge cases`
- `9c7133ea` `docs(web): record final managed fetch desktop acceptance`

## 证据边界与剩余事项

当前完成的是有限类别的生产灰度接入，不是完整公开 Admission Campaign。以下事项必须
另开分支、重新审批，不能从当前结论自动推导：

1. 每类 15–20 个独立 URL 的正式公开 Admission；
2. Unsupported Document、DNS/TLS/Network/Timeout 的扩面；
3. 私有业务 PDF 或 Browser 最终 URL；
4. Browser→MCP 自动循环；
5. UI/数据库 Provider 选择器或运行时用户开关；
6. 额外 Fetch Provider 的产品接入。

一个可维护性缺口仍然存在：Desktop `bun run dev` 的 stdout 没有形成可恢复的脱敏
运行证据。若未来需要把人工验收升级为可审计发布门禁，应另行设计只保存计数和
provenance 的日志 sink；不得通过记录 URL、正文或用户问题来解决。

## 后续续接入口

若只维护当前能力，先阅读：

- [Managed Fetch Production Admission](./2026-08-02-managed-fetch-production-admission.md)
- [Managed Fetch Provider Maintenance Guide](../../architecture/web/managed-web-fetch-provider-maintenance.md)
- [Managed Fetch Policy](../../architecture/web/managed-web-fetch-policy.md)

开始任何扩面前，先执行：

```powershell
git status --short
git branch --show-current
git rev-parse HEAD
git diff --check
Get-ChildItem .github/workflows -File -ErrorAction SilentlyContinue
```

必须保护现有本地提交和 ignored 评测证据；禁止 reset、`git add -A`、Push、PR 和
GitHub Actions，除非仓库 Owner 另行明确授权。

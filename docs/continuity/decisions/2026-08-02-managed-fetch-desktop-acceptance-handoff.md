# Managed Fetch Desktop 验收交接

> 已由 [2026-08-03 Managed Fetch 当前状态与后续交接](./2026-08-03-managed-fetch-current-state.md)
> 取代。本文件保留为默认启用前的历史验收说明，不再代表当前分支状态。

日期：2026-08-02
分支：`feat/fetch-optimization`
当前 HEAD：`99e2864efdfd2cb304a4aa851e1860ba8ffb042e`
Goal：等待 Desktop 人工验收，默认值尚未启用

## 当前已完成

- 已将 Parallel MCP Fetch 接入 `EvidenceBacked` 模式。
- 已完成生产 MCP Safety Gate：Fetch 参数、公开 URL、敏感信息、恢复调用上限均在网络前校验。
- 已完成类别策略：PDF、JavaScript Shell、Empty Content 可远程；普通 HTML、Unsupported、Network、Timeout 等保持 Local-only 或 Deferred。
- 已完成自动 Canary：9 个逻辑 attempt、6 次实际 Fetch、0 次 Search；PDF 和 JS 均成功，HTML 控制为零远程调用。
- Safety 报告五项均为 0，工作树干净，结果记录在 [Canary ADR](./2026-08-02-managed-fetch-desktop-canary.md)。
- 当前未设置环境变量时仍为 `Disabled`；`off` 仍是 Local-only 紧急关闭开关。

## 为什么还需要 Desktop 实测

自动 Canary 验证了生产 Harness 和 MCP Provider，但没有证明 Desktop 进程启动时读取环境变量、模型实际选择 `web_extract`、UI 能展示可读结果，以及应用关闭后状态正常。这些属于宿主运行时和用户通道验收，不能由 Wiremock 或库测试替代。

只需要一次 Desktop 会话，不是正式 30 次 Pilot，也不需要私有 PDF。

## 三项实测分别验证什么

| 案例 | 验证目标 | 通过条件 |
|---|---|---|
| `public-static-example-domain` | 普通静态 HTML 不应浪费 MCP 配额 | `web_extract` 返回可读正文；`remote_attempted=false`、`remote_success_count=0` |
| `public-pdf-w3c-dummy` | Local PDF 失败后触发 EvidenceBacked MCP fallback | 结果标为 `Pdf`；`remote_attempted=true`；远程正文可读且没有错误页 |
| `public-js-eslint-code-explorer` | JS Shell 失败后触发 EvidenceBacked MCP fallback | 结果标为 `JavascriptShell`；`remote_attempted=true`；远程正文可读且 Marker 内容正确 |

测试时必须使用 `web_extract`，不要改用 Browser；Browser 成功不能作为 MCP Fetch 证据。日志或反馈只需提供上述字段和成功/失败结论，不要粘贴完整 URL、正文、Cookie 或 Header。

## 实测启动方式

在 PowerShell 中启动 Desktop：

```powershell
$env:NOMIFUN_MANAGED_FETCH_MODE = "evidence-backed"
$env:RUSTC_WRAPPER = ""
bun run dev
```

案例 URL 可直接从版本化语料 [corpus.public.json](../../../../crates/agent/flowy-web/evaluation/corpus.public.json) 中按 ID 查找。普通 HTML、PDF、JS 各执行一次即可。

## 实测后的分支动作

- 三项都通过：提交一个独立变更，把 Desktop 未设置环境变量时的默认值改为 `EvidenceBacked`；保留 `NOMIFUN_MANAGED_FETCH_MODE=off`，重跑自动门禁和最多 6 次 post-enable Canary。
- 任一项失败：不改默认值，记录 `retain_experimental`，继续保持 Local-only 默认。
- 在验收结果返回前，不执行默认启用，也不声称生产准入完成。

本交接不授权 Push、PR、GitHub Actions、Browser→MCP 循环或扩展到 Unsupported/Network/Timeout 类别。

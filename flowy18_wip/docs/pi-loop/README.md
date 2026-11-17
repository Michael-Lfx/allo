# pi-rust 最小 loop 实验归档

[← 文档总索引](../README.md)

本目录保留 2026-11 的离线实验记录。实验工程（`experiments/pi-loop`）已删除，源码与依赖由上游仓库固定提交还原。

## 上游

- 仓库：<https://github.com/bigfish1913/pi-rust>
- 固定提交：`300c1f3b560b653b812a8877956dc1a1776629b7`（版本 0.24.2，MIT）
- 仅使用低层 `rpi-agent` 的 `run_agent_loop` 与 `rpi-ai` 的事件类型，未使用高层 `Agent`／harness。

## 结论

1. 低层 loop 可以直接调用，无需 fork。
2. 首次锁定 177 个 packages；平台相关解析包不等同生产新增依赖。
3. 真实 `run_agent_loop` 验证了文本增量、工具续跑、取消传播与迟到输出抑制、run 间模型切换、旧文本协议桥接。
4. 实验是脚本化 provider 的离线验证，不代表真实 HTTP/SSE 或云模型兼容性。
5. 生产迁移后来在 `flowy-agent-core` 完成，未复制实验工程。

## 证据文件

| 文件 | 内容 |
| --- | --- |
| PLAN.md | 实验目标、顺序与风险 |
| REPORT.md | 结论、实测、边界与接入建议 |
| MIGRATION-REVIEW.md | 迁移前对抗审查与计划 |
| PROGRESS.md | 逐轮执行记录 |
| BLOCKED.md | 当时未决事项 |
| dependency-tree.txt | 锁定依赖树（仅 rpi-agent/rpi-ai/rpi-telemetry） |
| final-test-output.txt | 最终 4/4 绿 |
| contract-test-output.txt | 契约负向测试 2 项 |
| mutation-red.txt | 桥接变异测试红（0/1） |
| cancel-test-output.txt | 取消传播与迟到输出 |
| bridge-test-output.txt | 旧文本协议桥接 |
| tool-test-output.txt | 工具续跑 |
| test-output.txt | 首次集成测试 |

## 复现

1. 克隆上游仓库并检出上述提交。
2. 按 `REPORT.md` 的“复现”一节创建独立 workspace，引用固定 SHA。
3. 按原测试文件重建探针；原始测试源码已随实验目录删除，可从本文件记录的上游契约与 `MIGRATION-REVIEW.md` 还原断言。

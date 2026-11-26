# Flowy 文档索引

Agent 阅读顺序：本页 → 对应分类 `README.md` → 目标文档。
根目录只保留本索引，具体文档按主题分类存放。

## 分类

| 分类 | 何时读 | 入口 |
| --- | --- | --- |
| GPUI 迁移 | **写任何 GPUI 代码前**：依赖锁定、输入控件、键上下文、布局陷阱、热补丁机制 | [gpui-pre-and-kit.md](gpui-pre-and-kit.md) |
| Windows 开发与热补丁 | **Windows 开发者与 Agent 首读**：`dx serve --hot-patch` 启动与排坑、编译/测试环境、rebase 踩坑 | [plans/windows-build-rebase-pitfalls.md](plans/windows-build-rebase-pitfalls.md) |
| 架构与决策 | 了解 agent 内核、工作契约、沙箱与 shell 等已定方案 | [architecture/README.md](architecture/README.md) · [Flowy v0 内核架构与实测索引](architecture/flowy-v0/README.md) |
| 计划与交接 | 接手未完成工作、查看待实现方案 | [plans/README.md](plans/README.md) · [Super Agent 界面交互与丝滑动效方案](plans/super-agent-ui-and-motion.zh.md) · [PPTX/PDF/JPG 预览依赖调研](plans/document-preview-dependencies.zh.md) |
| pi-loop 工作流 | 查 pi-rust loop 实验、证据与迁移结论 | [pi-loop/README.md](pi-loop/README.md) |
| 云端模型目录工作流 | 查账号模型目录、验证与阻塞记录 | [cloud-model-catalog/README.md](cloud-model-catalog/README.md) |
| 历史归档 | 查已被取代的早期记录 | [archive/README.md](archive/README.md) |
| 依赖许可 | 查看预览引擎回退字体的分发声明 | [预览字体许可](licenses/preview-fonts/README.md) |

当前资料库实施计划：[首页对话文件交付与资料库](plans/conversation-artifact-library.zh.md)。来源为首页对话的显式交付，删除原对话后文件独立保留。

Personal Agent 自动化开发计划：[对话式自动化开发计划](plans/personal-agent-automation-development-plan.md)（计划，2026-11-25；含 31 项任务、依赖图与 V2 验收映射）。

分批执行状态：[Personal Agent 自动化实施与验证记录](plans/personal-agent-automation-execution.md)（2026-11-26，PA-A01、PA-A02、PA-A04、PA-B01 已完成并验证，当前 4/31；下一项 PA-A03）。

## 维护规则

1. 新文档先放入对应分类目录，再更新该分类 `README.md` 和本页入口。
2. 文档内链接使用相对路径；跨分类引用用 `../<分类>/<文件>.md`。
3. 已完成的计划保留原文，状态写在分类索引里；被取代的文档移入 `archive/`。

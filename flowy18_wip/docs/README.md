# Flowy 文档索引

Agent 阅读顺序：本页 → 对应分类 `README.md` → 目标文档。
根目录只保留本索引，具体文档按主题分类存放。

## 分类

| 分类 | 何时读 | 入口 |
| --- | --- | --- |
| 架构与决策 | 了解 agent 内核、shell 等已定方案 | [architecture/README.md](architecture/README.md) |
| 计划与交接 | 接手未完成工作、查看待实现方案 | [plans/README.md](plans/README.md) · [PPTX/PDF/JPG 预览依赖调研](plans/document-preview-dependencies.zh.md) |
| pi-loop 工作流 | 查 pi-rust loop 实验、证据与迁移结论 | [pi-loop/README.md](pi-loop/README.md) |
| 云端模型目录工作流 | 查账号模型目录、验证与阻塞记录 | [cloud-model-catalog/README.md](cloud-model-catalog/README.md) |
| 历史归档 | 查已被取代的早期记录 | [archive/README.md](archive/README.md) |

## 维护规则

1. 新文档先放入对应分类目录，再更新该分类 `README.md` 和本页入口。
2. 文档内链接使用相对路径；跨分类引用用 `../<分类>/<文件>.md`。
3. 已完成的计划保留原文，状态写在分类索引里；被取代的文档移入 `archive/`。

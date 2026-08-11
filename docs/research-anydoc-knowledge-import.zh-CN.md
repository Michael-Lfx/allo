# AnyDoc 知识库文件导入研究

## 外部事实（来源：AnyDoc 官方仓库）

- AnyDoc 是 MIT 许可的纯 Rust 文档转换库，统一输出 GitHub-Flavored Markdown。
- 支持 `.doc/.docx/.docm`、`.ppt/.pps/.pot/.pptx/.pptm/.ppsx/.ppsm`、`.xls/.xlsx/.xlsm/.xlsb`、`.odt/.ods/.odp`、`.rtf`、`.epub`、`.csv`、`.pdf`。
- 默认按文件内容标记检测格式；CSV 没有可靠的内容签名，需要扩展名或显式 format。
- 错误包括 `unsupported`、`malformed`、`encrypted`、`resourceLimit`、`missingPart`、`io`。
- 文本型 PDF 可本地转换；图片型/扫描型 PDF 不提供 OCR，会被视为 unsupported。嵌入图片在 Markdown 中保留 alt 文本，原始资源只存在于 AnyDoc 文档模型中。

来源：

- https://github.com/firecrawl/anydoc/blob/main/README.md
- https://github.com/firecrawl/anydoc/blob/main/node/README.md

## 本仓库对应点

- `crates/backend/nomifun-knowledge` 的知识文档模型是目录内 `.md` 文件；`list_files`、树、搜索、挂载和回写都围绕 Markdown 工作。
- 现有 `POST /api/knowledge/bases/{id}/upload` 接收 JSON `{ files: [{ path, content }] }`，最终调用 `KnowledgeService::write_file`。
- 前端 `KnowledgeDetailPage` 当前只过滤 `.md`，并把 `File.text()` 结果发送到上述接口。

## 结论

AnyDoc 应位于上传入口和 `write_file` 之间：接收原始字节，转换成 Markdown，随后复用现有 Markdown 落盘、搜索、挂载、编辑和回写链路。下游不需要知道源格式。

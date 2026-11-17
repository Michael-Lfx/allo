# 早期环境调查记录（已被新记录取代）

当前状态见 [cloud-model-catalog/BLOCKED.md](cloud-model-catalog/BLOCKED.md)。以下保留开工时原始观察，MSVC初始化和下载问题现已解除；真实会话路由仍阻塞。

1. 基线命令未能进入测试：
   - 命令：`RUSTUP_TOOLCHAIN=1.97.1-x86_64-pc-windows-msvc CC=cl.exe CXX=cl.exe CARGO_PROFILE_DEV_DEBUG=0 cargo test -p flowy-auth --ignore-rust-version`
   - 结果：`ring v0.17.14` 构建失败，`ToolNotFound: failed to find tool "cl.exe"`，退出码 101。
   - 另一次经 `vcvars64.bat` 的尝试被 shell 返回 `The network path was not found`，未进入 Cargo。
   - 影响：无法核对任务书给出的 auth/agent 基线数字；继续执行与编译环境无关的纯解析、目录状态和静态修改，最终测试结果如实记录。

2. 推理请求契约未在当前文档中确认：模型目录 endpoint 是否可直接作为 LLM base_url、JWT 是否为推理凭证。当前实现只准备目录数据和选择状态，不宣称真实对话链路已验证。

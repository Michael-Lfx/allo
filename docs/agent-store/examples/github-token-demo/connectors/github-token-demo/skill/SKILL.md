---
name: github-token-demo
description: 示例技能，随连接器条目一起分发；说明这个连接器需要一把 GitHub Personal Access Token。
---

# GitHub 连接器（示例）

这个条目连的是 GitHub 的远端 MCP 服务，认证方式是一把**用户自己提供**的 Personal Access Token：

- 令牌由用户在连接器抽屉的「填入凭据」里输入，落在本机 `~/.agent-store/config.toml` 的 `[credentials]`，按 principal 键控；
- 请求时由 host 把 `Authorization: Bearer ${GITHUB_PAT}` 里的引用替换成真值再发出；
- 令牌值两个方向都不过线：协议响应里只有键名（`missing`）与元数据，没有值。

需要 `repo` 等工作区权限，具体范围见 [GitHub 令牌设置](https://github.com/settings/tokens)。

> 这是 `docs/agent-store/examples/` 下的示例技能，用来补齐条目结构；连接器组件本身来自同目录的 `mcp.json`，
> 技能是可选附属内容。

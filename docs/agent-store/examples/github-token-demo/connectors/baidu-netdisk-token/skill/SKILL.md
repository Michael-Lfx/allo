---
name: baidu-netdisk-token
description: 连接百度网盘：浏览与检索文件、管理目录、创建分享链接、查询容量。需要用户自备 Access Token。
---

# 百度网盘连接器

连的是百度网盘官方的远端 MCP 服务（SSE），认证方式是一把**用户自己提供**的 Access Token：

- 令牌由用户在连接器抽屉的「填入凭据」里输入，落在本机 `~/.agent-store/config.toml` 的 `[credentials]`，按 principal 键控；
- 请求时由 host 把 URL 查询参数里的引用替换成真值再发出，令牌值两个方向都不过线：协议响应里只有键名（`missing`）与元数据，没有值。

令牌在[百度网盘开放平台](https://pan.baidu.com/union/doc/Wm9sl0i0j)完成用户鉴权后获取，过期需重新获取。

能力面：文件/文档/图片/视频列表、文件详情、建目录、复制/删除/移动/重命名、按 URL 与文本上传、文件名与语义检索、创建分享链接、用户信息与容量查询。官方文档见 [baidu-netdisk/mcp](https://github.com/baidu-netdisk/mcp)。

> 上传**本地文件**需要 stdio 模式（`uv` + `netdisk.py`，见官方仓库的 Python 使用说明），本条目只覆盖远端 SSE 形态。

# 兼容性矩阵

## 平台

当前仅发布 **Windows x64** 构建；其余平台暂未提供，需按需立项后再开放。二进制在[下载中心](http://111.170.173.22:10014/downloads/)按目标命名分发。

| 操作系统 | 架构 | 状态 |
| --- | --- | --- |
| Windows | x86_64 | 已发布 |
| macOS | Apple 芯片（aarch64） | 未提供 |
| macOS | Intel（x86_64） | 未提供 |
| Linux | x86_64 | 未提供 |
| Linux | aarch64 | 未提供 |

> 下载按钮会按你的系统识别平台：仅 Windows x64 提供直链，其余平台引导至下载中心手动查看。

## 来源格式

| 来源 | 导入方式 | 兼容性状态 |
| --- | --- | --- |
| CodeBuddy Plugin | Importer → PluginSnapshot | compatible / compatible-with-adapter |
| WorkBuddy Skill | Importer → PluginSnapshot | compatible |
| WorkBuddy Connector | Importer → PluginSnapshot | compatible / manual-review |
| 未确认版权资源 | 标记 `pending-legal-review` | 不进入公开分发 |

## 连接器

- 至少一个 MCP Connector 可发现工具并执行受控调用；
- OAuth 采用标准 PKCE Loopback，凭据进入安全存储，运行时注入；
- 登录未贯通调用时，连接器标记为 `partial`，不显示 `connected`。

## 非目标（V1）

云端执行、多租户、HA、完整 Marketplace 审核后台、签名更新体系、任意 Hook/bin 执行——均不在 V1 范围。

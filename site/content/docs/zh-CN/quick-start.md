# 快速开始

Flowy Agent Store 把一个本地优先的 Agent 运行时打包成**单个可执行文件**，并内嵌完整的 Web UI。你不需要安装数据库、容器或任何常驻服务——下载、启动、打开浏览器即可。

## 1. 下载并启动

从 [GitHub Releases](https://github.com/your-org/flowy-agent-store/releases) 获取对应平台的二进制，直接运行：

```bash
flowy-agent-store serve
```

进程会在本机启动 App Server（默认 `http://localhost:8787`），并自动打开浏览器中的工作台。

> 本地优先：执行、凭据与运行状态都只存在于你的机器。云端仅用于定义、版本与分发的同步。

## 2. 导入 Agent

工作台支持从 CodeBuddy / WorkBuddy 格式的目录导入专家（Agent）、技能（Skill）与连接器（Connector）。导入后内容会被转为**不可变快照**，可在目录中查询与运行。

## 3. 启动一次运行

在目录中选择一个 Agent，点击「运行」，工作台会提交一次 Run。运行事件、计划（DAG）与产物（Artifact）会实时呈现在时间线与产物面板中。

## 下一步

- 阅读 [命令行用法](/zh-CN/docs/cli) 了解完整命令。
- 阅读 [架构说明](/zh-CN/docs/architecture) 理解 Runtime 与 App Server 协议的分层。
- 阅读 [兼容性矩阵](/zh-CN/docs/compatibility) 确认你的平台与来源格式支持情况。
